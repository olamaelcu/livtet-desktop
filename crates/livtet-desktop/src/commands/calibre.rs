//! Import a Calibre library.
//!
//! Reads a Calibre library's `metadata.db` through the bundled `calibre` Lua
//! plugin (which uses the host `sqlite` module) and persists each book into the
//! catalog via the shared [`super::import::persist_import`] path. The plugin is
//! embedded in the binary and materialized to a temp directory per import, so it
//! needs no separate installation.

use std::path::PathBuf;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use specta::Type;
use tauri::State;

use livtet_importer::{BookRecord, ImporterCover};
use livtet_types::DbId;

use super::import::{ImportError, ImportMode, LibraryTarget, persist_import};
use crate::types::AppState;

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
    let library_root = PathBuf::from(&library_path);
    let host = state.plugin_host_path.clone().into_std_path_buf();

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
                &state.db,
                &state.covers_dir,
                LibraryTarget {
                    dir: &state.books_dir,
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
                    index_edition(&state, &outcome.edition_id).await;
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
async fn index_edition(state: &AppState, edition_id: &str) {
    let Ok(edition_id) = edition_id.parse::<DbId>() else {
        return;
    };
    let db = state.db.db_conn();
    let guard = state.search_index.read().await;
    if let Some(index) = guard.as_ref() {
        let _ = index.add_edition(&db, edition_id).await;
    }
}

/// Materialize the bundled Calibre plugin, launch the host with a read-only
/// grant on the library's `metadata.db`, and call `scan`.
async fn scan_calibre(
    host: PathBuf,
    library_root: PathBuf,
) -> Result<Vec<BookRecord>, ImportError> {
    let db_path = library_root.join("metadata.db");
    if !db_path.is_file() {
        return Err(ImportError::new(
            "io",
            format!("no metadata.db under {}", library_root.display()),
        ));
    }
    let source = library_root.to_string_lossy().into_owned();

    tokio::task::spawn_blocking(move || -> Result<Vec<BookRecord>, ImportError> {
        let temp = tempfile::tempdir().map_err(|err| ImportError::new("io", err))?;
        let plugin_dir = temp.path().join("plugins").join("calibre");
        std::fs::create_dir_all(&plugin_dir).map_err(|err| ImportError::new("io", err))?;
        std::fs::write(plugin_dir.join("plugin.toml"), CALIBRE_PLUGIN_TOML)
            .map_err(|err| ImportError::new("io", err))?;
        std::fs::write(plugin_dir.join("init.lua"), CALIBRE_INIT_LUA)
            .map_err(|err| ImportError::new("io", err))?;
        let config = temp.path().join("host.toml");
        std::fs::write(&config, CALIBRE_HOST_CONFIG).map_err(|err| ImportError::new("io", err))?;

        let options = stanchion::remote::RemoteOptions::new(host)
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
    use super::*;

    fn host_binary() -> PathBuf {
        let test_binary = std::env::current_exe().unwrap();
        let profile_dir = test_binary.parent().and_then(|path| path.parent()).unwrap();
        profile_dir.join(if cfg!(windows) {
            "livtet-plugin-host.exe"
        } else {
            "livtet-plugin-host"
        })
    }

    /// Build a minimal Calibre-shaped `metadata.db` with one book.
    fn write_fixture_library(root: &std::path::Path) {
        let db = rusqlite::Connection::open(root.join("metadata.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE books(id INTEGER PRIMARY KEY, title TEXT, sort TEXT, path TEXT, pubdate TEXT, has_cover INTEGER);
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
             CREATE TABLE data(id INTEGER PRIMARY KEY, book INTEGER, format TEXT, name TEXT);

             INSERT INTO books VALUES (1, 'Dune', 'Dune', 'Herbert/Dune (1)', '1965-08-01T00:00:00+00:00', 1);
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
        write_fixture_library(root.path());

        let records = scan_calibre(host_binary(), root.path().to_path_buf())
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
}
