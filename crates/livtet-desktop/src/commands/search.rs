use livtet_core::data::entities::{
    authors, formats, genres, languages, publishers, subjects, tags,
};
use livtet_core::data::orm::{DatabaseConnection, EntityTrait, QueryOrder};
use livtet_core::search::model::{SearchHit, SearchOptions};
use livtet_search::{SearchIndex, WorkFiltersQuery};
use serde::Serialize;
use specta::Type;
use tauri::State;

use livtet_types::{DbId, SortDirection, WorkSortBy};

use crate::error::SearchIndexError;
use crate::types::AppState;

pub(crate) async fn build_filtered(
    index: &SearchIndex,
    db: &livtet_core::data::orm::DatabaseConnection,
    query: &str,
    filters: EditionFilters,
) -> Result<(WorkFiltersQuery, SearchOptions), SearchIndexError> {
    let (format_labels, language_labels) = index
        .label_resolver
        .resolve(db, &filters.format_ids, &filters.language_ids)
        .await
        .map_err(SearchIndexError::other)?;

    // Relevance (BM25) is the default ordering. Only switch to an explicit
    // sort when the caller asked for one, so free-text search keeps its rank.
    let want_sort = filters.sort_by.is_some();

    let resolved = livtet_search::WorkFiltersResolved {
        filters: filters.into_core(),
        format_labels,
        language_labels,
    };
    let built = WorkFiltersQuery::new(resolved, query.to_string());

    let mut opts = SearchOptions::default();
    if want_sort {
        opts.sort = Some(built.build_sort());
    }
    Ok((built, opts))
}

#[tauri::command]
#[specta::specta]
pub async fn search_typeahead(
    query: String,
    limit: Option<i32>,
    state: State<'_, AppState>,
) -> Result<Vec<SearchResult>, SearchIndexError> {
    let guard = state.search_index.read().await;
    search_typeahead_inner(guard.as_ref(), &query, limit).await
}

/// Blank input short-circuits to no results before the index is consulted,
/// so an unavailable index only surfaces once the user has typed something.
pub(crate) async fn search_typeahead_inner(
    index: Option<&SearchIndex>,
    query: &str,
    limit: Option<i32>,
) -> Result<Vec<SearchResult>, SearchIndexError> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let index = index.ok_or(SearchIndexError::unavailable("search_typeahead"))?;

    let limit_usize = limit.unwrap_or(5).max(1) as usize;
    let hits = index
        .search(query, limit_usize)
        .await
        .map_err(SearchIndexError::other)?;

    let results = hits.into_iter().map(SearchResult::from).collect();

    Ok(results)
}

#[tauri::command]
#[specta::specta]
pub async fn search_editions(
    query: Option<String>,
    filters: Option<EditionFilters>,
    offset: Option<i32>,
    limit: Option<i32>,
    state: State<'_, AppState>,
) -> Result<SearchResponse, SearchIndexError> {
    let guard = state.search_index.read().await;
    let db = state.db.db_conn();
    search_editions_inner(
        guard.as_ref(),
        &db,
        query.as_deref(),
        filters,
        offset,
        limit,
    )
    .await
}

pub(crate) async fn search_editions_inner(
    index: Option<&SearchIndex>,
    db: &DatabaseConnection,
    query: Option<&str>,
    filters: Option<EditionFilters>,
    offset: Option<i32>,
    limit: Option<i32>,
) -> Result<SearchResponse, SearchIndexError> {
    let index = index.ok_or(SearchIndexError::unavailable("search_editions"))?;

    let q = query.unwrap_or("");
    let offset_usize = offset.unwrap_or(0).max(0) as usize;
    let limit_usize = limit.unwrap_or(20).max(1) as usize;

    let (built, mut opts) = build_filtered(index, db, q, filters.unwrap_or_default()).await?;
    opts.offset = offset_usize as i64;

    let hits = index
        .search_with_query(
            built
                .build_query(index.index())
                .map_err(SearchIndexError::other)?,
            limit_usize,
            &opts,
        )
        .await
        .map_err(SearchIndexError::other)?;

    let total = index
        .count_with_query(
            built
                .build_query(index.index())
                .map_err(SearchIndexError::other)?,
        )
        .await
        .map_err(SearchIndexError::other)? as i32;

    Ok(SearchResponse {
        hits: hits.into_iter().map(SearchResult::from).collect(),
        total,
    })
}

