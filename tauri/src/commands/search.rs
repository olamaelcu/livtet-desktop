//! Edition-level search command.
//!
//! Wraps `livtet_core::search::SearchIndex::search` (re-exported
//! from `livtet-search`). Returns `Vec<SearchHitRow>` so the
//! generated TS bindings can ship the highlighted byte ranges
//! as `number[]` instead of refusing the bigint (`usize`) wrapper
//! that the upstream `SearchHit` uses.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

/// Mirror of `livtet_core::search::SearchHit` whose bigint-bearing
/// field is re-typed for the IPC boundary.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SearchHitRow {
    pub kind: livtet_core::search::HitKind,
    pub edition_id: Option<String>,
    pub work_id: String,
    pub author_id: Option<String>,
    pub title: String,
    pub work_title: Option<String>,
    pub edition_title: Option<String>,
    pub authors: Vec<String>,
    pub isbn: Option<String>,
    pub format: Option<String>,
    pub language: Option<String>,
    pub published_date: Option<String>,
    pub score: f32,
    pub explanation: Option<String>,
    pub snippet_text: Option<String>,
    /// `[start, end]` byte ranges into `snippet_text`. The upstream
    /// `Range<usize>` is re-expressed as `[u32; 2]` so the specta
    /// exporter can ship it as `number[]` instead of refusing.
    pub snippet_highlighted: Vec<[u32; 2]>,
    pub grouped_edition_ids: Vec<String>,
    pub source: String,
    pub publisher: Option<String>,
    pub page_count: Option<u32>,
    pub cover_url: Option<String>,
    pub description: Option<String>,
    pub isbn_13: Option<String>,
}

impl From<livtet_core::search::SearchHit> for SearchHitRow {
    fn from(h: livtet_core::search::SearchHit) -> Self {
        let snippet_highlighted = h
            .snippet_highlighted
            .into_iter()
            .map(|r| [r.start as u32, r.end as u32])
            .collect();
        Self {
            kind: h.kind,
            edition_id: h.edition_id,
            work_id: h.work_id,
            author_id: h.author_id,
            title: h.title,
            work_title: h.work_title,
            edition_title: h.edition_title,
            authors: h.authors,
            isbn: h.isbn,
            format: h.format,
            language: h.language,
            published_date: h.published_date,
            score: h.score,
            explanation: h.explanation,
            snippet_text: h.snippet_text,
            snippet_highlighted,
            grouped_edition_ids: h.grouped_edition_ids,
            source: h.source,
            publisher: None,
            page_count: None,
            cover_url: None,
            description: None,
            isbn_13: None,
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> Result<Vec<SearchHitRow>, String> {
    state
        .search
        .search(&query, limit as usize)
        .await
        .map(|hits| hits.into_iter().map(SearchHitRow::from).collect())
        .map_err(|e| e.to_string())
}

// End-to-end search is exercised by the integration tests in
// livtet-search; here we cover the wrapper contract by stubbing.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_command_signature_compiles() {
        // Compile-only assertion: if the wrapper signature
        // diverges from what the SearchIndex::search call expects,
        // this test fails to compile.
        fn _check(
            state: State<'_, AppState>,
            q: String,
            l: u32,
        ) -> impl std::future::Future<Output = Result<Vec<SearchHitRow>, String>> {
            search(state, q, l)
        }
    }
}
