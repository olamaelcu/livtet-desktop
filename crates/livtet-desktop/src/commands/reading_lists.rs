//! Reading lists: the desktop command surface over `reading_lists` and
//! `reading_list_book`.
//!
//! The core crate exposes the same verbs to mobile through `livtet-ffi`, but
//! that store owns its own pool and search index; the desktop already owns both
//! in [`AppState`], so these commands read the same tables directly instead of
//! opening a second writer. They mirror the engine contract exactly: the same
//! DTO shape, the same partial-patch rules, the same strict-permutation reorder,
//! and the same error text for a rejected reorder.
//!
//! A reading list is its own collection, separate from the ATProto shelves
//! (`to-read` / `reading` / `read` / `dnf`), which keep their own UI and
//! records.

use livtet_core::data::entities::{reading_list_book, reading_lists};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
    TransactionTrait,
};
use livtet_types::{DbId, now_primitive};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::types::AppState;

/// A reading-list command error, shaped like the other command errors.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ReadingListError {
    /// Stable machine code: `invalid_id` | `not_found` | `invalid_input` |
    /// `database`.
    pub code: String,
    pub message: String,
}

impl ReadingListError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    fn invalid_id(id: &str) -> Self {
        Self::new("invalid_id", format!("invalid input: invalid id {id}"))
    }

    fn not_found(list_id: DbId) -> Self {
        Self::new("not_found", format!("not found: reading_lists with id {list_id}"))
    }

    fn invalid_input(message: impl Into<String>) -> Self {
        Self::new("invalid_input", format!("invalid input: {}", message.into()))
    }

    fn database<E: std::fmt::Display>(error: E) -> Self {
        Self::new("database", format!("database error: {error}"))
    }
}