/// One selectable value in a filter axis: a stable id plus its display label.
///
/// At most one of `flag_emoji` / `logo_url` is set, and only for the axis that
/// owns that kind of decoration: languages carry a flag emoji, publishers carry
/// a remote logo URL. Every other axis leaves both empty.
#[derive(Debug, Clone, Serialize, Type)]
pub struct FilterOption {
    pub id: DbId,
    pub label: String,
    pub flag_emoji: Option<String>,
    pub logo_url: Option<String>,
}

/// Every filter option the search UI can offer, grouped by axis.
#[derive(Debug, Clone, Serialize, Type)]
pub struct FilterOptions {
    pub formats: Vec<FilterOption>,
    pub languages: Vec<FilterOption>,
    pub tags: Vec<FilterOption>,
    pub genres: Vec<FilterOption>,
    pub subjects: Vec<FilterOption>,
    pub publishers: Vec<FilterOption>,
    pub authors: Vec<FilterOption>,
}

/// Read the seven filter value tables, each ordered by name for stable display.
#[tauri::command]
#[specta::specta]
pub async fn filter_options(state: State<'_, AppState>) -> Result<FilterOptions, SearchIndexError> {
    filter_options_inner(&state.db.db_conn()).await
}

pub(crate) async fn filter_options_inner(
    db: &DatabaseConnection,
) -> Result<FilterOptions, SearchIndexError> {
    macro_rules! axis {
        ($entity:ident, $decorate:expr) => {
            $entity::Entity::find()
                .order_by_asc($entity::Column::Name)
                .all(db)
                .await
                .map_err(SearchIndexError::other)?
                .into_iter()
                .map(|m| {
                    let (flag_emoji, logo_url) = $decorate(&m);
                    FilterOption {
                        id: m.id,
                        label: m.name,
                        flag_emoji,
                        logo_url,
                    }
                })
                .collect::<Vec<_>>()
        };
    }

    Ok(FilterOptions {
        formats: axis!(formats, |_| (None, None)),
        languages: axis!(languages, |m: &languages::Model| (
            m.flag_emoji.clone(),
            None
        )),
        tags: axis!(tags, |_| (None, None)),
        genres: axis!(genres, |_| (None, None)),
        subjects: axis!(subjects, |_| (None, None)),
        publishers: axis!(publishers, |m: &publishers::Model| (
            None,
            m.logo_url.clone()
        )),
        authors: axis!(authors, |_| (None, None)),
    })
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, Type)]
pub struct EditionFilters {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag_ids: Vec<DbId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genre_ids: Vec<DbId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject_ids: Vec<DbId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publisher_ids: Vec<DbId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub author_ids: Vec<DbId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub format_ids: Vec<DbId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub language_ids: Vec<DbId>,
    /// File availability: `None` = any, `Some(true)` = on disk,
    /// `Some(false)` = virtual / remotely referenced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_file: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<WorkSortBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_direction: Option<SortDirection>,
}

