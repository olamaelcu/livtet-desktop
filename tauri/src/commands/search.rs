//! Edition-level search command.
//!
//! Wraps `livtet_core::search::SearchIndex::search` (re-exported
//! from `livtet-search`). Returns `Vec<SearchHitRow>` with extra
//! cover metadata and remote-search fields added.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

async fn enrich_with_cover_metadata(
    db: &livtet_core::data::orm::DatabaseConnection,
    rows: &mut [SearchHitRow],
) -> Result<(), livtet_core::data::orm::DbErr> {
    use std::collections::HashMap;
    use std::str::FromStr;

    use livtet_core::DbId;
    use livtet_core::data::entities::digital_inventory;
    use livtet_core::data::orm::{ColumnTrait, EntityTrait, QueryFilter};

    let ids: Vec<DbId> = rows
        .iter()
        .filter(|r| r.source == "local")
        .filter_map(|r| r.edition_id.as_deref())
        .filter_map(|s| DbId::from_str(s).ok())
        .collect();

    if ids.is_empty() {
        return Ok(());
    }

    let inventory_rows = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.is_in(ids.clone()))
        .all(db)
        .await?;

    let cover_map: HashMap<String, (Option<String>, Option<String>)> = inventory_rows
        .iter()
        .map(|r| {
            (
                r.edition_id.to_string(),
                (r.blurhash.clone(), r.dominant_color.clone()),
            )
        })
        .collect();

    for row in rows.iter_mut() {
        if let Some(ref edition_id) = row.edition_id
            && let Some((blurhash, dominant_color)) = cover_map.get(edition_id.as_str())
        {
            row.blurhash = blurhash.clone();
            row.dominant_color = dominant_color.clone();
        }
    }

    Ok(())
}

/// Check which remote search hits already exist in the local
/// catalog via ISBN-13 matching, and annotate them with the
/// existing edition ID.
pub async fn enrich_catalog_status(
    db: &livtet_core::data::orm::DatabaseConnection,
    rows: &mut [SearchHitRow],
) {
    use livtet_core::Isbn;
    use livtet_core::data::entities::{edition_identifiers, editions, identifiers};
    use livtet_core::data::orm::{
        ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
    };

    for row in rows.iter_mut() {
        let Some(ref isbn_13) = row.isbn_13 else {
            continue;
        };
        let Ok(isbn) = Isbn::parse(isbn_13) else {
            continue;
        };
        let urn = format!("urn:isbn:{}", isbn.as_str());

        let identifier = match identifiers::Entity::find()
            .filter(identifiers::Column::Value.eq(&urn))
            .one(db)
            .await
        {
            Ok(Some(id)) => id,
            _ => continue,
        };

        let edition = match editions::Entity::find()
            .join(
                JoinType::InnerJoin,
                edition_identifiers::Relation::Edition.def().rev(),
            )
            .filter(edition_identifiers::Column::IdentifierId.eq(identifier.id))
            .order_by_asc(editions::Column::Id)
            .one(db)
            .await
        {
            Ok(Some(e)) => e,
            _ => continue,
        };

        row.in_catalog = true;
        row.in_catalog_edition_id = Some(edition.id.to_string());
    }
}

/// Mirror of `livtet_core::search::SearchHit` with extra fields
/// for remote search results and cover metadata.
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
    /// `[start, end]` byte ranges into `snippet_text`.
    pub snippet_highlighted: Vec<[u32; 2]>,
    pub grouped_edition_ids: Vec<String>,
    pub source: String,
    pub publisher: Option<String>,
    pub page_count: Option<u32>,
    pub cover_url: Option<String>,
    pub description: Option<String>,
    pub isbn_13: Option<String>,
    pub blurhash: Option<String>,
    pub dominant_color: Option<String>,
    /// Whether this edition has a row in `digital_inventory`
    /// (i.e. there is a file on disk).
    pub has_file: bool,
    /// Whether this hit already exists in the local catalog.
    pub in_catalog: bool,
    /// The edition ID of the matching catalog entry, when in_catalog is true.
    pub in_catalog_edition_id: Option<String>,
}

impl From<livtet_core::search::SearchHit> for SearchHitRow {
    fn from(h: livtet_core::search::SearchHit) -> Self {
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
            snippet_highlighted: h.snippet_highlighted,
            grouped_edition_ids: h.grouped_edition_ids,
            source: h.source,
            publisher: None,
            page_count: None,
            cover_url: None,
            description: None,
            isbn_13: None,
            blurhash: None,
            dominant_color: None,
            has_file: h.has_file,
            in_catalog: false,
            in_catalog_edition_id: None,
        }
    }
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err, fields(query, limit))]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> Result<Vec<SearchHitRow>, String> {
    let search = state.search.read().await;
    let hits = search
        .search(&query, limit as usize)
        .await
        .map_err(|e| e.to_string())?;

    let mut rows: Vec<SearchHitRow> = hits.into_iter().map(SearchHitRow::from).collect();

    enrich_with_cover_metadata(&state.db.db_conn(), &mut rows)
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows)
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
