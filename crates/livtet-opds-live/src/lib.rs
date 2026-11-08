//! Concrete [`OpdsRepository`] implementation backed by SeaORM and `livtet-core`.
//!
//! This adapter translates the abstract `OpdsRepository` trait — the interface
//! used by handlers, services, and the OPDS feed mapper — into a Tantivy-driven
//! search path for finding works, with SeaORM only for metadata hydration
//! (batch-loading related tables for the matching work IDs).
//!
//! Batch-loading strategy
//! ----------------------
//! For a list of `N` works we issue one query per related table, scoped by the
//! pre-collected IDs, and stitch the results back together in memory. This
//! avoids the N+1 trap while keeping the code straightforward. The query set
//! is roughly:
//!
//! 1. Works
//! 2. Editions for the work IDs
//! 3. EditionsInventory for the edition IDs (carries cover/file paths)
//! 4. Formats referenced by inventory rows
//! 5. work_authors + authors + edition_authors (overlapping author set)
//! 6. work_tags + tags
//! 7. work_genres + genres
//! 8. work_subjects + subjects
//! 9. work_publishers + publishers
//! 10. edition_publishers + publishers (per-edition publisher)
//! 11. edition_identifiers + identifiers
//! 12. series_entries + series (joined through editions)
//!
//! The search path (finding which work IDs match the query) goes through
//! Tantivy via [`livtet_search::SearchIndex`] — the SQLite database is only
//! used for indexing (looking up full metadata for pre-discovered work IDs).

use std::{collections::HashMap, str::FromStr, sync::Arc};

use async_trait::async_trait;
use livtet_core::{
    SharedState,
    data::entities::{
        authors, authors::Entity as Authors, digital_inventory,
        digital_inventory::Entity as EditionsInventory, edition_authors,
        edition_authors::Entity as EditionAuthors, edition_identifiers,
        edition_identifiers::Entity as EditionIdentifiers, edition_publishers,
        edition_publishers::Entity as EditionPublishers, editions, editions::Entity as Editions,
        formats, formats::Entity as Formats, genres, genres::Entity as Genres, identifiers,
        identifiers::Entity as Identifiers, languages, languages::Entity as Languages, publishers,
        publishers::Entity as Publishers, series, series::Entity as Series, series_entries,
        series_entries::Entity as SeriesEntries, subjects, subjects::Entity as Subjects, tags,
        tags::Entity as Tags, work_authors, work_genres, work_publishers, work_subjects, work_tags,
        works, works::Entity as Works,
    },
};
use livtet_opds_types::OpdsError;
use livtet_search::{SearchIndex, SearchOptions, WorkFiltersQuery, WorkFiltersResolved};
use livtet_types::{
    DbId, SortDirection as CoreSortDirection, WorkFilters as CoreWorkFilters,
    WorkSortBy as CoreWorkSortBy,
};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};

use livtet_opds_server::{
    AuthorInfo, EditionAggregate, FacetAxis, FacetCount, FacetCounts, OpdsRepository, SeriesInfo,
    WorkAggregate, WorkQuery,
};

/// Live `OpdsRepository` implementation that reads from a SeaORM
/// `DatabaseConnection` produced by `livtet-core`, and counts /
/// collapses work IDs through a shared Tantivy [`SearchIndex`].
#[derive(Clone)]
pub struct LivtetOpdsRepository {
    db: DatabaseConnection,
    search_index: Arc<SearchIndex>,
}

impl LivtetOpdsRepository {
    /// Construct a repository from an existing SeaORM connection and a
    /// shared Tantivy [`SearchIndex`].
    pub fn new(db: DatabaseConnection, search_index: Arc<SearchIndex>) -> Self {
        Self { db, search_index }
    }

    /// Construct a repository from a [`SharedState`] and a shared
    /// Tantivy [`SearchIndex`]. This is the common path when wiring
    /// the OPDS server into the same process as the Tauri backend:
    /// `LivtetOpdsRepository::from_state(&state, search_index)`.
    pub fn from_state(state: &SharedState, search_index: Arc<SearchIndex>) -> Self {
        Self {
            db: state.db_conn(),
            search_index,
        }
    }

    /// Borrow the underlying connection. Exposed for tests and callers that
    /// need to issue auxiliary queries outside the trait surface.
    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    /// Borrow the shared Tantivy search index. Exposed for tests that
    /// want to trigger a reindex between seeding and assertions.
    pub fn search_index(&self) -> &SearchIndex {
        &self.search_index
    }
}

