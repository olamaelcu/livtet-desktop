use livtet_core::search::model::{SearchHit, SearchOptions};
use serde::Serialize;
use specta::Type;
use tauri::State;

use livtet_types::{DbId, SortDirection, WorkSortBy};

use crate::error::SearchIndexError;
use crate::types::AppState;

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
    offset: Option<i32>,
    limit: Option<i32>,
    state: State<'_, AppState>,
) -> Result<SearchResponse, SearchIndexError> {
    let guard = state.search_index.read().await;
    let index = guard
        .as_ref()
        .ok_or(SearchIndexError::unavailable("search_editions"))?;

    let q = query.as_deref().unwrap_or("");
    let offset_usize = offset.unwrap_or(0).max(0) as usize;
    let limit_usize = limit.unwrap_or(20).max(1) as usize;

    let opts = SearchOptions {
        offset: offset_usize as i64,
        ..Default::default()
    };

    let hits = index
        .search_with_options(q, limit_usize, &opts)
        .await
        .map_err(SearchIndexError::other)?;

    let total = index
        .count_works_filtered(q, &livtet_types::WorkFilters::default())
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
    state: State<'_, AppState>,
) -> Result<i32, SearchIndexError> {
    let guard = state.search_index.read().await;
    let index = guard
        .as_ref()
        .ok_or(SearchIndexError::unavailable("search_editions_count"))?;

    let q = query.as_deref().unwrap_or("");

    let count: usize = index
        .count_works_filtered(q, &livtet_types::WorkFilters::default())
        .await
        .map_err(SearchIndexError::other)?;

    Ok(count as i32)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<WorkSortBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_direction: Option<SortDirection>,
}

impl EditionFilters {
    /// Drop the u64 `limit` field and hand the rest to the index layer.
    #[allow(dead_code)]
    fn into_core(self) -> livtet_types::WorkFilters {
        livtet_types::WorkFilters {
            tag_ids: self.tag_ids,
            genre_ids: self.genre_ids,
            subject_ids: self.subject_ids,
            publisher_ids: self.publisher_ids,
            author_ids: self.author_ids,
            format_ids: self.format_ids,
            language_ids: self.language_ids,
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
    }
}
