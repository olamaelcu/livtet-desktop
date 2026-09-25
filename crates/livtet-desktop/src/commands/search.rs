use livtet_core::data::entities::{
    authors, formats, genres, languages, publishers, subjects, tags,
};
use livtet_core::data::orm::{EntityTrait, QueryOrder};
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
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let guard = state.search_index.read().await;
    let index = guard
        .as_ref()
        .ok_or(SearchIndexError::unavailable("search_typeahead"))?;

    let limit_usize = limit.unwrap_or(5).max(1) as usize;
    let hits = index
        .search(&query, limit_usize)
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
    let index = guard
        .as_ref()
        .ok_or(SearchIndexError::unavailable("search_editions"))?;
    let db = state.db.db_conn();

    let q = query.as_deref().unwrap_or("");
    let offset_usize = offset.unwrap_or(0).max(0) as usize;
    let limit_usize = limit.unwrap_or(20).max(1) as usize;

    let (built, mut opts) = build_filtered(index, &db, q, filters.unwrap_or_default()).await?;
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

#[tauri::command]
#[specta::specta]
pub async fn search_editions_count(
    query: Option<String>,
    filters: Option<EditionFilters>,
    state: State<'_, AppState>,
) -> Result<i32, SearchIndexError> {
    let guard = state.search_index.read().await;
    let index = guard
        .as_ref()
        .ok_or(SearchIndexError::unavailable("search_editions_count"))?;
    let db = state.db.db_conn();

    let q = query.as_deref().unwrap_or("");
    let (built, _opts) = build_filtered(index, &db, q, filters.unwrap_or_default()).await?;

    let count = index
        .count_with_query(
            built
                .build_query(index.index())
                .map_err(SearchIndexError::other)?,
        )
        .await
        .map_err(SearchIndexError::other)?;
    Ok(count as i32)
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
    let db = state.db.db_conn();

    macro_rules! axis {
        ($entity:ident, $decorate:expr) => {
            $entity::Entity::find()
                .order_by_asc($entity::Column::Name)
                .all(&db)
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
    use super::EditionFilters;
    use livtet_types::{SortDirection, WorkSortBy};

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
}
