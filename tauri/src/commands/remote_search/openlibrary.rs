//! OpenLibrary REST provider.
//!
//! Endpoint: https://openlibrary.org/search.json
//! Auth: none.
//!
//! Cover URLs are built from the `cover_i` field:
//!   https://covers.openlibrary.org/b/id/{cover_i}-M.jpg

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::commands::remote_search::{Provider, ProviderError, ProviderId, RawSearchHit};
use livtet_core::DbId;
use livtet_core::covers::{CacheKey, CoverFetcher, FetchError, FetchedCover};
use livtet_core::data::entities::{edition_identifiers, editions, identifiers};
use livtet_core::data::orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

pub struct OpenLibrary {
    http: reqwest::Client,
}

impl OpenLibrary {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    fn content_key_for_isbn(isbn: &str, size: &str) -> String {
        format!("openlibrary::isbn::{isbn}::{size}::jpg")
    }
}

#[async_trait]
impl CoverFetcher for OpenLibrary {
    fn priority(&self) -> u8 {
        2
    }

    async fn keys_for(
        &self,
        edition_id: DbId,
        db: &DatabaseConnection,
    ) -> Result<Vec<CacheKey>, livtet_core::data::orm::DbErr> {
        let edition = editions::Entity::find_by_id(edition_id).one(db).await?;
        if edition.is_none() {
            return Ok(Vec::new());
        }

        let edition_idents = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(db)
            .await?;

        let mut keys = Vec::new();
        for ei in edition_idents {
            let identifier = identifiers::Entity::find_by_id(ei.identifier_id)
                .one(db)
                .await?;
            let Some(identifier) = identifier else {
                continue;
            };

            if identifier.kind != "isbn" {
                continue;
            }

            let isbn = match identifier.value.strip_prefix("urn:isbn:") {
                Some(v) => v.to_string(),
                None => continue,
            };

            for size in &["S", "M", "L"] {
                keys.push(CacheKey {
                    key: Self::content_key_for_isbn(&isbn, size),
                    provider: "openlibrary".into(),
                    identifier_type: "isbn".into(),
                    identifier_value: isbn.clone(),
                    size: (*size).into(),
                });
            }
        }

        Ok(keys)
    }

    async fn fetch(&self, key: &CacheKey) -> Result<FetchedCover, FetchError> {
        let url = format!(
            "https://covers.openlibrary.org/b/{}/{}-{}.jpg",
            key.identifier_type, key.identifier_value, key.size,
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| FetchError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(FetchError::NotFound);
        }
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| FetchError::Network(e.to_string()))?;
        Ok(FetchedCover {
            bytes: bytes.to_vec(),
            content_type,
        })
    }
}