#[async_trait]
impl OpdsRepository for LivtetOpdsRepository {
    async fn list_works(&self, query: &WorkQuery) -> Result<Vec<WorkAggregate>, OpdsError> {
        let filters = work_query_to_filters(query)?;
        let limit = std::cmp::max(query.limit.unwrap_or(50), 1) as usize;
        let page = std::cmp::max(query.page.unwrap_or(1), 1) as usize;
        let offset = (page.saturating_sub(1)) * limit;

        let has_search = query.search_query.as_deref().is_some_and(|s| !s.is_empty());

        let work_models: Vec<works::Model> = if has_search {
            // Human-driven text search via Tantivy. The index holds
            // edition-level documents with full-text fields (title,
            // authors, description, identifiers, format, language).
            // We resolve format/language labels so the Tantivy
            // index can filter on those text fields.
            let search_text = query.search_query.clone().unwrap_or_default();
            let format_labels = resolve_format_labels(&self.db, &filters).await?;
            let language_labels = resolve_language_labels(&self.db, &filters).await?;
            let resolved =
                WorkFiltersResolved::from_filters(filters, format_labels, language_labels);
            let filter_query = WorkFiltersQuery::new(resolved, search_text);
            let prebuilt = filter_query
                .build_query(self.search_index.index())
                .map_err(|e| OpdsError::Database(e.to_string()))?;
            let opts = SearchOptions {
                collapse_to_works: true,
                sort: Some(filter_query.build_sort()),
                offset: offset as i64,
                ..SearchOptions::default()
            };

            let hits = self
                .search_index
                .search_with_query(prebuilt, limit, &opts)
                .await
                .map_err(|e| OpdsError::Database(e.to_string()))?;

            // Extract work IDs from Tantivy hits and hydrate from SQLite.
            let tantivy_ids: Vec<DbId> = hits
                .iter()
                .filter_map(|h| h.work_id.parse::<DbId>().ok())
                .collect();

            if tantivy_ids.is_empty() {
                return Ok(Vec::new());
            }

            let mut models: Vec<works::Model> = Works::find()
                .filter(works::Column::Id.is_in(tantivy_ids.clone()))
                .all(&self.db)
                .await
                .map_err(db_err)?;

            // Re-order to match Tantivy's ranking.
            let order: HashMap<DbId, usize> = tantivy_ids
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, i))
                .collect();
            models.sort_by_key(|w| order.get(&w.id).copied().unwrap_or(usize::MAX));
            models
        } else {
            // No free-text search — use SQL for filter-based browsing.
            // The junction tables (work_tags, work_authors, …) are the
            // canonical source for work-level filter matching.
            let sort_by = filters.sort_by.unwrap_or_default();
            let sort_direction = filters.sort_direction.unwrap_or_default();
            let mut q: sea_orm::Select<Works> = Works::find();
            q = apply_work_filter_id_subqueries(q, &filters, &self.db).await?;
            q = match (sort_by, sort_direction) {
                (CoreWorkSortBy::CreatedAt, CoreSortDirection::Desc) => {
                    q.order_by_desc(works::Column::CreatedAt)
                }
                (CoreWorkSortBy::CreatedAt, CoreSortDirection::Asc) => {
                    q.order_by_asc(works::Column::CreatedAt)
                }
                (CoreWorkSortBy::Title, CoreSortDirection::Desc) => {
                    q.order_by_desc(works::Column::Title)
                }
                (CoreWorkSortBy::Title, CoreSortDirection::Asc) => {
                    q.order_by_asc(works::Column::Title)
                }
                (CoreWorkSortBy::UpdatedAt, CoreSortDirection::Desc) => {
                    q.order_by_desc(works::Column::UpdatedAt)
                }
                (CoreWorkSortBy::UpdatedAt, CoreSortDirection::Asc) => {
                    q.order_by_asc(works::Column::UpdatedAt)
                }
                (CoreWorkSortBy::NewestCap, _) => q.order_by_desc(works::Column::CreatedAt),
            };
            q.limit(limit as u64)
                .offset(offset as u64)
                .all(&self.db)
                .await
                .map_err(db_err)?
        };

        assemble_works(&self.db, work_models).await
    }

    async fn get_work(&self, id: &str) -> Result<Option<WorkAggregate>, OpdsError> {
        let Ok(parsed) = DbId::from_str(id) else {
            // A garbage id from a remote client should not surface as 500.
            return Ok(None);
        };

        let Some(work) = Works::find_by_id(parsed)
            .one(&self.db)
            .await
            .map_err(db_err)?
        else {
            return Ok(None);
        };

        let mut aggregates = assemble_works(&self.db, vec![work]).await?;
        Ok(aggregates.pop())
    }

    async fn count_works(&self, query: &WorkQuery) -> Result<u64, OpdsError> {
        let filters = work_query_to_filters(query)?;
        use sea_orm::PaginatorTrait;
        let mut db_query = Works::find();
        db_query = apply_work_filter_id_subqueries(db_query, &filters, &self.db).await?;
        if let Some(search) = query.search_query.as_deref() {
            db_query = db_query.filter(works::Column::Title.like(format!("%{search}%")));
        }
        let count = db_query.count(&self.db).await.map_err(db_err)?;
        Ok(count)
    }

    async fn list_authors(&self) -> Result<Vec<AuthorInfo>, OpdsError> {
        let rows = Authors::find()
            .order_by_asc(authors::Column::Name)
            .all(&self.db)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|a| AuthorInfo {
                id: a.id.to_string(),
                name: a.name,
                role: "author".to_string(),
            })
            .collect())
    }

    async fn list_genres(&self) -> Result<Vec<String>, OpdsError> {
        let rows = Genres::find()
            .order_by_asc(genres::Column::Name)
            .all(&self.db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(|g| g.name).collect())
    }

    async fn list_tags(&self) -> Result<Vec<String>, OpdsError> {
        let rows = Tags::find()
            .order_by_asc(tags::Column::Name)
            .all(&self.db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(|t| t.name).collect())
    }

    async fn compute_facets(
        &self,
        query: &WorkQuery,
        axes: &[FacetAxis],
    ) -> Result<FacetCounts, OpdsError> {
        compute_facets(&self.db, &self.search_index, query, axes).await
    }

    async fn get_linked_file(
        &self,
        work_id: &str,
        inventory_id: Option<&str>,
    ) -> Result<(Vec<u8>, String, String), OpdsError> {
        // Garbage work_ids return MissingField rather than 500; the
        // get_work call below also handles invalid ULIDs but doing
        // the check here lets us emit a clearer message before the
        // heavier DB round-trip.
        if DbId::from_str(work_id).is_err() {
            return Err(OpdsError::MissingField(format!("work_id {work_id}")));
        }

        let Some(work) = self.get_work(work_id).await? else {
            return Err(OpdsError::MissingField(format!("work {work_id}")));
        };

        // Edition selection: pin to inventory_id when supplied, else
        // take the first edition with a non-null file_path. The order
        // is whatever `assemble_works` produced (editions ordered by
        // `editions.created_at` ascending), so this picks the
        // earliest-linked edition when the client doesn't pick one.
        let selected = work.editions.iter().find_map(|ed| {
            let inv_id = ed.inventory_id.as_deref()?;
            let file_path = ed.file_path.as_deref()?;
            if let Some(want) = inventory_id {
                if inv_id == want {
                    Some((
                        inv_id.to_string(),
                        file_path.to_string(),
                        ed.file_mime_type.clone(),
                    ))
                } else {
                    None
                }
            } else {
                Some((
                    inv_id.to_string(),
                    file_path.to_string(),
                    ed.file_mime_type.clone(),
                ))
            }
        });

        let (inv_id, file_path, file_mime_type) = match selected {
            Some(s) => s,
            None => {
                return Err(OpdsError::MissingField(format!(
                    "work {work_id} has no linked file"
                )));
            }
        };

        // Validate via the shared pruning helper so a broken symlink
        // both surfaces as MissingField *and* nulls the row for the
        // next reader. Returning early here is enough; the caller can
        // re-fetch the aggregate and see an absent file_path.
        let parsed_inv_id = match DbId::from_str(&inv_id) {
            Ok(id) => id,
            Err(_) => return Err(OpdsError::MissingField(format!("inventory_id {inv_id}"))),
        };
        let valid_path =
            match validate_and_prune_file_path(&self.db, parsed_inv_id, &file_path).await {
                Some(p) => p,
                None => {
                    return Err(OpdsError::MissingField(format!(
                        "linked file for work {work_id} is missing on disk"
                    )));
                }
            };

        // Read the file. We use `std::fs::read` (sync, blocking) rather
        // than `tokio::fs` because the OPDS server runs on the same
        // thread pool as the SeaORM calls and the files are typically
        // small (< 50MB for an ebook). Streaming via tokio would be a
        // future optimization for very large files.
        //
        // The `Path` / `PathBuf` / `std::fs` calls here are explicitly
        // opted-out per clippy.toml because the download endpoint is
        // a thin shim around the OS file API; threading `camino` /
        // `fs_err` through it would just be ceremony. Matches the
        // pattern in `livtet_core::crud::file_info`.
        #[allow(clippy::disallowed_types, clippy::disallowed_methods)]
        fn read_and_filename(
            valid_path: &str,
            inv_id: &str,
        ) -> Result<(Vec<u8>, String), OpdsError> {
            let bytes = std::fs::read(valid_path).map_err(|e| {
                OpdsError::Database(format!("failed to read linked file {valid_path}: {e}"))
            })?;
            let filename = std::path::Path::new(valid_path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{inv_id}.bin"));
            Ok((bytes, filename))
        }
        let (bytes, filename) = read_and_filename(&valid_path, &inv_id)?;

        let mime_type = file_mime_type
            .or_else(|| mime_guess_from_path(&valid_path))
            .unwrap_or_else(|| "application/octet-stream".to_string());

        Ok((bytes, mime_type, filename))
    }
}

