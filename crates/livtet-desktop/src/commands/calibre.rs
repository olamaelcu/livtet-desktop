//! Import a Calibre library.
//!
//! Reads a Calibre library's `metadata.db` through the bundled `calibre` Lua
//! plugin (which uses the host `sqlite` module) and persists each book into the
//! catalog via the shared [`super::import::persist_import`] path. The plugin is
//! embedded in the binary and materialized to a temp directory per import, so it
//! needs no separate installation.

use camino::{Utf8Path, Utf8PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use specta::Type;
use tauri::State;

use livtet_importer::{BookRecord, ImporterCover};
use livtet_types::DbId;

use super::import::{ImportError, ImportMode, LibraryTarget, persist_import};
use crate::types::{AppState, ArcMut};
use livtet_core::data::SharedState;
use livtet_core::search::SearchIndex;

const CALIBRE_INIT_LUA: &str = include_str!("calibre/init.lua");
const CALIBRE_PLUGIN_TOML: &str = include_str!("calibre/plugin.toml");
const CALIBRE_HOST_CONFIG: &str =
    "[capabilities]\nallow = [\"log\", \"sqlite\"]\n\n[signatures]\nrequired = false\n";

/// The tally returned to the UI after importing a Calibre library.
#[derive(Debug, Clone, Serialize, Type)]
pub struct CalibreImportSummary {
    /// Books discovered in the library.
    pub books: i32,
    /// Format files newly imported.
    pub imported: i32,
    /// Format files already present (hash match).
    pub duplicated: i32,
    /// Format files that could not be read or persisted.
    pub failed: i32,
}

/// Import every book in the Calibre library rooted at `library_path`.
#[tauri::command]
#[specta::specta]
pub async fn import_calibre_library(
    library_path: String,
    mode: ImportMode,
    state: State<'_, AppState>,
) -> Result<CalibreImportSummary, ImportError> {
    import_calibre_library_inner(
        state.plugin_host_path.clone(),
        Utf8PathBuf::from(&library_path),
        mode,
        &state.db,
        &state.covers_dir,
        &state.books_dir,
        &state.search_index,
    )
    .await
}

/// State-free body of [`import_calibre_library`]: scan the library with the
/// plugin host at `host`, then persist every readable format file.
async fn import_calibre_library_inner(
    host: Utf8PathBuf,
    library_root: Utf8PathBuf,
    mode: ImportMode,
    db: &SharedState,
    covers_dir: &Utf8Path,
    books_dir: &Utf8Path,
    search_index: &ArcMut<Option<SearchIndex>>,
) -> Result<CalibreImportSummary, ImportError> {
    let records = scan_calibre(host, library_root.clone()).await?;
    let books = i32::try_from(records.len()).unwrap_or(i32::MAX);

    let mut imported = 0;
    let mut duplicated = 0;
    let mut failed = 0;

    for record in records {
        let mut meta = record.meta;
        if let Some(cover_rel) = &record.cover_path
            && let Ok(bytes) = fs_err::tokio::read(library_root.join(cover_rel)).await
        {
            meta.cover = Some(ImporterCover {
                mime: mime_for(cover_rel).to_string(),
                data_base64: STANDARD.encode(bytes),
            });
        }

        for file in &record.files {
            let file_path = library_root.join(&file.path);
            let Ok(bytes) = fs_err::tokio::read(&file_path).await else {
                failed += 1;
                continue;
            };
            match persist_import(
                db,
                covers_dir,
                LibraryTarget {
                    dir: books_dir,
                    mode,
                },
                &meta,
                &bytes,
                &file_path,
                &file.format,
            )
            .await
            {
                Ok(outcome) if outcome.duplicate => duplicated += 1,
                Ok(outcome) => {
                    imported += 1;
                    index_edition(db, search_index, &outcome.edition_id).await;
                }
                Err(_) => failed += 1,
            }
        }
    }

    Ok(CalibreImportSummary {
        books,
        imported,
        duplicated,
        failed,
    })
}

/// Add a freshly-imported edition to the search index (best effort).
async fn index_edition(
    db: &SharedState,
    search_index: &ArcMut<Option<SearchIndex>>,
    edition_id: &str,
) {
    let Ok(edition_id) = edition_id.parse::<DbId>() else {
        return;
    };
    let db = db.db_conn();
    let guard = search_index.read().await;
    if let Some(index) = guard.as_ref() {
        let _ = index.add_edition(&db, edition_id).await;
    }
}

/// Materialize the bundled Calibre plugin, launch the host with a read-only
/// grant on the library's `metadata.db`, and call `scan`.
async fn scan_calibre(
    host: Utf8PathBuf,
    library_root: Utf8PathBuf,
) -> Result<Vec<BookRecord>, ImportError> {
    let db_path = library_root.join("metadata.db");
    if !db_path.is_file() {
        return Err(ImportError::new(
            "io",
            format!("no metadata.db under {library_root}"),
        ));
    }
    let source = library_root.as_str().to_owned();

    tokio::task::spawn_blocking(move || -> Result<Vec<BookRecord>, ImportError> {
        let temp = tempfile::tempdir().map_err(|err| ImportError::new("io", err))?;
        let temp_root = Utf8Path::new(
            temp.path()
                .to_str()
                .ok_or_else(|| ImportError::new("io", "non-UTF-8 temp path"))?,
        );
        let plugin_dir = temp_root.join("plugins").join("calibre");
        fs_err::create_dir_all(&plugin_dir).map_err(|err| ImportError::new("io", err))?;
        fs_err::write(plugin_dir.join("plugin.toml"), CALIBRE_PLUGIN_TOML)
            .map_err(|err| ImportError::new("io", err))?;
        fs_err::write(plugin_dir.join("init.lua"), CALIBRE_INIT_LUA)
            .map_err(|err| ImportError::new("io", err))?;
        let config = temp_root.join("host.toml");
        fs_err::write(&config, CALIBRE_HOST_CONFIG).map_err(|err| ImportError::new("io", err))?;

        // `RemoteOptions` takes std paths at this boundary.
        let options = stanchion::remote::RemoteOptions::new(host.into_std_path_buf())
            .config(&config)
            .plugins(temp.path().join("plugins"))
            .arg("--sqlite")
            .arg(db_path.as_os_str());
        let mut remote = stanchion::remote::RemoteRegistry::launch(options)
            .map_err(|err| ImportError::new("importer", err))?;
        let raw: serde_json::Value = remote
            .call("calibre", "scan", [serde_json::json!(source)])
            .map_err(|err| ImportError::new("importer", err))?;

        let serde_json::Value::Array(items) = raw else {
            return Err(ImportError::new("importer", "scan did not return a list"));
        };
        let mut records = Vec::with_capacity(items.len());
        for item in items {
            records.push(
                BookRecord::from_wire_json(item)
                    .map_err(|err| ImportError::new("importer", err))?,
            );
        }
        Ok(records)
    })
    .await
    .map_err(|err| ImportError::new("importer", err))?
}

fn mime_for(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn host_binary() -> Utf8PathBuf {
        let test_binary = std::env::current_exe().unwrap();
        let profile_dir = test_binary.parent().and_then(|path| path.parent()).unwrap();
        Utf8PathBuf::from_path_buf(profile_dir.join(if cfg!(windows) {
            "livtet-plugin-host.exe"
        } else {
            "livtet-plugin-host"
        }))
        .unwrap()
    }

    /// The subset of Calibre's `metadata.db` schema the bundled plugin queries.
    const CALIBRE_SCHEMA: &str = "
        CREATE TABLE books(id INTEGER PRIMARY KEY, title TEXT, sort TEXT, path TEXT, pubdate TEXT, has_cover INTEGER);
        CREATE TABLE authors(id INTEGER PRIMARY KEY, name TEXT, sort TEXT);
        CREATE TABLE books_authors_link(id INTEGER PRIMARY KEY, book INTEGER, author INTEGER);
        CREATE TABLE identifiers(id INTEGER PRIMARY KEY, book INTEGER, type TEXT, val TEXT);
        CREATE TABLE tags(id INTEGER PRIMARY KEY, name TEXT);
        CREATE TABLE books_tags_link(id INTEGER PRIMARY KEY, book INTEGER, tag INTEGER);
        CREATE TABLE languages(id INTEGER PRIMARY KEY, lang_code TEXT);
        CREATE TABLE books_languages_link(id INTEGER PRIMARY KEY, book INTEGER, lang_code INTEGER, item_order INTEGER);
        CREATE TABLE publishers(id INTEGER PRIMARY KEY, name TEXT);
        CREATE TABLE books_publishers_link(id INTEGER PRIMARY KEY, book INTEGER, publisher INTEGER);
        CREATE TABLE comments(id INTEGER PRIMARY KEY, book INTEGER, text TEXT);
        CREATE TABLE data(id INTEGER PRIMARY KEY, book INTEGER, format TEXT, name TEXT);";

    /// Build a minimal Calibre-shaped `metadata.db` with one book.
    fn write_fixture_library(root: &Utf8Path) {
        let db = rusqlite::Connection::open(root.join("metadata.db")).unwrap();
        db.execute_batch(CALIBRE_SCHEMA).unwrap();
        db.execute_batch(
            "INSERT INTO books VALUES (1, 'Dune', 'Dune', 'Herbert/Dune (1)', '1965-08-01T00:00:00+00:00', 1);
             INSERT INTO authors VALUES (1, 'Frank Herbert', 'Herbert, Frank');
             INSERT INTO books_authors_link VALUES (1, 1, 1);
             INSERT INTO identifiers VALUES (1, 1, 'isbn', '9780441172719');
             INSERT INTO identifiers VALUES (2, 1, 'amazon', 'B00B7NPRY8');
             INSERT INTO tags VALUES (1, 'Science Fiction');
             INSERT INTO books_tags_link VALUES (1, 1, 1);
             INSERT INTO languages VALUES (1, 'eng');
             INSERT INTO books_languages_link VALUES (1, 1, 1, 0);
             INSERT INTO publishers VALUES (1, 'Ace');
             INSERT INTO books_publishers_link VALUES (1, 1, 1);
             INSERT INTO comments VALUES (1, 1, 'A desert planet.');
             INSERT INTO data VALUES (1, 1, 'EPUB', 'Dune - Frank Herbert');",
        )
        .unwrap();
    }

    #[tokio::test]
    async fn scans_a_calibre_library() {
        let root = tempfile::tempdir().unwrap();
        write_fixture_library(Utf8Path::new(root.path().to_str().unwrap()));

        let records = scan_calibre(
            host_binary(),
            Utf8PathBuf::from(root.path().to_str().unwrap()),
        )
        .await
        .expect("scan succeeds");

        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.meta.title, "Dune");
        assert_eq!(record.meta.title_sort.as_deref(), Some("Dune"));
        assert_eq!(record.meta.contributors.len(), 1);
        assert_eq!(record.meta.contributors[0].name, "Frank Herbert");
        assert_eq!(record.meta.isbns, vec!["9780441172719".to_string()]);
        assert_eq!(
            record.meta.other_identifiers,
            vec!["amazon:B00B7NPRY8".to_string()]
        );
        assert_eq!(record.meta.subjects, vec!["Science Fiction".to_string()]);
        assert_eq!(record.meta.language.as_deref(), Some("eng"));
        assert_eq!(record.meta.publisher.as_deref(), Some("Ace"));
        assert_eq!(record.meta.description.as_deref(), Some("A desert planet."));
        assert_eq!(record.meta.published.as_ref().map(|p| p.year), Some(1965));
        assert_eq!(record.files.len(), 1);
        assert_eq!(record.files[0].format, "epub");
        assert_eq!(
            record.files[0].path,
            "Herbert/Dune (1)/Dune - Frank Herbert.epub"
        );
        assert_eq!(
            record.cover_path.as_deref(),
            Some("Herbert/Dune (1)/cover.jpg")
        );
    }

    const COVER_BYTES: &[u8] = b"\xFF\xD8\xFF\xE0 dune cover";

    fn utf8(dir: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from(dir.path().to_str().unwrap())
    }

    /// A library exercising every per-file outcome:
    /// - Dune: an EPUB on disk with `cover.jpg`, plus a PDF listed but missing.
    /// - Kindred: an EPUB on disk; `has_cover` is set but no cover file exists.
    /// - A blank-titled book whose EPUB exists but fails catalog validation.
    fn write_import_library(root: &Utf8Path) {
        let db = rusqlite::Connection::open(root.join("metadata.db")).unwrap();
        db.execute_batch(CALIBRE_SCHEMA).unwrap();
        db.execute_batch(
            "INSERT INTO books VALUES (1, 'Dune', 'Dune', 'Herbert/Dune (1)', '1965-08-01T00:00:00+00:00', 1);
             INSERT INTO books VALUES (2, 'Kindred', 'Kindred', 'Butler/Kindred (2)', NULL, 1);
             INSERT INTO books VALUES (3, '   ', '   ', 'Unknown/Blank (3)', NULL, 0);
             INSERT INTO authors VALUES (1, 'Frank Herbert', 'Herbert, Frank');
             INSERT INTO authors VALUES (2, 'Octavia E. Butler', 'Butler, Octavia E.');
             INSERT INTO authors VALUES (3, 'Unknown', 'Unknown');
             INSERT INTO books_authors_link VALUES (1, 1, 1);
             INSERT INTO books_authors_link VALUES (2, 2, 2);
             INSERT INTO books_authors_link VALUES (3, 3, 3);
             INSERT INTO identifiers VALUES (1, 1, 'isbn', '9780441172719');
             INSERT INTO data VALUES (1, 1, 'EPUB', 'Dune');
             INSERT INTO data VALUES (2, 1, 'PDF', 'Dune');
             INSERT INTO data VALUES (3, 2, 'EPUB', 'Kindred');
             INSERT INTO data VALUES (4, 3, 'EPUB', 'Blank');",
        )
        .unwrap();

        let dune = root.join("Herbert/Dune (1)");
        fs_err::create_dir_all(&dune).unwrap();
        fs_err::write(dune.join("Dune.epub"), b"dune epub bytes").unwrap();
        fs_err::write(dune.join("cover.jpg"), COVER_BYTES).unwrap();

        let kindred = root.join("Butler/Kindred (2)");
        fs_err::create_dir_all(&kindred).unwrap();
        fs_err::write(kindred.join("Kindred.epub"), b"kindred epub bytes").unwrap();

        let blank = root.join("Unknown/Blank (3)");
        fs_err::create_dir_all(&blank).unwrap();
        fs_err::write(blank.join("Blank.epub"), b"blank epub bytes").unwrap();
    }

    async fn inventory_for(
        conn: &livtet_core::data::orm::DatabaseConnection,
        title: &str,
    ) -> livtet_core::data::entities::digital_inventory::Model {
        use livtet_core::data::entities::{digital_inventory, editions};
        use livtet_core::data::orm::{ColumnTrait, EntityTrait, QueryFilter};

        let edition = editions::Entity::find()
            .filter(editions::Column::Title.eq(title))
            .one(conn)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("edition {title} imported"));
        digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition.id))
            .one(conn)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("inventory for {title}"))
    }

    #[tokio::test]
    async fn import_tallies_each_file_outcome_and_persists_books() {
        use livtet_core::data::entities::editions;
        use livtet_core::data::orm::EntityTrait;
        use livtet_core::data::{Kind, TestDb};

        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state();
        let library = tempfile::tempdir().unwrap();
        let covers = tempfile::tempdir().unwrap();
        let books = tempfile::tempdir().unwrap();
        let search_dir = tempfile::tempdir().unwrap();
        write_import_library(&utf8(&library));
        let index = SearchIndex::open(&utf8(&search_dir)).expect("search index");
        let search_index: ArcMut<Option<SearchIndex>> =
            std::sync::Arc::new(tokio::sync::RwLock::new(Some(index)));

        let summary = import_calibre_library_inner(
            host_binary(),
            utf8(&library),
            ImportMode::Copy,
            &db,
            &utf8(&covers),
            &utf8(&books),
            &search_index,
        )
        .await
        .expect("import succeeds");

        assert_eq!(summary.books, 3);
        assert_eq!(summary.imported, 2, "Dune EPUB and Kindred EPUB");
        assert_eq!(summary.duplicated, 0);
        assert_eq!(summary.failed, 2, "missing Dune PDF and blank-titled book");

        let conn = db.db_conn();
        let mut titles: Vec<String> = editions::Entity::find()
            .all(&conn)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|edition| edition.title)
            .collect();
        titles.sort();
        assert_eq!(titles, vec!["Dune".to_string(), "Kindred".to_string()]);

        // Each edition owns a copy of its own file, not a sibling's.
        let dune = inventory_for(&conn, "Dune").await;
        let dune_file = dune.file_path.clone().expect("dune library path");
        assert_eq!(fs_err::read(&dune_file).unwrap(), b"dune epub bytes");
        assert!(dune_file.starts_with(utf8(&books).as_str()));
        let kindred = inventory_for(&conn, "Kindred").await;
        let kindred_file = kindred.file_path.clone().expect("kindred library path");
        assert_eq!(fs_err::read(&kindred_file).unwrap(), b"kindred epub bytes");

        // The Calibre cover travels with its book; a missing cover is skipped.
        let dune_cover = dune.cover_path.expect("dune cover stored");
        assert!(dune_cover.starts_with(utf8(&covers).as_str()));
        assert_eq!(fs_err::read(&dune_cover).unwrap(), COVER_BYTES);
        assert_eq!(kindred.cover_path, None);

        // Fresh imports are searchable straight away.
        let guard = search_index.read().await;
        let hits = guard
            .as_ref()
            .unwrap()
            .search("Kindred", 10)
            .await
            .expect("search");
        let kindred_edition = kindred.edition_id.to_string();
        assert!(
            hits.iter()
                .any(|hit| hit.edition_id.as_deref() == Some(kindred_edition.as_str())),
            "imported edition missing from search hits: {hits:?}"
        );
    }

    #[tokio::test]
    async fn reimporting_a_library_counts_duplicates_without_new_editions() {
        use livtet_core::data::entities::editions;
        use livtet_core::data::orm::{EntityTrait, PaginatorTrait};
        use livtet_core::data::{Kind, TestDb};

        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state();
        let library = tempfile::tempdir().unwrap();
        let covers = tempfile::tempdir().unwrap();
        let books = tempfile::tempdir().unwrap();
        write_import_library(&utf8(&library));
        // No search index loaded: importing must still succeed.
        let search_index: ArcMut<Option<SearchIndex>> =
            std::sync::Arc::new(tokio::sync::RwLock::new(None));

        let first = import_calibre_library_inner(
            host_binary(),
            utf8(&library),
            ImportMode::Copy,
            &db,
            &utf8(&covers),
            &utf8(&books),
            &search_index,
        )
        .await
        .expect("import succeeds");
        assert_eq!(first.imported, 2);
        let summary = import_calibre_library_inner(
            host_binary(),
            utf8(&library),
            ImportMode::Copy,
            &db,
            &utf8(&covers),
            &utf8(&books),
            &search_index,
        )
        .await
        .expect("re-import succeeds");

        assert_eq!(summary.books, 3);
        assert_eq!(summary.imported, 0);
        assert_eq!(summary.duplicated, 2);
        assert_eq!(summary.failed, 2);
        let editions = editions::Entity::find().count(&db.db_conn()).await.unwrap();
        assert_eq!(editions, 2);
    }

    #[tokio::test]
    async fn scan_rejects_a_directory_without_metadata_db() {
        let root = tempfile::tempdir().unwrap();

        let error = scan_calibre(host_binary(), utf8(&root))
            .await
            .expect_err("not a Calibre library");

        assert_eq!(error.code, "io");
    }

    #[tokio::test]
    async fn scan_reports_an_importer_error_when_the_host_cannot_launch() {
        let root = tempfile::tempdir().unwrap();
        write_fixture_library(&utf8(&root));
        let missing_host = utf8(&root).join("no-such-plugin-host");

        let error = scan_calibre(missing_host, utf8(&root))
            .await
            .expect_err("host binary is missing");

        assert_eq!(error.code, "importer");
    }

    #[tokio::test]
    async fn scan_lists_every_format_and_omits_absent_covers() {
        let root = tempfile::tempdir().unwrap();
        write_import_library(&utf8(&root));

        let records = scan_calibre(host_binary(), utf8(&root))
            .await
            .expect("scan succeeds");

        assert_eq!(records.len(), 3);
        let dune = &records[0];
        let formats: Vec<(&str, &str)> = dune
            .files
            .iter()
            .map(|file| (file.format.as_str(), file.path.as_str()))
            .collect();
        assert_eq!(
            formats,
            vec![
                ("epub", "Herbert/Dune (1)/Dune.epub"),
                ("pdf", "Herbert/Dune (1)/Dune.pdf"),
            ]
        );
        let kindred_authors: Vec<(&str, Option<&str>)> = records[1]
            .meta
            .contributors
            .iter()
            .map(|c| (c.name.as_str(), c.file_as.as_deref()))
            .collect();
        assert_eq!(
            kindred_authors,
            vec![("Octavia E. Butler", Some("Butler, Octavia E."))]
        );
        assert_eq!(records[1].meta.published, None);
        assert_eq!(records[2].cover_path, None, "has_cover = 0");
    }

    #[test]
    fn mime_for_maps_cover_extensions_case_insensitively() {
        assert_eq!(mime_for("a/cover.png"), "image/png");
        assert_eq!(mime_for("a/COVER.PNG"), "image/png");
        assert_eq!(mime_for("a/cover.webp"), "image/webp");
        assert_eq!(mime_for("a/cover.GIF"), "image/gif");
        assert_eq!(mime_for("a/cover.jpg"), "image/jpeg");
        assert_eq!(mime_for("a/cover.jpeg"), "image/jpeg");
        assert_eq!(mime_for("a/cover"), "image/jpeg");
    }
}
