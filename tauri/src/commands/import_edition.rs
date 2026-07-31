//! Import remote search results into the local catalog.
//!
//! Creates a new work + edition from a remote search hit, preserving
//! provenance by storing the provider's work ID and edition URL as identifiers.

use livtet_core::DbId;
use livtet_core::data::entities::{
    authors, edition_authors, edition_identifiers, editions, identifiers, languages, publishers,
    work_identifiers, works,
};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use livtet_core::{CommonLanguages, Isbn};
use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use time;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum ImportResult {
    Created { edition_id: String },
    AlreadyExists,
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state, request), err)]
pub async fn import_edition(
    state: State<'_, AppState>,
    request: ImportRequest,
) -> Result<ImportResult, String> {
    let db = state.db.db_conn();
    let result = import_edition_impl(&db, request)
        .await
        .map_err(|e| e.to_string())?;

    // Reindex after a successful import so the new edition
    // appears in search results immediately.
    if matches!(result, ImportResult::Created { .. }) {
        let db = state.db.db_conn();
        let search = state.search.read().await;
        search.reindex(&db).await.map_err(|e| e.to_string())?;
    }

    Ok(result)
}

pub async fn import_edition_impl(
    db: &DatabaseConnection,
    request: ImportRequest,
) -> miette::Result<ImportResult> {
    let isbn_urn = request
        .isbn_13
        .as_ref()
        .and_then(|i| Isbn::parse(i).ok())
        .map(|isbn| format!("urn:isbn:{}", isbn.as_str()));

    if let Some(ref urn) = isbn_urn {
        let existing: Option<identifiers::Model> = identifiers::Entity::find()
            .filter(identifiers::Column::Value.eq(urn))
            .one(db)
            .await
            .into_diagnostic()?;
        if existing.is_some() {
            tracing::info!("import skipped: ISBN already in catalog");
            return Ok(ImportResult::AlreadyExists);
        }
    }

    let now = livtet_core::now_primitive();

    let language_id = resolve_or_create_language(db, &request.language, now).await?;
    let publisher_id = resolve_or_create_publisher(db, &request.publisher, now).await?;
    let work_id = DbId::new();
    let edition_id = DbId::new();

    let work = works::ActiveModel {
        id: Set(work_id),
        title: Set(request.title.clone()),
        description: Set(request.description.clone()),
        created_at: Set(now),
        updated_at: Set(None),
        language_id: Set(language_id),
        ..Default::default()
    };
    work.insert(db).await.into_diagnostic()?;

    let edition = editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        title: Set(Some(request.title.clone())),
        published_date: Set(None),
        format_id: Set(None),
        language_id: Set(language_id),
        notes: Set(None),
        description: Set(request.description.clone()),
        created_at: Set(now),
        updated_at: Set(None),
        group_id: Set(None),
    };
    edition.insert(db).await.into_diagnostic()?;

    for (idx, author_name) in request.authors.iter().enumerate() {
        let author_id = create_author(db, author_name).await?;

        let edition_author = edition_authors::ActiveModel {
            edition_id: Set(edition_id),
            author_id: Set(author_id),
            role: Set(if idx == 0 {
                "author".into()
            } else {
                "contributor".into()
            }),
        };
        edition_author.insert(db).await.into_diagnostic()?;
    }

    if let Some(ref urn) = isbn_urn {
        let identifier = identifiers::ActiveModel {
            id: Set(DbId::new()),
            kind: Set("isbn".into()),
            value: Set(urn.clone()),
        };
        let inserted = identifier.insert(db).await.into_diagnostic()?;

        let edition_identifier = edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(inserted.id),
        };
        edition_identifier.insert(db).await.into_diagnostic()?;
    }

    let work_identifier = identifiers::ActiveModel {
        id: Set(DbId::new()),
        kind: Set(request.provider.clone()),
        value: Set(format!(
            "urn:{}:{}",
            request.provider, request.provider_work_id
        )),
    };
    let wid = work_identifier.insert(db).await.into_diagnostic()?;

    let work_ident = work_identifiers::ActiveModel {
        work_id: Set(work_id),
        identifier_id: Set(wid.id),
    };
    work_ident.insert(db).await.into_diagnostic()?;

    if let Some(ref url) = request.provider_edition_url {
        let edition_identifier = identifiers::ActiveModel {
            id: Set(DbId::new()),
            kind: Set("http".into()),
            value: Set(url.clone()),
        };
        let eid = edition_identifier.insert(db).await.into_diagnostic()?;

        let edition_ident = edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(eid.id),
        };
        edition_ident.insert(db).await.into_diagnostic()?;
    }

    if let Some(publisher_id) = publisher_id {
        use livtet_core::data::entities::edition_publishers;
        let edition_publisher = edition_publishers::ActiveModel {
            edition_id: Set(edition_id),
            publisher_id: Set(publisher_id),
        };
        edition_publisher.insert(db).await.into_diagnostic()?;
    }

    tracing::info!(edition_id = %edition_id, "imported edition");
    Ok(ImportResult::Created {
        edition_id: edition_id.to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ImportRequest {
    pub title: String,
    pub authors: Vec<String>,
    pub isbn: Option<String>,
    pub isbn_13: Option<String>,
    pub publisher: Option<String>,
    pub page_count: Option<u32>,
    pub language: Option<String>,
    pub published_date: Option<String>,
    pub description: Option<String>,
    pub provider: String,
    pub provider_work_id: String,
    pub provider_edition_url: Option<String>,
}

async fn resolve_or_create_language(
    db: &DatabaseConnection,
    lang_code: &Option<String>,
    now: time::PrimitiveDateTime,
) -> miette::Result<Option<DbId>> {
    let Some(code) = lang_code else {
        return Ok(None);
    };

    let lang_info = CommonLanguages::normalize_language_code(code);

    let lookup_code = lang_info
        .as_ref()
        .map(|li| li.code.as_str())
        .unwrap_or(code.as_str());

    let existing: Option<languages::Model> = languages::Entity::find()
        .filter(languages::Column::Code.eq(lookup_code))
        .one(db)
        .await
        .into_diagnostic()?;

    if let Some(l) = existing {
        return Ok(Some(l.id));
    }

    let (lang_name, flag_emoji) = match lang_info {
        Some(ref li) => (li.english_name.clone(), li.flag_emoji.clone()),
        None => (code.clone(), None),
    };

    let lang = languages::ActiveModel {
        id: Set(DbId::new()),
        name: Set(lang_name),
        code: Set(lookup_code.to_string()),
        flag_emoji: Set(flag_emoji),
        created_at: Set(now),
        updated_at: Set(None),
    };
    let lang = lang.insert(db).await.into_diagnostic()?;

    Ok(Some(lang.id))
}

async fn resolve_or_create_publisher(
    db: &DatabaseConnection,
    publisher: &Option<String>,
    now: time::PrimitiveDateTime,
) -> miette::Result<Option<DbId>> {
    let Some(name) = publisher else {
        return Ok(None);
    };

    let existing: Option<publishers::Model> = publishers::Entity::find()
        .filter(publishers::Column::Name.eq(name))
        .one(db)
        .await
        .into_diagnostic()?;

    if let Some(p) = existing {
        return Ok(Some(p.id));
    }

    let publisher = publishers::ActiveModel {
        id: Set(DbId::new()),
        name: Set(name.clone()),
        website: Set(None),
        logo_url: Set(None),
        created_at: Set(now),
        updated_at: Set(None),
    };
    let publisher = publisher.insert(db).await.into_diagnostic()?;

    Ok(Some(publisher.id))
}

async fn create_author(db: &DatabaseConnection, name: &str) -> miette::Result<DbId> {
    let existing: Option<authors::Model> = authors::Entity::find()
        .filter(authors::Column::Name.eq(name))
        .one(db)
        .await
        .into_diagnostic()?;

    if let Some(a) = existing {
        return Ok(a.id);
    }

    let author = authors::ActiveModel {
        id: Set(DbId::new()),
        name: Set(name.into()),
    };
    let author = author.insert(db).await.into_diagnostic()?;

    Ok(author.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::data::TestDb;

    async fn setup_test_db() -> TestDb {
        TestDb::new(None).await.unwrap()
    }

    #[tokio::test]
    async fn import_creates_work_and_edition() {
        let test_db = setup_test_db().await;
        let db = test_db.state().db_conn();

        let request = ImportRequest {
            title: "Test Book".to_string(),
            authors: vec!["Test Author".to_string()],
            isbn: None,
            isbn_13: None,
            publisher: None,
            page_count: None,
            language: None,
            published_date: None,
            description: None,
            provider: "google_books".to_string(),
            provider_work_id: "/books/test123".to_string(),
            provider_edition_url: Some("https://books.google.com/books?id=test123".to_string()),
        };

        let result = import_edition_impl(&db, request)
            .await
            .expect("import should succeed");
        assert!(matches!(result, ImportResult::Created { .. }));

        let works_vec: Vec<works::Model> = works::Entity::find()
            .filter(works::Column::Title.eq("Test Book"))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(works_vec.len(), 1);

        let editions: Vec<editions::Model> = editions::Entity::find()
            .filter(editions::Column::WorkId.eq(works_vec[0].id))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(editions.len(), 1);
        assert_eq!(editions[0].title, Some("Test Book".to_string()));
    }

    #[tokio::test]
    async fn import_deduplicates_by_isbn() {
        let test_db = setup_test_db().await;
        let db = test_db.state().db_conn();

        let request = ImportRequest {
            title: "Test Book".to_string(),
            authors: vec!["Test Author".to_string()],
            isbn: None,
            isbn_13: Some("9780000000002".to_string()),
            publisher: None,
            page_count: None,
            language: None,
            published_date: None,
            description: None,
            provider: "google_books".to_string(),
            provider_work_id: "test1".to_string(),
            provider_edition_url: None,
        };

        let result1 = import_edition_impl(&db, request.clone())
            .await
            .expect("first import should succeed");
        let result2 = import_edition_impl(&db, request)
            .await
            .expect("second import should succeed");

        assert!(matches!(result1, ImportResult::Created { .. }));
        assert!(matches!(result2, ImportResult::AlreadyExists));
    }
}