// ---------------------------------------------------------------------------
// Filter conversion
// ---------------------------------------------------------------------------

/// Translate the OPDS-facing `WorkQuery` (single-value string IDs) into the
/// `livtet_types::WorkFilters` (multi-value typed IDs).
///
/// `series_id` is not yet modeled on the core filter struct, so it's
/// accepted on the query for forward compatibility but currently has no
/// filtering effect.
fn work_query_to_filters(query: &WorkQuery) -> Result<CoreWorkFilters, OpdsError> {
    let parse_id = |s: &str| -> Result<DbId, OpdsError> {
        DbId::from_str(s).map_err(|e| OpdsError::Database(e.to_string()))
    };

    let tag_ids: Vec<DbId> = query
        .tag_id
        .as_deref()
        .map(parse_id)
        .transpose()?
        .into_iter()
        .collect();
    let genre_ids: Vec<DbId> = query
        .genre_id
        .as_deref()
        .map(parse_id)
        .transpose()?
        .into_iter()
        .collect();
    let author_ids: Vec<DbId> = query
        .author_id
        .as_deref()
        .map(parse_id)
        .transpose()?
        .into_iter()
        .collect();
    let subject_ids: Vec<DbId> = query
        .subject_id
        .as_deref()
        .map(parse_id)
        .transpose()?
        .into_iter()
        .collect();
    let publisher_ids: Vec<DbId> = query
        .publisher_id
        .as_deref()
        .map(parse_id)
        .transpose()?
        .into_iter()
        .collect();
    let format_ids: Vec<DbId> = query
        .format_id
        .as_deref()
        .map(parse_id)
        .transpose()?
        .into_iter()
        .collect();
    let language_ids: Vec<DbId> = query
        .language_id
        .as_deref()
        .map(parse_id)
        .transpose()?
        .into_iter()
        .collect();

    let sort_by = match query.sort_by.as_deref() {
        Some("title") | Some("Title") => Some(CoreWorkSortBy::Title),
        Some("updated") | Some("updated_at") | Some("UpdatedAt") => Some(CoreWorkSortBy::UpdatedAt),
        _ => Some(CoreWorkSortBy::CreatedAt),
    };

    let sort_direction = match query.sort_direction.as_deref() {
        Some("asc") | Some("ASC") | Some("Asc") => Some(CoreSortDirection::Asc),
        _ => Some(CoreSortDirection::Desc),
    };

    // `series_id` is not yet modeled on the core filter struct; it's
    // accepted on the query for forward compatibility and therefore has
    // no filtering effect today.
    let _ = query.series_id.as_deref();

    Ok(CoreWorkFilters {
        tag_ids,
        genre_ids,
        subject_ids,
        publisher_ids,
        author_ids,
        format_ids,
        language_ids,
        sort_by,
        sort_direction,
        limit: None,
        has_file: None,
    })
}

/// Compute the set of work IDs that pass the active filters. Uses
/// SQL (junction-table path) because Tantivy only indexes
/// edition-level categorical IDs.
async fn compute_active_work_ids(
    db: &DatabaseConnection,
    filters: &CoreWorkFilters,
) -> Result<Vec<DbId>, OpdsError> {
    let mut query = Works::find();
    query = apply_work_filter_id_subqueries(query, filters, db).await?;
    let rows = query.all(db).await.map_err(db_err)?;
    Ok(rows.into_iter().map(|w| w.id).collect())
}

/// Resolve format IDs from [`CoreWorkFilters`] to text labels by
/// looking up the `formats` table. Used to build
/// [`WorkFiltersResolved`] so format filters work through Tantivy's
/// text-based `format` field.
async fn resolve_format_labels(
    db: &DatabaseConnection,
    filters: &CoreWorkFilters,
) -> Result<Vec<String>, OpdsError> {
    if filters.format_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = Formats::find()
        .filter(formats::Column::Id.is_in(filters.format_ids.clone()))
        .all(db)
        .await
        .map_err(db_err)?;
    Ok(rows.into_iter().map(|f| f.name).collect())
}