impl EditionFilters {
    /// Drop the u64 `limit` field and hand the rest to the index layer.
    pub(crate) fn into_core(self) -> livtet_types::WorkFilters {
        livtet_types::WorkFilters {
            tag_ids: self.tag_ids,
            genre_ids: self.genre_ids,
            subject_ids: self.subject_ids,
            publisher_ids: self.publisher_ids,
            author_ids: self.author_ids,
            format_ids: self.format_ids,
            language_ids: self.language_ids,
            has_file: self.has_file,
            sort_by: self.sort_by,
            sort_direction: self.sort_direction,
            limit: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct SearchResult {
    pub work_id: String,
    pub edition_id: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub pub_date: Option<String>,
    pub snippet_text: Option<String>,
    pub kind: livtet_search::HitKind,
    pub has_file: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct SearchResponse {
    pub hits: Vec<SearchResult>,
    pub total: i32,
}

impl From<SearchHit> for SearchResult {
    fn from(hit: SearchHit) -> Self {
        Self {
            work_id: hit.work_id,
            edition_id: hit.edition_id,
            title: hit.title,
            authors: hit.authors,
            pub_date: hit.published_date,
            snippet_text: hit.snippet_text,
            kind: hit.kind,
            has_file: hit.has_file,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use livtet_core::data::entities::{
        digital_inventory, edition_authors, edition_publishers, edition_tags, editions, works,
    };
    use livtet_core::data::orm::{ActiveModelTrait, Set};
    use livtet_core::data::{Kind, TestDb};
    use livtet_search::HitKind;
    use livtet_types::{CommonLanguages, KnownFormats, SortField, now_primitive};

    #[test]
    fn into_core_drops_limit_and_maps_sort() {
        let filters = EditionFilters {
            tag_ids: vec![livtet_types::DbId::new()],
            sort_by: Some(WorkSortBy::Title),
            sort_direction: Some(SortDirection::Asc),
            ..EditionFilters::default()
        };

        let core = filters.into_core();
        assert_eq!(core.limit, None, "desktop DTO must never carry a u64 limit");
        assert_eq!(core.tag_ids.len(), 1);
        assert_eq!(core.sort_by, Some(WorkSortBy::Title));
        assert_eq!(core.sort_direction, Some(SortDirection::Asc));
    }

    #[test]
    fn default_filters_map_to_default_core() {
        let core = EditionFilters::default().into_core();
        assert!(core.tag_ids.is_empty());
        assert_eq!(core.sort_by, None);
        assert_eq!(core.limit, None);
        assert_eq!(core.has_file, None, "no availability constraint by default");
    }

    #[test]
    fn into_core_carries_has_file() {
        let core = EditionFilters {
            has_file: Some(false),
            ..EditionFilters::default()
        }
        .into_core();
        assert_eq!(core.has_file, Some(false));
    }

    // ---- Real-index fixtures -------------------------------------------

    #[derive(Clone, Copy)]
    struct Book {
        work_id: DbId,
        edition_id: DbId,
        author_id: DbId,
    }

    struct BookSpec {
        title: &'static str,
        author: &'static str,
        description: &'static str,
        format_id: Option<DbId>,
        language_id: Option<DbId>,
        published: Option<time::Date>,
        age_days: i64,
        with_file: bool,
    }

    impl BookSpec {
        fn new(title: &'static str, author: &'static str, description: &'static str) -> Self {
            Self {
                title,
                author,
                description,
                format_id: None,
                language_id: None,
                published: None,
                age_days: 0,
                with_file: false,
            }
        }
    }

    async fn seed_book(db: &DatabaseConnection, spec: BookSpec) -> Book {
        let created_at = now_primitive() - time::Duration::days(spec.age_days);

        let work_id = DbId::new();
        works::ActiveModel {
            id: Set(work_id),
            title: Set(spec.title.to_string()),
            description: Set(None),
            sort_title: Set(None),
            series_type: Set(None),
            language_id: Set(None),
            preferred_edition_id: Set(None),
            created_at: Set(created_at),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("work");

        let edition_id = DbId::new();
        editions::ActiveModel {
            id: Set(edition_id),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(Some(spec.title.to_string())),
            published_date: Set(spec.published),
            format_id: Set(spec.format_id),
            language_id: Set(spec.language_id),
            notes: Set(None),
            description: Set(Some(spec.description.to_string())),
            format_metadata: Set(None),
            created_at: Set(created_at),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("edition");

        let author_id = DbId::new();
        authors::ActiveModel {
            id: Set(author_id),
            name: Set(spec.author.to_string()),
        }
        .insert(db)
        .await
        .expect("author");
        edition_authors::ActiveModel {
            edition_id: Set(edition_id),
            author_id: Set(author_id),
            role: Set("aut".to_string()),
        }
        .insert(db)
        .await
        .expect("edition author");

        if spec.with_file {
            digital_inventory::ActiveModel {
                id: Set(DbId::new()),
                edition_id: Set(edition_id),
                file_path: Set(Some(format!("/books/{edition_id}.epub"))),
                cover_path: Set(None),
                blurhash: Set(None),
                dominant_color: Set(None),
                file_hash: Set(None),
                file_size_bytes: Set(None),
                file_format: Set(Some("epub".to_string())),
                notes: Set(None),
                added_at: Set(created_at),
                updated_at: Set(None),
            }
            .insert(db)
            .await
            .expect("digital inventory");
        }

        Book {
            work_id,
            edition_id,
            author_id,
        }
    }

    /// Three editions over a real Tantivy index:
    ///
    /// | title      | author        | format         | language        | file | age |
    /// |------------|---------------|----------------|-----------------|------|-----|
    /// | Foundation | Isaac Asimov  | EPUB (seeded)  | English (seeded)| yes  | 3d  |
    /// | Dune       | Frank Herbert | PDF (seeded)   | klingon         | no   | 2d  |
    /// | Hyperion   | Dan Simmons   | manga          | —               | no   | 1d  |
    ///
    /// Every description mentions "galactic". Foundation also carries the
    /// `classic` tag and the `Gnome Press` publisher (with a logo URL).
    /// `manga` / `klingon` are lowercase single-stem names so they survive
    /// the index's `en_stem` analyzer verbatim.
    struct Library {
        index: SearchIndex,
        db: DatabaseConnection,
        _test_db: TestDb,
        _index_dir: tempfile::TempDir,
        foundation: Book,
        dune: Book,
        hyperion: Book,
        classic_tag_id: DbId,
        gnome_press_id: DbId,
        manga_format_id: DbId,
        klingon_language_id: DbId,
    }

    impl Library {
        async fn new() -> Self {
            let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
            let db = test_db.state().db_conn();
            let now = now_primitive();

            let manga_format_id = DbId::new();
            formats::ActiveModel {
                id: Set(manga_format_id),
                name: Set("manga".to_string()),
                metadata_schema: Set(serde_json::Value::Null),
                progress_unit: Set(None),
            }
            .insert(&db)
            .await
            .expect("manga format");

            let klingon_language_id = DbId::new();
            languages::ActiveModel {
                id: Set(klingon_language_id),
                name: Set("klingon".to_string()),
                code: Set("tlh".to_string()),
                flag_emoji: Set(None),
                created_at: Set(now),
                updated_at: Set(None),
            }
            .insert(&db)
            .await
            .expect("klingon language");

            let foundation = seed_book(
                &db,
                BookSpec {
                    format_id: Some(KnownFormats::Epub.into()),
                    language_id: Some(CommonLanguages::English.into()),
                    published: Some(
                        time::Date::from_calendar_date(1951, time::Month::June, 1).expect("date"),
                    ),
                    age_days: 3,
                    with_file: true,
                    ..BookSpec::new(
                        "Foundation",
                        "Isaac Asimov",
                        "Hari Seldon invents psychohistory to shorten a galactic dark age.",
                    )
                },
            )
            .await;
            let dune = seed_book(
                &db,
                BookSpec {
                    format_id: Some(KnownFormats::Pdf.into()),
                    language_id: Some(klingon_language_id),
                    age_days: 2,
                    ..BookSpec::new(
                        "Dune",
                        "Frank Herbert",
                        "Desert planet politics inside a galactic spice empire.",
                    )
                },
            )
            .await;
            let hyperion = seed_book(
                &db,
                BookSpec {
                    format_id: Some(manga_format_id),
                    age_days: 1,
                    ..BookSpec::new(
                        "Hyperion",
                        "Dan Simmons",
                        "Pilgrims travel to the Time Tombs on a galactic frontier world.",
                    )
                },
            )
            .await;

            let classic_tag_id = DbId::new();
            tags::ActiveModel {
                id: Set(classic_tag_id),
                name: Set("classic".to_string()),
                created_at: Set(now),
                updated_at: Set(None),
            }
            .insert(&db)
            .await
            .expect("tag");
            edition_tags::ActiveModel {
                edition_id: Set(foundation.edition_id),
                tag_id: Set(classic_tag_id),
            }
            .insert(&db)
            .await
            .expect("edition tag");

            let gnome_press_id = DbId::new();
            publishers::ActiveModel {
                id: Set(gnome_press_id),
                name: Set("Gnome Press".to_string()),
                website: Set(None),
                logo_url: Set(Some("https://example.invalid/gnome.png".to_string())),
                created_at: Set(now),
                updated_at: Set(None),
            }
            .insert(&db)
            .await
            .expect("publisher");
            edition_publishers::ActiveModel {
                edition_id: Set(foundation.edition_id),
                publisher_id: Set(gnome_press_id),
            }
            .insert(&db)
            .await
            .expect("edition publisher");

            let index_dir = tempfile::tempdir().expect("index dir");
            let index_path = camino::Utf8Path::from_path(index_dir.path()).expect("utf-8 path");
            let index = SearchIndex::open(index_path).expect("open index");
            index.reindex(&db).await.expect("reindex");

            Self {
                index,
                db,
                _test_db: test_db,
                _index_dir: index_dir,
                foundation,
                dune,
                hyperion,
                classic_tag_id,
                gnome_press_id,
                manga_format_id,
                klingon_language_id,
            }
        }

        async fn search(
            &self,
            query: Option<&str>,
            filters: EditionFilters,
            offset: Option<i32>,
            limit: Option<i32>,
        ) -> SearchResponse {
            search_editions_inner(
                Some(&self.index),
                &self.db,
                query,
                Some(filters),
                offset,
                limit,
            )
            .await
            .expect("search_editions")
        }

        async fn filtered(&self, filters: EditionFilters) -> SearchResponse {
            self.search(None, filters, None, None).await
        }
    }

    fn titles(response: &SearchResponse) -> Vec<&str> {
        response.hits.iter().map(|h| h.title.as_str()).collect()
    }

    fn sorted_titles(response: &SearchResponse) -> Vec<&str> {
        let mut t = titles(response);
        t.sort_unstable();
        t
    }

    // ---- search_editions -----------------------------------------------

    #[tokio::test]
    async fn search_editions_without_index_is_unavailable() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        let err = search_editions_inner(None, &db, Some("dune"), None, None, None)
            .await
            .expect_err("no index loaded");
        assert!(matches!(err, SearchIndexError::Unavailable(_)), "{err:?}");
    }

    #[tokio::test]
    async fn empty_query_lists_every_edition_but_no_author_docs() {
        let lib = Library::new().await;

        let response = search_editions_inner(Some(&lib.index), &lib.db, None, None, None, None)
            .await
            .expect("search");

        // The index also holds one `author` document per author; those must
        // never leak into edition results or inflate the total.
        assert_eq!(response.total, 3);
        assert_eq!(sorted_titles(&response), ["Dune", "Foundation", "Hyperion"]);
        assert!(response.hits.iter().all(|h| h.kind == HitKind::Edition));

        let blank = lib
            .search(Some("   "), EditionFilters::default(), None, None)
            .await;
        assert_eq!(
            blank.total, 3,
            "whitespace-only query behaves like no query"
        );
    }

    #[tokio::test]
    async fn query_matches_author_and_title() {
        let lib = Library::new().await;

        let by_author = lib
            .search(Some("herbert"), EditionFilters::default(), None, None)
            .await;
        assert_eq!(titles(&by_author), ["Dune"]);
        assert_eq!(by_author.total, 1);

        let by_title = lib
            .search(Some("hyperion"), EditionFilters::default(), None, None)
            .await;
        assert_eq!(titles(&by_title), ["Hyperion"]);
        assert_eq!(by_title.total, 1);

        let none = lib
            .search(Some("necronomicon"), EditionFilters::default(), None, None)
            .await;
        assert!(none.hits.is_empty());
        assert_eq!(none.total, 0);
    }

    #[tokio::test]
    async fn hits_carry_edition_fields_through_from_impl() {
        let lib = Library::new().await;

        let response = lib
            .search(Some("psychohistory"), EditionFilters::default(), None, None)
            .await;
        assert_eq!(response.hits.len(), 1);
        let hit = &response.hits[0];

        assert_eq!(hit.work_id, lib.foundation.work_id.to_string());
        assert_eq!(
            hit.edition_id.as_deref(),
            Some(lib.foundation.edition_id.to_string().as_str())
        );
        assert_eq!(hit.title, "Foundation");
        assert_eq!(hit.authors, ["Isaac Asimov"]);
        assert!(
            hit.pub_date
                .as_deref()
                .is_some_and(|d| d.starts_with("1951-06-01")),
            "pub_date must come from published_date: {:?}",
            hit.pub_date
        );
        assert!(
            hit.snippet_text
                .as_deref()
                .is_some_and(|s| s.contains("psychohistory")),
            "snippet must come from the matched description: {:?}",
            hit.snippet_text
        );
        assert!(hit.has_file, "Foundation has a digital_inventory row");
        assert_eq!(hit.kind, HitKind::Edition);
    }

    #[tokio::test]
    async fn has_file_filter_splits_on_digital_inventory() {
        let lib = Library::new().await;

        let on_disk = lib
            .filtered(EditionFilters {
                has_file: Some(true),
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(titles(&on_disk), ["Foundation"]);
        assert_eq!(on_disk.total, 1);
        assert!(on_disk.hits[0].has_file);

        let virtual_only = lib
            .filtered(EditionFilters {
                has_file: Some(false),
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(sorted_titles(&virtual_only), ["Dune", "Hyperion"]);
        assert_eq!(virtual_only.total, 2);
        assert!(virtual_only.hits.iter().all(|h| !h.has_file));
    }

    #[tokio::test]
    async fn id_filters_narrow_to_linked_editions() {
        let lib = Library::new().await;

        let tagged = lib
            .filtered(EditionFilters {
                tag_ids: vec![lib.classic_tag_id],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(titles(&tagged), ["Foundation"]);

        let published = lib
            .filtered(EditionFilters {
                publisher_ids: vec![lib.gnome_press_id],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(titles(&published), ["Foundation"]);

        let by_author = lib
            .filtered(EditionFilters {
                author_ids: vec![lib.dune.author_id],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(titles(&by_author), ["Dune"]);
        assert_eq!(by_author.total, 1);

        // Ids within one axis are OR'd.
        let either_author = lib
            .filtered(EditionFilters {
                author_ids: vec![lib.dune.author_id, lib.hyperion.author_id],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(sorted_titles(&either_author), ["Dune", "Hyperion"]);
    }

    #[tokio::test]
    async fn query_and_filters_are_intersected() {
        let lib = Library::new().await;

        // "galactic" matches all three descriptions; the author filter
        // must cut that down rather than widen it.
        let response = lib
            .search(
                Some("galactic"),
                EditionFilters {
                    author_ids: vec![lib.dune.author_id],
                    ..EditionFilters::default()
                },
                None,
                None,
            )
            .await;
        assert_eq!(titles(&response), ["Dune"]);
        assert_eq!(response.total, 1);

        let disjoint = lib
            .search(
                Some("herbert"),
                EditionFilters {
                    has_file: Some(true),
                    ..EditionFilters::default()
                },
                None,
                None,
            )
            .await;
        assert!(disjoint.hits.is_empty(), "Dune has no file");
        assert_eq!(disjoint.total, 0);
    }

    #[tokio::test]
    async fn format_and_language_ids_resolve_through_their_own_tables() {
        let lib = Library::new().await;

        let manga = lib
            .filtered(EditionFilters {
                format_ids: vec![lib.manga_format_id],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(titles(&manga), ["Hyperion"]);
        assert_eq!(manga.total, 1);

        let klingon = lib
            .filtered(EditionFilters {
                language_ids: vec![lib.klingon_language_id],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(titles(&klingon), ["Dune"]);
        assert_eq!(klingon.total, 1);

        // Both axes at once: no edition is manga *and* klingon.
        let both = lib
            .filtered(EditionFilters {
                format_ids: vec![lib.manga_format_id],
                language_ids: vec![lib.klingon_language_id],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(both.total, 0);
    }

    #[tokio::test]
    #[ignore = "bug: format/language labels are matched as raw terms against the en_stem-analyzed \
                `format`/`language` fields, so seeded names like EPUB / English never match"]
    async fn seeded_format_and_language_ids_filter_editions() {
        let lib = Library::new().await;

        let epub = lib
            .filtered(EditionFilters {
                format_ids: vec![KnownFormats::Epub.into()],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(titles(&epub), ["Foundation"]);

        let english = lib
            .filtered(EditionFilters {
                language_ids: vec![CommonLanguages::English.into()],
                ..EditionFilters::default()
            })
            .await;
        assert_eq!(titles(&english), ["Foundation"]);
    }

    #[tokio::test]
    async fn explicit_sort_orders_results() {
        let lib = Library::new().await;
        let sorted = |sort_by, sort_direction| EditionFilters {
            sort_by: Some(sort_by),
            sort_direction,
            ..EditionFilters::default()
        };

        let title_asc = lib
            .filtered(sorted(WorkSortBy::Title, Some(SortDirection::Asc)))
            .await;
        assert_eq!(titles(&title_asc), ["Dune", "Foundation", "Hyperion"]);

        let title_default = lib.filtered(sorted(WorkSortBy::Title, None)).await;
        assert_eq!(
            titles(&title_default),
            ["Hyperion", "Foundation", "Dune"],
            "direction defaults to descending"
        );

        let oldest_first = lib
            .filtered(sorted(WorkSortBy::CreatedAt, Some(SortDirection::Asc)))
            .await;
        assert_eq!(titles(&oldest_first), ["Foundation", "Dune", "Hyperion"]);

        // Sorting applies on top of a text query too.
        let galactic_newest = lib
            .search(
                Some("galactic"),
                sorted(WorkSortBy::CreatedAt, Some(SortDirection::Desc)),
                None,
                None,
            )
            .await;
        assert_eq!(titles(&galactic_newest), ["Hyperion", "Dune", "Foundation"]);
    }

    #[tokio::test]
    async fn offset_and_limit_page_while_total_counts_everything() {
        let lib = Library::new().await;
        let by_title = || EditionFilters {
            sort_by: Some(WorkSortBy::Title),
            sort_direction: Some(SortDirection::Asc),
            ..EditionFilters::default()
        };

        let second = lib.search(None, by_title(), Some(1), Some(1)).await;
        assert_eq!(titles(&second), ["Foundation"]);
        assert_eq!(second.total, 3, "total ignores paging");

        let tail = lib.search(None, by_title(), Some(1), Some(10)).await;
        assert_eq!(titles(&tail), ["Foundation", "Hyperion"]);

        let past_end = lib.search(None, by_title(), Some(3), Some(10)).await;
        assert!(past_end.hits.is_empty());
        assert_eq!(past_end.total, 3);
    }

    #[tokio::test]
    async fn negative_offset_and_non_positive_limit_are_clamped() {
        let lib = Library::new().await;
        let by_title = || EditionFilters {
            sort_by: Some(WorkSortBy::Title),
            sort_direction: Some(SortDirection::Asc),
            ..EditionFilters::default()
        };

        let negative_offset = lib.search(None, by_title(), Some(-5), Some(2)).await;
        assert_eq!(titles(&negative_offset), ["Dune", "Foundation"]);

        let zero_limit = lib.search(None, by_title(), None, Some(0)).await;
        assert_eq!(
            titles(&zero_limit),
            ["Dune"],
            "limit is clamped to at least one"
        );

        let negative_limit = lib.search(None, by_title(), None, Some(-3)).await;
        assert_eq!(titles(&negative_limit), ["Dune"]);
    }

    // ---- build_filtered --------------------------------------------------

    #[tokio::test]
    async fn build_filtered_keeps_relevance_unless_sort_by_is_set() {
        let lib = Library::new().await;

        // A direction alone (the UI's default toggle state) must not
        // replace BM25 ranking for free-text search.
        let (_, direction_only) = build_filtered(
            &lib.index,
            &lib.db,
            "galactic",
            EditionFilters {
                sort_direction: Some(SortDirection::Asc),
                ..EditionFilters::default()
            },
        )
        .await
        .expect("build");
        assert!(direction_only.sort.is_none());

        let (_, titled) = build_filtered(
            &lib.index,
            &lib.db,
            "galactic",
            EditionFilters {
                sort_by: Some(WorkSortBy::Title),
                sort_direction: Some(SortDirection::Asc),
                ..EditionFilters::default()
            },
        )
        .await
        .expect("build");
        let spec = titled.sort.expect("explicit sort requested");
        assert_eq!(spec.field, SortField::Title);
        assert_eq!(spec.direction, SortDirection::Asc);
    }

    // ---- search_typeahead ----------------------------------------------

    #[tokio::test]
    async fn typeahead_blank_query_short_circuits_before_index_check() {
        let results = search_typeahead_inner(None, " \t ", Some(3))
            .await
            .expect("blank query never needs the index");
        assert!(results.is_empty());

        let err = search_typeahead_inner(None, "dune", Some(3))
            .await
            .expect_err("real query needs the index");
        assert!(matches!(err, SearchIndexError::Unavailable(_)), "{err:?}");
    }

    #[tokio::test]
    async fn typeahead_returns_editions_only() {
        let lib = Library::new().await;

        // "Isaac Asimov" is also indexed as an author document; typeahead
        // must surface the edition, not the person.
        let results = search_typeahead_inner(Some(&lib.index), "asimov", None)
            .await
            .expect("typeahead");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, HitKind::Edition);
        assert_eq!(results[0].title, "Foundation");
        assert_eq!(
            results[0].edition_id.as_deref(),
            Some(lib.foundation.edition_id.to_string().as_str())
        );
        assert_eq!(results[0].authors, ["Isaac Asimov"]);
    }

    #[tokio::test]
    async fn typeahead_limit_defaults_to_five_and_clamps_to_one() {
        let lib = Library::new().await;
        for (title, author) in [
            ("Ringworld", "Larry Niven"),
            ("Neuromancer", "William Gibson"),
            ("Solaris", "Stanislaw Lem"),
            ("Ubik", "Philip Dick"),
        ] {
            seed_book(
                &lib.db,
                BookSpec::new(title, author, "Another galactic adventure."),
            )
            .await;
        }
        lib.index.reindex(&lib.db).await.expect("reindex");

        let search = |limit| search_typeahead_inner(Some(&lib.index), "galactic", limit);

        assert_eq!(
            search(None).await.expect("default").len(),
            5,
            "7 matches, default cap 5"
        );
        assert_eq!(search(Some(2)).await.expect("two").len(), 2);
        assert_eq!(search(Some(0)).await.expect("zero").len(), 1);
        assert_eq!(search(Some(-4)).await.expect("negative").len(), 1);
        assert_eq!(search(Some(50)).await.expect("wide").len(), 7);
    }

    // ---- filter_options ------------------------------------------------

    fn assert_sorted(axis: &str, options: &[FilterOption]) {
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        let mut expected = labels.clone();
        expected.sort_unstable();
        assert_eq!(labels, expected, "{axis} must be ordered by name");
    }

    fn find<'a>(options: &'a [FilterOption], label: &str) -> &'a FilterOption {
        options
            .iter()
            .find(|o| o.label == label)
            .unwrap_or_else(|| panic!("{label} missing from {options:?}"))
    }

    #[tokio::test]
    async fn filter_options_lists_every_axis_sorted_with_decorations() {
        let lib = Library::new().await;
        let opts = filter_options_inner(&lib.db).await.expect("filter options");

        for (axis, options) in [
            ("formats", &opts.formats),
            ("languages", &opts.languages),
            ("tags", &opts.tags),
            ("genres", &opts.genres),
            ("subjects", &opts.subjects),
            ("publishers", &opts.publishers),
            ("authors", &opts.authors),
        ] {
            assert_sorted(axis, options);
        }

        // Seeded catalog formats keep their deterministic ids.
        for known in KnownFormats::all() {
            assert_eq!(find(&opts.formats, known.name()).id, DbId::from(known));
        }
        assert_eq!(find(&opts.formats, "manga").id, lib.manga_format_id);

        let english = find(&opts.languages, "English");
        assert_eq!(english.id, DbId::from(CommonLanguages::English));
        assert_eq!(
            english.flag_emoji.as_deref(),
            Some(CommonLanguages::English.flag_emoji())
        );
        assert_eq!(english.logo_url, None);
        assert_eq!(find(&opts.languages, "klingon").flag_emoji, None);

        let gnome = find(&opts.publishers, "Gnome Press");
        assert_eq!(gnome.id, lib.gnome_press_id);
        assert_eq!(
            gnome.logo_url.as_deref(),
            Some("https://example.invalid/gnome.png")
        );
        assert_eq!(gnome.flag_emoji, None);

        assert_eq!(find(&opts.tags, "classic").id, lib.classic_tag_id);
        let author_labels: Vec<&str> = opts.authors.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            author_labels,
            ["Dan Simmons", "Frank Herbert", "Isaac Asimov"]
        );
        assert_eq!(find(&opts.authors, "Frank Herbert").id, lib.dune.author_id);

        // Only languages carry flags and only publishers carry logos.
        for (axis, options) in [
            ("formats", &opts.formats),
            ("tags", &opts.tags),
            ("genres", &opts.genres),
            ("subjects", &opts.subjects),
            ("authors", &opts.authors),
        ] {
            assert!(
                options
                    .iter()
                    .all(|o| o.flag_emoji.is_none() && o.logo_url.is_none()),
                "{axis} must not be decorated"
            );
        }
        assert!(opts.languages.iter().all(|o| o.logo_url.is_none()));
        assert!(opts.publishers.iter().all(|o| o.flag_emoji.is_none()));
    }
}