#[async_trait]
impl Provider for OpenLibrary {
    fn id(&self) -> ProviderId {
        ProviderId::OpenLibrary
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<RawSearchHit>, ProviderError> {
        debug!(
            query = %query,
            limit,
            provider = "openlibrary",
            "sending search request"
        );
        let url = format!(
            "https://openlibrary.org/search.json?q={}&limit={}&fields=key,title,author_name,first_publish_year,isbn,publisher,language,number_of_pages_median,cover_i",
            urlencoding::encode(query),
            limit,
        );
        let res = self.http.get(&url).send().await?;
        let status = res.status();
        debug!(
            status = %status,
            provider = "openlibrary",
            "received response"
        );
        if status.as_u16() == 429 {
            let retry = res
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(60);
            return Err(ProviderError::RateLimited {
                retry_after_seconds: retry,
            });
        }
        if !status.is_success() {
            warn!(
                status = status.as_u16(),
                provider = "openlibrary",
                "non-success status"
            );
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: res.text().await.unwrap_or_default(),
            });
        }
        let body: OpenLibraryResponse = res.json().await.map_err(|e| {
            warn!(error = %e, provider = "openlibrary", "failed to parse response body");
            ProviderError::Parse(e.to_string())
        })?;
        let hits: Vec<RawSearchHit> = body
            .docs
            .into_iter()
            .filter_map(map_openlibrary_hit)
            .collect();
        debug!(
            hit_count = hits.len(),
            provider = "openlibrary",
            "search completed"
        );
        Ok(hits)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OpenLibraryResponse {
    docs: Vec<OpenLibraryDoc>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OpenLibraryDoc {
    key: Option<String>,
    title: Option<String>,
    author_name: Option<Vec<String>>,
    first_publish_year: Option<i32>,
    isbn: Option<Vec<String>>,
    publisher: Option<Vec<String>>,
    language: Option<Vec<String>>,
    number_of_pages_median: Option<u32>,
    cover_i: Option<i64>,
}

fn map_openlibrary_hit(doc: OpenLibraryDoc) -> Option<RawSearchHit> {
    let key = doc.key?;
    let title = doc.title.unwrap_or_default();
    let authors = doc.author_name.unwrap_or_default();
    let isbns = doc.isbn.unwrap_or_default();
    let isbn_13 = isbns.iter().find(|s| s.len() == 13).cloned();
    let isbn = isbns
        .iter()
        .find(|s| s.len() == 10)
        .cloned()
        .or_else(|| isbn_13.clone());
    let cover_url = doc
        .cover_i
        .map(|i| format!("https://covers.openlibrary.org/b/id/{i}-M.jpg"));
    let publisher = doc.publisher.and_then(|mut p| {
        if p.is_empty() {
            None
        } else {
            Some(p.remove(0))
        }
    });
    let language = doc.language.and_then(|mut l| {
        if l.is_empty() {
            None
        } else {
            Some(l.remove(0))
        }
    });
    Some(RawSearchHit {
        provider_id: ProviderId::OpenLibrary,
        provider_work_id: key,
        title,
        authors,
        isbn,
        isbn_13,
        publisher,
        page_count: doc.number_of_pages_median,
        language,
        published_date: doc.first_publish_year.map(|y| y.to_string()),
        cover_url,
        description: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_full_response_with_cover_i() {
        let json = serde_json::json!({
            "key": "/works/OL45804W",
            "title": "Pride and Prejudice",
            "author_name": ["Jane Austen"],
            "first_publish_year": 1813,
            "isbn": ["0141439513", "9780141439518"],
            "publisher": ["Penguin Classics"],
            "language": ["eng"],
            "number_of_pages_median": 432,
            "cover_i": 8231856
        });
        let doc: OpenLibraryDoc = serde_json::from_value(json).unwrap();
        let hit = map_openlibrary_hit(doc).unwrap();
        assert_eq!(hit.provider_id, ProviderId::OpenLibrary);
        assert_eq!(hit.provider_work_id, "/works/OL45804W");
        assert_eq!(hit.title, "Pride and Prejudice");
        assert_eq!(hit.authors, vec!["Jane Austen"]);
        assert_eq!(hit.isbn.as_deref(), Some("0141439513"));
        assert_eq!(hit.isbn_13.as_deref(), Some("9780141439518"));
        assert_eq!(hit.publisher.as_deref(), Some("Penguin Classics"));
        assert_eq!(hit.language.as_deref(), Some("eng"));
        assert_eq!(hit.page_count, Some(432));
        assert_eq!(hit.published_date.as_deref(), Some("1813"));
        assert_eq!(
            hit.cover_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/8231856-M.jpg")
        );
    }

    #[test]
    fn drops_hit_without_key() {
        let doc: OpenLibraryDoc = serde_json::from_value(serde_json::json!({
            "title": "Orphan"
        }))
        .unwrap();
        assert!(map_openlibrary_hit(doc).is_none());
    }

    #[test]
    fn takes_first_publisher_and_language() {
        let doc: OpenLibraryDoc = serde_json::from_value(serde_json::json!({
            "key": "/works/x",
            "publisher": ["First", "Second"],
            "language": ["en", "de"]
        }))
        .unwrap();
        let hit = map_openlibrary_hit(doc).unwrap();
        assert_eq!(hit.publisher.as_deref(), Some("First"));
        assert_eq!(hit.language.as_deref(), Some("en"));
    }

    #[test]
    fn tolerates_unknown_fields() {
        let doc: OpenLibraryDoc = serde_json::from_value(serde_json::json!({
            "key": "/works/x",
            "future_field_we_dont_know_about": {"nested": [1, 2, 3]}
        }))
        .unwrap();
        assert!(map_openlibrary_hit(doc).is_some());
    }

    #[tokio::test]
    #[ignore = "live HTTP test — requires network; run via `cargo test -- --ignored`"]
    async fn live_search_returns_hits() {
        let http = crate::http::build_client();
        let provider = OpenLibrary::new(http);
        let hits = provider
            .search("The Hobbit", 5)
            .await
            .expect("live OpenLibrary search should succeed");
        assert!(!hits.is_empty(), "live search returned no hits");
        let hit = &hits[0];
        assert_eq!(hit.provider_id, ProviderId::OpenLibrary);
        assert!(
            hit.title.to_lowercase().contains("hobbit"),
            "expected title to contain 'hobbit', got {:?}",
            hit.title
        );
        assert!(
            hit.authors
                .iter()
                .any(|a| a.to_lowercase().contains("tolkien")),
            "expected an author containing 'tolkien', got {:?}",
            hit.authors
        );
        assert!(!hit.provider_work_id.is_empty());
    }
}