/// Resolve language IDs from [`CoreWorkFilters`] to text labels by
/// looking up the `languages` table. Used to build
/// [`WorkFiltersResolved`] so language filters work through Tantivy's
/// text-based `language` field.
async fn resolve_language_labels(
    db: &DatabaseConnection,
    filters: &CoreWorkFilters,
) -> Result<Vec<String>, OpdsError> {
    if filters.language_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = Languages::find()
        .filter(languages::Column::Id.is_in(filters.language_ids.clone()))
        .all(db)
        .await
        .map_err(db_err)?;
    Ok(rows.into_iter().map(|l| l.name).collect())
}

/// Apply the six junction/edition subquery filters. Each non-empty axis
/// restricts the works query to works that have at least one
/// junction/edition row pointing back at one of the requested IDs.
/// Title `LIKE` search, sort, and pagination are layered on top by the
/// callers; this helper deliberately does not touch them so it can be
/// reused for count, facet-id, and the OPDS list path without branching.
async fn apply_work_filter_id_subqueries(
    mut query: sea_orm::Select<Works>,
    filters: &CoreWorkFilters,
    db: &DatabaseConnection,
) -> Result<sea_orm::Select<Works>, OpdsError> {
    if !filters.tag_ids.is_empty() {
        let work_ids: Vec<DbId> = work_tags::Entity::find()
            .filter(work_tags::Column::TagId.is_in(filters.tag_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.work_id)
            .collect();
        query = query.filter(works::Column::Id.is_in(work_ids));
    }
    if !filters.genre_ids.is_empty() {
        let work_ids: Vec<DbId> = work_genres::Entity::find()
            .filter(work_genres::Column::GenreId.is_in(filters.genre_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.work_id)
            .collect();
        query = query.filter(works::Column::Id.is_in(work_ids));
    }
    if !filters.subject_ids.is_empty() {
        let work_ids: Vec<DbId> = work_subjects::Entity::find()
            .filter(work_subjects::Column::SubjectId.is_in(filters.subject_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.work_id)
            .collect();
        query = query.filter(works::Column::Id.is_in(work_ids));
    }
    if !filters.publisher_ids.is_empty() {
        let work_ids: Vec<DbId> = work_publishers::Entity::find()
            .filter(work_publishers::Column::PublisherId.is_in(filters.publisher_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.work_id)
            .collect();
        query = query.filter(works::Column::Id.is_in(work_ids));
    }
    if !filters.author_ids.is_empty() {
        let work_ids: Vec<DbId> = work_authors::Entity::find()
            .filter(work_authors::Column::AuthorId.is_in(filters.author_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.work_id)
            .collect();
        query = query.filter(works::Column::Id.is_in(work_ids));
    }
    if !filters.format_ids.is_empty() {
        let work_ids: Vec<DbId> = Editions::find()
            .filter(editions::Column::FormatId.is_in(filters.format_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.work_id)
            .collect();
        query = query.filter(works::Column::Id.is_in(work_ids));
    }
    if !filters.language_ids.is_empty() {
        let work_ids: Vec<DbId> = Editions::find()
            .filter(editions::Column::LanguageId.is_in(filters.language_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.work_id)
            .collect();
        query = query.filter(works::Column::Id.is_in(work_ids));
    }
    Ok(query)
}

// ---------------------------------------------------------------------------
// Facet counts
// ---------------------------------------------------------------------------

/// Cap on the number of values returned per axis. Keeps the feed response
/// bounded when a library has thousands of distinct tags or authors.
const FACET_LIMIT: usize = 50;
/// Per-axis facet computation, called from the trait method. The
/// `WorkQuery` is first reduced to a set of "active" work IDs via
/// SQL (since Tantivy only indexes edition-level categorical IDs;
/// work-level tag/genre/subject/publisher matching must use the
/// junction tables), then each axis runs in turn.
///
/// The implementation uses the load-then-group approach: the junction
/// tables are small relative to the `works` table, and the existing
/// patterns in `assemble_works` already do batched lookups for related
/// entities. Each axis runs at most two queries (junction rows + value
/// labels).
///
/// SQLite is used for the filter matching (work-level junction tables);
/// the resulting work IDs are then used for facet computation.
async fn compute_facets(
    db: &DatabaseConnection,
    _search_index: &SearchIndex,
    query: &WorkQuery,
    axes: &[FacetAxis],
) -> Result<FacetCounts, OpdsError> {
    if axes.is_empty() {
        return Ok(FacetCounts::default());
    }

    let filters = work_query_to_filters(query)?;
    let active_work_ids = compute_active_work_ids(db, &filters).await?;

    // Each axis runs in turn. The handler fires the whole `compute_facets`
    // call alongside `list_works` and `count_works` via `tokio::try_join!`,
    // so further fanning-out the per-axis queries isn't a meaningful win
    // against the single-writer SQLite connection. The sequential loop
    // also keeps the error path simple: a failure in any axis fails the
    // whole call.
    let mut out = FacetCounts::default();
    for axis in axes {
        let counts = compute_axis_facets(db, &active_work_ids, *axis, query).await?;
        match axis {
            FacetAxis::Author => out.authors = counts,
            FacetAxis::Tag => out.tags = counts,
            FacetAxis::Genre => out.genres = counts,
            FacetAxis::Subject => out.subjects = counts,
            FacetAxis::Publisher => out.publishers = counts,
            FacetAxis::Format => out.formats = counts,
            FacetAxis::Language => out.languages = counts,
        }
    }
    Ok(out)
}

/// Compute the [`FacetCount`]s for a single axis. The active work ID set
/// scopes the counts; the `WorkQuery` is consulted to flag the value that
/// matches the current filter as `active: true`.
async fn compute_axis_facets(
    db: &DatabaseConnection,
    active_work_ids: &[DbId],
    axis: FacetAxis,
    query: &WorkQuery,
) -> Result<Vec<FacetCount>, OpdsError> {
    if active_work_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Per-axis implementation. Each arm pulls the relevant (value_id,
    // work_id) pairs for the active work set, groups in memory, fetches
    // labels in a single batched query, and assembles [`FacetCount`]s.
    //
    // The label-lookup is inlined per axis because SeaORM's `Column`
    // enum doesn't share an `Id` variant across entities — each
    // label-bearing entity has its own primary-key variant. Going
    // generic there costs more code than it saves.
    let (pairs, label_by_id): (Vec<(DbId, DbId)>, HashMap<DbId, String>) = match axis {
        FacetAxis::Author => {
            let rows = work_authors::Entity::find()
                .filter(work_authors::Column::WorkId.is_in(active_work_ids.to_vec()))
                .all(db)
                .await
                .map_err(db_err)?;
            let pairs: Vec<(DbId, DbId)> =
                rows.into_iter().map(|r| (r.author_id, r.work_id)).collect();
            let value_ids: Vec<DbId> = pairs.iter().map(|(v, _)| *v).collect();
            let label: HashMap<DbId, String> = if value_ids.is_empty() {
                HashMap::new()
            } else {
                Authors::find()
                    .filter(authors::Column::Id.is_in(value_ids))
                    .all(db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .map(|a| (a.id, a.name))
                    .collect()
            };
            (pairs, label)
        }
        FacetAxis::Tag => {
            let rows = work_tags::Entity::find()
                .filter(work_tags::Column::WorkId.is_in(active_work_ids.to_vec()))
                .all(db)
                .await
                .map_err(db_err)?;
            let pairs: Vec<(DbId, DbId)> =
                rows.into_iter().map(|r| (r.tag_id, r.work_id)).collect();
            let value_ids: Vec<DbId> = pairs.iter().map(|(v, _)| *v).collect();
            let label: HashMap<DbId, String> = if value_ids.is_empty() {
                HashMap::new()
            } else {
                Tags::find()
                    .filter(tags::Column::Id.is_in(value_ids))
                    .all(db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .map(|t| (t.id, t.name))
                    .collect()
            };
            (pairs, label)
        }
        FacetAxis::Genre => {
            let rows = work_genres::Entity::find()
                .filter(work_genres::Column::WorkId.is_in(active_work_ids.to_vec()))
                .all(db)
                .await
                .map_err(db_err)?;
            let pairs: Vec<(DbId, DbId)> =
                rows.into_iter().map(|r| (r.genre_id, r.work_id)).collect();
            let value_ids: Vec<DbId> = pairs.iter().map(|(v, _)| *v).collect();
            let label: HashMap<DbId, String> = if value_ids.is_empty() {
                HashMap::new()
            } else {
                Genres::find()
                    .filter(genres::Column::Id.is_in(value_ids))
                    .all(db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .map(|g| (g.id, g.name))
                    .collect()
            };
            (pairs, label)
        }
        FacetAxis::Subject => {
            let rows = work_subjects::Entity::find()
                .filter(work_subjects::Column::WorkId.is_in(active_work_ids.to_vec()))
                .all(db)
                .await
                .map_err(db_err)?;
            let pairs: Vec<(DbId, DbId)> = rows
                .into_iter()
                .map(|r| (r.subject_id, r.work_id))
                .collect();
            let value_ids: Vec<DbId> = pairs.iter().map(|(v, _)| *v).collect();
            let label: HashMap<DbId, String> = if value_ids.is_empty() {
                HashMap::new()
            } else {
                Subjects::find()
                    .filter(subjects::Column::Id.is_in(value_ids))
                    .all(db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .map(|s| (s.id, s.name))
                    .collect()
            };
            (pairs, label)
        }
        FacetAxis::Publisher => {
            let rows = work_publishers::Entity::find()
                .filter(work_publishers::Column::WorkId.is_in(active_work_ids.to_vec()))
                .all(db)
                .await
                .map_err(db_err)?;
            let pairs: Vec<(DbId, DbId)> = rows
                .into_iter()
                .map(|r| (r.publisher_id, r.work_id))
                .collect();
            let value_ids: Vec<DbId> = pairs.iter().map(|(v, _)| *v).collect();
            let label: HashMap<DbId, String> = if value_ids.is_empty() {
                HashMap::new()
            } else {
                Publishers::find()
                    .filter(publishers::Column::Id.is_in(value_ids))
                    .all(db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .map(|p| (p.id, p.name))
                    .collect()
            };
            (pairs, label)
        }
        FacetAxis::Format => {
            let pairs = edition_inventory_format_pairs(db, active_work_ids).await?;
            let value_ids: Vec<DbId> = pairs.iter().map(|(v, _)| *v).collect();
            let label: HashMap<DbId, String> = if value_ids.is_empty() {
                HashMap::new()
            } else {
                Formats::find()
                    .filter(formats::Column::Id.is_in(value_ids))
                    .all(db)
                    .await
                    .map_err(db_err)?
                    .into_iter()
                    .map(|f| (f.id, f.name))
                    .collect()
            };
            (pairs, label)
        }
        FacetAxis::Language => {
            // No `languages` table yet; the raw ID is the label.
            let pairs = edition_language_pairs(db, active_work_ids).await?;
            (pairs, HashMap::new())
        }
    };

    // Group (value_id -> distinct work IDs).
    let mut value_to_works: HashMap<DbId, std::collections::HashSet<DbId>> = HashMap::new();
    for (value_id, work_id) in pairs {
        value_to_works.entry(value_id).or_default().insert(work_id);
    }
    if value_to_works.is_empty() {
        return Ok(Vec::new());
    }

    let active_value_id = axis.current_filter(query).map(str::to_string);

    let mut counts: Vec<FacetCount> = value_to_works
        .into_iter()
        .map(|(value_id, works)| {
            let id = value_id.to_string();
            let label = label_by_id
                .get(&value_id)
                .cloned()
                .unwrap_or_else(|| id.clone());
            let active = active_value_id.as_deref() == Some(id.as_str());
            FacetCount {
                id,
                label,
                count: works.len() as i64,
                active,
            }
        })
        .collect();

    // Sort by count desc, then label asc, then truncate to the per-axis
    // cap. The task spec calls for this order: popular values float to
    // the top, ties broken alphabetically.
    counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    counts.truncate(FACET_LIMIT);
    Ok(counts)
}

/// Read the format axis from `editions.format_id` (the canonical
/// binding — `digital_inventory.format_id` was removed in m0012).
/// Returns distinct `(format_id, work_id)` pairs where the edition
/// belongs to a work in the active set and the edition has a
/// format binding.
async fn edition_inventory_format_pairs(
    db: &DatabaseConnection,
    active_work_ids: &[DbId],
) -> Result<Vec<(DbId, DbId)>, OpdsError> {
    if active_work_ids.is_empty() {
        return Ok(Vec::new());
    }
    let editions: Vec<editions::Model> = Editions::find()
        .filter(editions::Column::WorkId.is_in(active_work_ids.to_vec()))
        .all(db)
        .await
        .map_err(db_err)?;
    Ok(editions
        .into_iter()
        .filter_map(|e| e.format_id.map(|fid| (fid, e.work_id)))
        .collect())
}

/// Read `(language_id, work_id)` from `editions` for the active work set.
/// There is no `languages` table yet so the `id` doubles as the label.
async fn edition_language_pairs(
    db: &DatabaseConnection,
    active_work_ids: &[DbId],
) -> Result<Vec<(DbId, DbId)>, OpdsError> {
    if active_work_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = Editions::find()
        .filter(editions::Column::WorkId.is_in(active_work_ids.to_vec()))
        .all(db)
        .await
        .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .filter_map(|e| e.language_id.map(|lang| (lang, e.work_id)))
        .collect())
}

// ---------------------------------------------------------------------------
// Aggregate assembly
// ---------------------------------------------------------------------------

/// Build `WorkAggregate`s for the given works, batch-loading every related
/// table in a single pass.
async fn assemble_works(
    db: &DatabaseConnection,
    works: Vec<works::Model>,
) -> Result<Vec<WorkAggregate>, OpdsError> {
    if works.is_empty() {
        return Ok(vec![]);
    }

    let work_ids: Vec<DbId> = works.iter().map(|w| w.id).collect();

    // ---- editions + per-edition side data ----
    let all_editions: Vec<editions::Model> = Editions::find()
        .filter(editions::Column::WorkId.is_in(work_ids.clone()))
        .order_by_asc(editions::Column::CreatedAt)
        .all(db)
        .await
        .map_err(db_err)?;

    let edition_ids: Vec<DbId> = all_editions.iter().map(|e| e.id).collect();

    // inventory (1 per edition) — gives us cover_path and file_path.
    // The format binding lives on `editions.format_id` (canonical);
    // `digital_inventory.format_id` was removed in m0012.
    //
    // `digital_inventory.edition_id` is UNIQUE since m0011 in
    // core/livet-data (uq_digital_inventory_edition_id), so the
    // query returns at most one row per edition. The HashMap
    // collect below cannot collide — the schema enforces it.
    let inventory_by_edition: HashMap<DbId, digital_inventory::Model> = if edition_ids.is_empty() {
        HashMap::new()
    } else {
        EditionsInventory::find()
            .filter(digital_inventory::Column::EditionId.is_in(edition_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|inv| (inv.edition_id, inv))
            .collect()
    };

    // format names for the format_ids we found (read from editions, the
    // canonical binding — the inventory row no longer carries format_id).
    let format_ids: Vec<DbId> = all_editions.iter().filter_map(|e| e.format_id).collect();
    let format_name_by_id: HashMap<DbId, String> = if format_ids.is_empty() {
        HashMap::new()
    } else {
        Formats::find()
            .filter(formats::Column::Id.is_in(format_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|f| (f.id, f.name))
            .collect()
    };

    // edition publishers
    let edition_publishers_pairs: Vec<edition_publishers::Model> = if edition_ids.is_empty() {
        vec![]
    } else {
        EditionPublishers::find()
            .filter(edition_publishers::Column::EditionId.is_in(edition_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
    };
    let edition_publisher_ids: Vec<DbId> = edition_publishers_pairs
        .iter()
        .map(|ep| ep.publisher_id)
        .collect();
    let publisher_by_id: HashMap<DbId, publishers::Model> = if edition_publisher_ids.is_empty() {
        HashMap::new()
    } else {
        Publishers::find()
            .filter(publishers::Column::Id.is_in(edition_publisher_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|p| (p.id, p))
            .collect()
    };
    let mut edition_publisher_name: HashMap<DbId, String> = HashMap::new();
    for ep in &edition_publishers_pairs {
        if let Some(p) = publisher_by_id.get(&ep.publisher_id) {
            edition_publisher_name.insert(ep.edition_id, p.name.clone());
        }
    }

    // edition authors
    let edition_authors_rows: Vec<edition_authors::Model> = if edition_ids.is_empty() {
        vec![]
    } else {
        EditionAuthors::find()
            .filter(edition_authors::Column::EditionId.is_in(edition_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
    };

    // edition identifiers (URN values)
    let edition_identifier_rows: Vec<edition_identifiers::Model> = if edition_ids.is_empty() {
        vec![]
    } else {
        EditionIdentifiers::find()
            .filter(edition_identifiers::Column::EditionId.is_in(edition_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
    };
    let identifier_ids: Vec<DbId> = edition_identifier_rows
        .iter()
        .map(|ei| ei.identifier_id)
        .collect();
    let identifier_by_id: HashMap<DbId, identifiers::Model> = if identifier_ids.is_empty() {
        HashMap::new()
    } else {
        Identifiers::find()
            .filter(identifiers::Column::Id.is_in(identifier_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|i| (i.id, i))
            .collect()
    };
    let mut identifiers_by_edition: HashMap<DbId, Vec<String>> = HashMap::new();
    let mut isbn_by_edition: HashMap<DbId, String> = HashMap::new();
    for ei in &edition_identifier_rows {
        if let Some(ident) = identifier_by_id.get(&ei.identifier_id) {
            identifiers_by_edition
                .entry(ei.edition_id)
                .or_default()
                .push(ident.value.clone());

            // Track ISBN specifically
            if ident.kind == "isbn" {
                isbn_by_edition.insert(ei.edition_id, ident.value.clone());
            }
        }
    }

    // series: edition -> series_entries -> series
    let series_entry_rows: Vec<series_entries::Model> = if edition_ids.is_empty() {
        vec![]
    } else {
        SeriesEntries::find()
            .filter(series_entries::Column::EditionId.is_in(edition_ids.clone()))
            .all(db)
            .await
            .map_err(db_err)?
    };
    let series_ids: Vec<DbId> = series_entry_rows.iter().map(|se| se.series_id).collect();
    let series_by_id: HashMap<DbId, series::Model> = if series_ids.is_empty() {
        HashMap::new()
    } else {
        Series::find()
            .filter(series::Column::Id.is_in(series_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|s| (s.id, s))
            .collect()
    };
    // Group series by *work* (any edition belonging to the work that is in
    // a series contributes that series to the work).
    let mut series_ids_by_work: HashMap<DbId, Vec<DbId>> = HashMap::new();
    for ed in &all_editions {
        for se in &series_entry_rows {
            if se.edition_id == ed.id {
                series_ids_by_work
                    .entry(ed.work_id)
                    .or_default()
                    .push(se.series_id);
            }
        }
    }

    // ---- work-level side data ----
    // work authors
    let work_author_rows = work_authors::Entity::find()
        .filter(work_authors::Column::WorkId.is_in(work_ids.clone()))
        .all(db)
        .await
        .map_err(db_err)?;
    let all_work_author_ids: Vec<DbId> = work_author_rows.iter().map(|wa| wa.author_id).collect();
    let all_edition_author_ids: Vec<DbId> =
        edition_authors_rows.iter().map(|ea| ea.author_id).collect();
    // `DbId` is `Hash + Eq` but not `Ord`, so dedup with a `HashSet` rather
    // than `sort`/`dedup` on a `Vec`.
    let mut seen_authors: std::collections::HashSet<DbId> = std::collections::HashSet::new();
    let mut all_author_ids: Vec<DbId> =
        Vec::with_capacity(all_work_author_ids.len() + all_edition_author_ids.len());
    for id in all_work_author_ids
        .iter()
        .chain(all_edition_author_ids.iter())
        .copied()
    {
        if seen_authors.insert(id) {
            all_author_ids.push(id);
        }
    }
    let author_by_id: HashMap<DbId, authors::Model> = if all_author_ids.is_empty() {
        HashMap::new()
    } else {
        Authors::find()
            .filter(authors::Column::Id.is_in(all_author_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    };

    let mut authors_by_work: HashMap<DbId, Vec<AuthorInfo>> = HashMap::new();
    for wa in &work_author_rows {
        if let Some(a) = author_by_id.get(&wa.author_id) {
            authors_by_work
                .entry(wa.work_id)
                .or_default()
                .push(AuthorInfo {
                    id: a.id.to_string(),
                    name: a.name.clone(),
                    role: wa.role.clone(),
                });
        }
    }

    let mut authors_by_edition: HashMap<DbId, Vec<AuthorInfo>> = HashMap::new();
    for ea in &edition_authors_rows {
        if let Some(a) = author_by_id.get(&ea.author_id) {
            authors_by_edition
                .entry(ea.edition_id)
                .or_default()
                .push(AuthorInfo {
                    id: a.id.to_string(),
                    name: a.name.clone(),
                    role: ea.role.clone(),
                });
        }
    }

    // work tags / genres / subjects / publishers
    let work_tag_rows = work_tags::Entity::find()
        .filter(work_tags::Column::WorkId.is_in(work_ids.clone()))
        .all(db)
        .await
        .map_err(db_err)?;
    let work_genre_rows = work_genres::Entity::find()
        .filter(work_genres::Column::WorkId.is_in(work_ids.clone()))
        .all(db)
        .await
        .map_err(db_err)?;
    let work_subject_rows = work_subjects::Entity::find()
        .filter(work_subjects::Column::WorkId.is_in(work_ids.clone()))
        .all(db)
        .await
        .map_err(db_err)?;
    let work_publisher_rows = work_publishers::Entity::find()
        .filter(work_publishers::Column::WorkId.is_in(work_ids.clone()))
        .all(db)
        .await
        .map_err(db_err)?;

    let tag_ids: Vec<DbId> = work_tag_rows.iter().map(|wt| wt.tag_id).collect();
    let genre_ids: Vec<DbId> = work_genre_rows.iter().map(|wg| wg.genre_id).collect();
    let subject_ids: Vec<DbId> = work_subject_rows.iter().map(|ws| ws.subject_id).collect();
    let work_publisher_ids: Vec<DbId> = work_publisher_rows
        .iter()
        .map(|wp| wp.publisher_id)
        .collect();

    let tag_by_id: HashMap<DbId, tags::Model> = if tag_ids.is_empty() {
        HashMap::new()
    } else {
        Tags::find()
            .filter(tags::Column::Id.is_in(tag_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|t| (t.id, t))
            .collect()
    };
    let genre_by_id: HashMap<DbId, genres::Model> = if genre_ids.is_empty() {
        HashMap::new()
    } else {
        Genres::find()
            .filter(genres::Column::Id.is_in(genre_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|g| (g.id, g))
            .collect()
    };
    let subject_by_id: HashMap<DbId, subjects::Model> = if subject_ids.is_empty() {
        HashMap::new()
    } else {
        Subjects::find()
            .filter(subjects::Column::Id.is_in(subject_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|s| (s.id, s))
            .collect()
    };
    let work_publisher_by_id: HashMap<DbId, publishers::Model> = if work_publisher_ids.is_empty() {
        HashMap::new()
    } else {
        Publishers::find()
            .filter(publishers::Column::Id.is_in(work_publisher_ids))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|p| (p.id, p))
            .collect()
    };

    let mut tags_by_work: HashMap<DbId, Vec<String>> = HashMap::new();
    for wt in &work_tag_rows {
        if let Some(t) = tag_by_id.get(&wt.tag_id) {
            tags_by_work
                .entry(wt.work_id)
                .or_default()
                .push(t.name.clone());
        }
    }
    let mut genres_by_work: HashMap<DbId, Vec<String>> = HashMap::new();
    for wg in &work_genre_rows {
        if let Some(g) = genre_by_id.get(&wg.genre_id) {
            genres_by_work
                .entry(wg.work_id)
                .or_default()
                .push(g.name.clone());
        }
    }
    let mut subjects_by_work: HashMap<DbId, Vec<String>> = HashMap::new();
    for ws in &work_subject_rows {
        if let Some(s) = subject_by_id.get(&ws.subject_id) {
            subjects_by_work
                .entry(ws.work_id)
                .or_default()
                .push(s.name.clone());
        }
    }
    let mut publishers_by_work: HashMap<DbId, Vec<String>> = HashMap::new();
    for wp in &work_publisher_rows {
        if let Some(p) = work_publisher_by_id.get(&wp.publisher_id) {
            publishers_by_work
                .entry(wp.work_id)
                .or_default()
                .push(p.name.clone());
        }
    }

    // ---- stitch editions to works ----
    let mut editions_by_work: HashMap<DbId, Vec<EditionAggregate>> = HashMap::new();
    for ed in all_editions {
        let inventory = inventory_by_edition.get(&ed.id);
        // Format binding is read from `ed.format_id` (the canonical
        // binding on the edition row); `digital_inventory.format_id`
        // was removed in m0012.
        let format_name = ed
            .format_id
            .and_then(|fid| format_name_by_id.get(&fid).cloned());
        let publisher_name = edition_publisher_name.get(&ed.id).cloned();
        let identifiers = identifiers_by_edition.remove(&ed.id).unwrap_or_default();
        let authors = authors_by_edition.remove(&ed.id).unwrap_or_default();
        let isbn = isbn_by_edition.get(&ed.id).cloned();

        // The OPDS acquisition link and download endpoint both need
        // to address a specific linked file. Surface the inventory
        // row's id and the mime type detected from the file's
        // extension — both are populated even when the file itself is
        // temporarily broken; the download handler will prune-and-404
        // in that case rather than handing out a stale link.
        let inventory_id = inventory.map(|i| i.id.to_string());
        let file_path = inventory.and_then(|i| i.file_path.clone());
        let file_mime_type = file_path.as_deref().and_then(mime_guess_from_path);

        let aggregate = EditionAggregate {
            id: ed.id.to_string(),
            title: ed.title,
            isbn,
            language: None, // languages table has no SeaORM entity yet
            publisher: publisher_name,
            published_date: ed.published_date.map(|d| d.to_string()),
            page_count: None, // TODO: determine where page_count should be stored
            format: format_name,
            authors,
            cover_path: inventory.and_then(|i| i.cover_path.clone()),
            file_path,
            identifiers,
            inventory_id,
            file_mime_type,
        };
        editions_by_work
            .entry(ed.work_id)
            .or_default()
            .push(aggregate);
    }

    // ---- stitch everything together ----
    let mut out = Vec::with_capacity(works.len());
    for w in works {
        let mut series_aggregates: Vec<SeriesInfo> = Vec::new();
        if let Some(series_ids) = series_ids_by_work.get(&w.id) {
            // de-dup series per work
            let mut seen: Vec<DbId> = vec![];
            for sid in series_ids {
                if seen.contains(sid) {
                    continue;
                }
                seen.push(*sid);
                if let Some(s) = series_by_id.get(sid) {
                    series_aggregates.push(SeriesInfo {
                        id: s.id.to_string(),
                        name: s.name.clone(),
                        position: None, // position is per-edition, not per-work
                    });
                }
            }
        }

        out.push(WorkAggregate {
            id: w.id.to_string(),
            title: w.title,
            description: w.description,
            sort_title: w.sort_title,
            preferred_edition_id: w.preferred_edition_id.map(|id| id.to_string()),
            authors: authors_by_work.remove(&w.id).unwrap_or_default(),
            editions: editions_by_work.remove(&w.id).unwrap_or_default(),
            tags: tags_by_work.remove(&w.id).unwrap_or_default(),
            genres: genres_by_work.remove(&w.id).unwrap_or_default(),
            subjects: subjects_by_work.remove(&w.id).unwrap_or_default(),
            publishers: publishers_by_work.remove(&w.id).unwrap_or_default(),
            series: series_aggregates,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Confirm that `file_path` still exists on disk. A missing file — including a
/// dangling symlink — means the linked edition is unusable: the inventory row's
/// `file_path` is nulled so later reads observe an absent file, and `None` is
/// returned so the caller can surface a 404.
#[allow(clippy::disallowed_types, clippy::disallowed_methods)]
async fn validate_and_prune_file_path(
    db: &DatabaseConnection,
    inventory_id: DbId,
    file_path: &str,
) -> Option<String> {
    if std::path::Path::new(file_path).exists() {
        return Some(file_path.to_string());
    }

    let _ = EditionsInventory::update(digital_inventory::ActiveModel {
        id: Set(inventory_id),
        file_path: Set(None),
        ..Default::default()
    })
    .exec(db)
    .await;

    None
}

fn db_err(e: sea_orm::DbErr) -> OpdsError {
    OpdsError::Database(e.to_string())
}

/// Detect the MIME type for `path` from its file extension via
/// `mime_guess`. Returns `None` when the extension is empty or
/// unknown — callers fall back to `application/octet-stream`.
///
/// `mime_guess` has a richer extension table than `mime::from_path`,
/// which matters for ebook formats like `.epub` (which `mime` reports
/// as `application/octet-stream` but `mime_guess` recognises as
/// `application/epub+zip`).
fn mime_guess_from_path(path: &str) -> Option<String> {
    let guess = mime_guess::from_path(path).first();
    guess.map(|m| m.essence_str().to_string())
}
