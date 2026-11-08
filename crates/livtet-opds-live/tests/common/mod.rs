//! Shared test support for the `livtet-opds-server` integration suites.

#![allow(dead_code)]

use livtet_core::data::entities::{
    work_authors, work_genres, work_publishers, work_subjects, work_tags, works,
};
use livtet_core::data::orm::{ActiveModelTrait, EntityTrait, Set};
use livtet_types::{DbId, now_primitive};
use sea_orm::DatabaseConnection;

/// Read a fixture file from `vendor/opds-test-catalog/`.
///
/// Resolves `path` relative to `<crate manifest dir>/../../vendor/opds-test-catalog/`,
/// matching the workspace layout where the catalog vendor tree lives at
/// the repo root and each OPDS crate sits one level deep.
pub fn fixture(path: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let base = camino::Utf8Path::new(manifest_dir)
        .parent()
        .expect("crate manifest dir has parent")
        .join("vendor/opds-test-catalog");
    let full = base.join(path);
    std::fs::read_to_string(full.as_std_path())
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", full))
}

/// Input accepted by [`create_work`].
///
/// Mirrors the shape the removed `livtet_core::crud::work::CreateWorkInput`
/// exposed so the existing test call sites stay unchanged.
pub struct CreateWorkInput {
    pub title: String,
    pub description: Option<String>,
    pub preferred_edition_id: Option<DbId>,
    pub work_authors: Vec<(DbId, String)>,
    pub work_tags: Vec<DbId>,
    pub work_genres: Vec<DbId>,
    pub work_subjects: Vec<DbId>,
    pub work_publishers: Vec<DbId>,
}

/// The work row created by [`create_work`], exposing `.work.id`.
pub struct CreatedWork {
    pub work: works::Model,
}

/// Insert a work plus its author/tag/genre/subject/publisher links.
///
/// The core CRUD module this suite was originally written against was removed
/// from `livtet-core`; this is the minimal local replacement.
pub async fn create_work(
    db: &DatabaseConnection,
    input: CreateWorkInput,
) -> Result<CreatedWork, sea_orm::DbErr> {
    let work_id = DbId::new();
    works::ActiveModel {
        id: Set(work_id),
        title: Set(input.title),
        description: Set(input.description),
        sort_title: Set(None),
        series_type: Set(None),
        language_id: Set(None),
        preferred_edition_id: Set(input.preferred_edition_id),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(db)
    .await?;

    for (author_id, role) in input.work_authors {
        work_authors::ActiveModel {
            work_id: Set(work_id),
            author_id: Set(author_id),
            role: Set(role),
        }
        .insert(db)
        .await?;
    }
    for tag_id in input.work_tags {
        work_tags::ActiveModel {
            work_id: Set(work_id),
            tag_id: Set(tag_id),
        }
        .insert(db)
        .await?;
    }
    for genre_id in input.work_genres {
        work_genres::ActiveModel {
            work_id: Set(work_id),
            genre_id: Set(genre_id),
        }
        .insert(db)
        .await?;
    }
    for subject_id in input.work_subjects {
        work_subjects::ActiveModel {
            work_id: Set(work_id),
            subject_id: Set(subject_id),
        }
        .insert(db)
        .await?;
    }
    for publisher_id in input.work_publishers {
        work_publishers::ActiveModel {
            work_id: Set(work_id),
            publisher_id: Set(publisher_id),
        }
        .insert(db)
        .await?;
    }

    let work = works::Entity::find_by_id(work_id)
        .one(db)
        .await?
        .expect("work row just inserted");
    Ok(CreatedWork { work })
}