/// One reading list, with its edition ids in position order.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReadingList {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub edition_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// Fields accepted when updating a reading list. Only `Some` fields change;
/// an empty `description` clears it. `name` is non-nullable, so an empty or
/// blank name is rejected rather than cleared.
#[derive(Debug, Clone, PartialEq, Deserialize, Type)]
pub struct ReadingListPatch {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Every reading list, each with its edition ids in position order.
#[tauri::command]
#[specta::specta]
pub async fn list_reading_lists(
    state: State<'_, AppState>,
) -> Result<Vec<ReadingList>, ReadingListError> {
    list_reading_lists_inner(&state.db.db_conn()).await
}

/// One reading list by id. Fails `not_found` for an unknown id.
#[tauri::command]
#[specta::specta]
pub async fn get_reading_list(
    list_id: String,
    state: State<'_, AppState>,
) -> Result<ReadingList, ReadingListError> {
    let id = parse_id(&list_id)?;
    get_reading_list_inner(&state.db.db_conn(), id).await
}

/// Rename and/or re-describe a reading list. Partial update: only the `Some`
/// fields of `patch` change. Bumps `updated_at`.
#[tauri::command]
#[specta::specta]
pub async fn update_reading_list(
    list_id: String,
    patch: ReadingListPatch,
    state: State<'_, AppState>,
) -> Result<ReadingList, ReadingListError> {
    let id = parse_id(&list_id)?;
    update_reading_list_inner(&state.db.db_conn(), id, patch).await
}

/// Replace a list's book order with an explicit one.
///
/// `edition_ids` must be a permutation of the list's current members: an
/// edition the list does not contain, a repeated edition, or an omission is
/// rejected with the engine's message. Positions are rewritten to `0..n`
/// inside one transaction and `updated_at` is bumped.
#[tauri::command]
#[specta::specta]
pub async fn reorder_reading_list(
    list_id: String,
    edition_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<ReadingList, ReadingListError> {
    let id = parse_id(&list_id)?;
    let ordered = edition_ids
        .iter()
        .map(|edition_id| parse_id(edition_id))
        .collect::<Result<Vec<_>, _>>()?;
    reorder_reading_list_inner(&state.db.db_conn(), id, ordered).await
}

fn parse_id(id: &str) -> Result<DbId, ReadingListError> {
    id.parse::<DbId>().map_err(|_| ReadingListError::invalid_id(id))
}

async fn list_reading_lists_inner(
    db: &DatabaseConnection,
) -> Result<Vec<ReadingList>, ReadingListError> {
    let lists = reading_lists::Entity::find()
        .order_by_asc(reading_lists::Column::CreatedAt)
        .all(db)
        .await
        .map_err(ReadingListError::database)?;

    let mut out = Vec::with_capacity(lists.len());
    for list in lists {
        out.push(list_dto(db, list).await?);
    }
    Ok(out)
}

async fn get_reading_list_inner(
    db: &DatabaseConnection,
    list_id: DbId,
) -> Result<ReadingList, ReadingListError> {
    let list = require_list(db, list_id).await?;
    list_dto(db, list).await
}

async fn update_reading_list_inner(
    db: &DatabaseConnection,
    list_id: DbId,
    patch: ReadingListPatch,
) -> Result<ReadingList, ReadingListError> {
    let list = require_list(db, list_id).await?;

    let name = match patch.name {
        Some(name) if name.trim().is_empty() => {
            return Err(ReadingListError::invalid_input(
                "reading list name must not be empty",
            ));
        }
        other => other,
    };

    let stamp = next_updated_at(list.updated_at);
    let mut active: reading_lists::ActiveModel = list.into();
    if let Some(name) = name {
        active.name = Set(name);
    }
    if let Some(description) = patch.description {
        active.description = Set(if description.is_empty() {
            None
        } else {
            Some(description)
        });
    }
    active.updated_at = Set(Some(stamp));
    let updated = active.update(db).await.map_err(ReadingListError::database)?;
    list_dto(db, updated).await
}

async fn reorder_reading_list_inner(
    db: &DatabaseConnection,
    list_id: DbId,
    edition_ids: Vec<DbId>,
) -> Result<ReadingList, ReadingListError> {
    let list = require_list(db, list_id).await?;

    let members = reading_list_book::Entity::find()
        .filter(reading_list_book::Column::ReadingListId.eq(list_id))
        .order_by_asc(reading_list_book::Column::Position)
        .all(db)
        .await
        .map_err(ReadingListError::database)?;
    let current: Vec<DbId> = members.iter().map(|m| m.edition_id).collect();

    // Reject anything that is not exactly the current member set: duplicates
    // would collide on the composite primary key, and a subset would leave an
    // arbitrary tail order.
    let mut ordered: Vec<DbId> = Vec::with_capacity(edition_ids.len());
    for edition_id in &edition_ids {
        if ordered.contains(edition_id) {
            return Err(ReadingListError::invalid_input(format!(
                "duplicate edition {edition_id} in reorder of reading list {list_id}"
            )));
        }
        if !current.contains(edition_id) {
            return Err(ReadingListError::invalid_input(format!(
                "edition {edition_id} is not in reading list {list_id}"
            )));
        }
        ordered.push(*edition_id);
    }
    if let Some(missing) = current.iter().find(|id| !ordered.contains(*id)) {
        return Err(ReadingListError::invalid_input(format!(
            "edition {missing} is missing from the reorder of reading list {list_id}"
        )));
    }

    let stamp = next_updated_at(list.updated_at);
    let txn = db.begin().await.map_err(ReadingListError::database)?;
    for (position, edition_id) in ordered.iter().enumerate() {
        let member = members
            .iter()
            .find(|m| m.edition_id == *edition_id)
            .cloned()
            .ok_or_else(|| {
                ReadingListError::invalid_input(format!(
                    "edition {edition_id} is not in reading list {list_id}"
                ))
            })?;
        if member.position == position as i32 {
            continue;
        }
        let mut active: reading_list_book::ActiveModel = member.into();
        active.position = Set(position as i32);
        active
            .update(&txn)
            .await
            .map_err(ReadingListError::database)?;
    }
    let mut active: reading_lists::ActiveModel = list.into();
    active.updated_at = Set(Some(stamp));
    active
        .update(&txn)
        .await
        .map_err(ReadingListError::database)?;
    txn.commit().await.map_err(ReadingListError::database)?;

    let list = require_list(db, list_id).await?;
    list_dto(db, list).await
}

/// Fail-closed lookup of a reading list row.
async fn require_list(
    db: &DatabaseConnection,
    list_id: DbId,
) -> Result<reading_lists::Model, ReadingListError> {
    reading_lists::Entity::find_by_id(list_id)
        .one(db)
        .await
        .map_err(ReadingListError::database)?
        .ok_or_else(|| ReadingListError::not_found(list_id))
}

/// DTO projection shared by every command that returns a list, so the
/// list-all and get-one surfaces cannot drift apart.
async fn list_dto(
    db: &DatabaseConnection,
    list: reading_lists::Model,
) -> Result<ReadingList, ReadingListError> {
    let members = reading_list_book::Entity::find()
        .filter(reading_list_book::Column::ReadingListId.eq(list.id))
        .order_by_asc(reading_list_book::Column::Position)
        .all(db)
        .await
        .map_err(ReadingListError::database)?;

    Ok(ReadingList {
        id: list.id.to_string(),
        name: list.name,
        description: list.description,
        edition_ids: members
            .into_iter()
            .map(|member| member.edition_id.to_string())
            .collect(),
        created_at: format_timestamp(list.created_at),
        updated_at: list.updated_at.map(format_timestamp),
    })
}

/// RFC 3339 text for a stored timestamp.
fn format_timestamp(timestamp: time::PrimitiveDateTime) -> String {
    use time::format_description::well_known::Rfc3339;
    timestamp
        .assume_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| timestamp.to_string())
}

/// Microsecond granularity of a stored timestamp: `SeaORM` renders datetimes
/// with `[subsecond digits:6]`, so anything finer is lost on write.
const NANOS_PER_STORE_TICK: u32 = 1_000;

/// Next `updated_at` for a list mutation: strictly greater than the stored
/// value. `now` is truncated to storage granularity first, so a
/// sub-microsecond clock tick between two mutations cannot compare as "newer"
/// and then round back onto the old value; when the truncated value is not
/// newer, one microsecond is added instead.
fn next_updated_at(prev: Option<time::PrimitiveDateTime>) -> time::PrimitiveDateTime {
    let now = now_primitive();
    let now = now
        .replace_nanosecond(now.nanosecond() / NANOS_PER_STORE_TICK * NANOS_PER_STORE_TICK)
        .unwrap_or(now);
    match prev {
        Some(prev) if now <= prev => prev + time::Duration::microseconds(1),
        _ => now,
    }
}
