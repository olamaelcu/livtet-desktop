//! Hardcover GraphQL provider.
//!
//! Endpoint: https://api.hardcover.app/v1/graphql
//! Auth: Authorization: Bearer <token>
//! Rate limit: 60 req/min. Max timeout: 30s. Query depth limit: 3.
//!
//! The query returns `data.search.results`, where `results` is
//! an opaque JSON array (Typesense docs). We type the envelope
//! with serde::Deserialize structs and type the per-hit
//! `HardcoverBookDoc` so serde ignores unknown fields. Publisher
//! and language are not in the Book search response — they live
//! on the Editions schema and would require a second lookup per
//! hit. We leave them as None.

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::commands::remote_search::{Provider, ProviderError, ProviderId, RawSearchHit};
use livtet_core::DbId;
use livtet_core::covers::{CacheKey, CoverFetcher, FetchError, FetchedCover};
use livtet_core::data::entities::{editions, identifiers, work_identifiers};
use livtet_core::data::orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

const HARDCOVER_URL: &str = "https://api.hardcover.app/v1/graphql";

const HARDCOVER_QUERY: &str = r#"
query BookSearch($query: String!, $per_page: Int!) {
  search(query: $query, query_type: "Book", per_page: $per_page, page: 1) {
    results
  }
}"#;

pub struct Hardcover {
    http: reqwest::Client,
    api_key: Option<String>,
}

impl Hardcover {
    pub fn new(http: reqwest::Client, api_key: Option<String>) -> Self {
        Self { http, api_key }
    }

    fn content_key_for_url(url: &str) -> String {
        format!("hardcover::url::{url}::original::jpg")
    }
}

#[async_trait]
impl Provider for Hardcover {
    fn id(&self) -> ProviderId {
        ProviderId::Hardcover
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<RawSearchHit>, ProviderError> {
        let key = self.api_key.as_deref().ok_or(ProviderError::Auth)?;
        debug!(
            query = %query,
            limit,
            provider = "hardcover",
            "sending GraphQL search request"
        );
        let body = serde_json::json!({
            "query": HARDCOVER_QUERY,
            "variables": { "query": query, "per_page": limit },
        });
        let res = self
            .http
            .post(HARDCOVER_URL)
            .header("Authorization", format!("Bearer {key}"))
            .json(&body)
            .send()
            .await?;
        let status = res.status();
        debug!(
            status = %status,
            provider = "hardcover",
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
        if status.as_u16() == 401 || status.as_u16() == 403 {
            warn!(
                status = status.as_u16(),
                provider = "hardcover",
                "auth rejected — API key missing or invalid"
            );
            return Err(ProviderError::Auth);
        }
        if !status.is_success() {
            warn!(
                status = status.as_u16(),
                provider = "hardcover",
                "non-success status"
            );
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: res.text().await.unwrap_or_default(),
            });
        }
        let body: HardcoverResponse = res.json().await.map_err(|e| {
            warn!(error = %e, provider = "hardcover", "failed to parse response body");
            ProviderError::Parse(e.to_string())
        })?;
        if let Some(errors) = body.errors {
            warn!(
                provider = "hardcover",
                error_count = errors.len(),
                "GraphQL errors returned"
            );
            return Err(ProviderError::GraphQL {
                messages: errors.into_iter().map(|e| e.message).collect(),
            });
        }
        let Some(data) = body.data else {
            return Ok(Vec::new());
        };
        let hits: Vec<RawSearchHit> = data
            .search
            .results
            .into_iter()
            .filter_map(map_hardcover_hit)
            .collect();
        debug!(
            hit_count = hits.len(),
            provider = "hardcover",
            "search completed"
        );
        Ok(hits)
    }
}

#[async_trait]
impl CoverFetcher for Hardcover {
    fn priority(&self) -> u8 {
        1
    }

    async fn keys_for(
        &self,
        edition_id: DbId,
        db: &DatabaseConnection,
    ) -> Result<Vec<CacheKey>, livtet_core::data::orm::DbErr> {
        let api_key = match self.api_key.as_deref() {
            Some(k) => k,
            None => return Ok(Vec::new()),
        };

        let edition = editions::Entity::find_by_id(edition_id).one(db).await?;
        let Some(edition) = edition else {
            return Ok(Vec::new());
        };

        let work_idents = work_identifiers::Entity::find()
            .filter(work_identifiers::Column::WorkId.eq(edition.work_id))
            .all(db)
            .await?;

        let mut hardcover_ids = Vec::new();
        for wi in work_idents {
            let identifier = identifiers::Entity::find_by_id(wi.identifier_id)
                .one(db)
                .await?;
            let Some(identifier) = identifier else {
                continue;
            };
            if identifier.kind == "hardcover" {
                if let Some(id_str) = identifier.value.strip_prefix("urn:hardcover:") {
                    if let Ok(id) = id_str.parse::<i64>() {
                        hardcover_ids.push(id);
                    }
                }
            }
        }

        if hardcover_ids.is_empty() {
            return Ok(Vec::new());
        }

        let query = r#"
            query BookImage($id: Int!) {
              books(where: {id: {_eq: $id}}) {
                image {
                  url
                }
              }
            }"#;

        let mut keys = Vec::new();
        for book_id in hardcover_ids {
            let body = serde_json::json!({
                "query": query,
                "variables": { "id": book_id },
            });
            let resp = match self
                .http
                .post(HARDCOVER_URL)
                .header("Authorization", format!("Bearer {api_key}"))
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "Hardcover book-by-id GraphQL request failed");
                    continue;
                }
            };
            let status = resp.status();
            if !status.is_success() {
                warn!(status = status.as_u16(), book_id = %book_id, "Hardcover book-by-id request failed");
                continue;
            }

            #[derive(Deserialize)]
            struct BookByIdResponse {
                data: Option<BookByIdData>,
            }
            #[derive(Deserialize)]
            struct BookByIdData {
                books: Vec<BookByIdBook>,
            }
            #[derive(Deserialize)]
            struct BookByIdBook {
                image: Option<BookByIdImage>,
            }
            #[derive(Deserialize)]
            struct BookByIdImage {
                url: Option<String>,
            }

            let parsed: BookByIdResponse = match resp.json().await {
                Ok(p) => p,
                Err(e) => {
                    warn!(error = %e, "Failed to parse Hardcover book-by-id response");
                    continue;
                }
            };

            let Some(url) = parsed
                .data
                .and_then(|d| d.books.into_iter().next())
                .and_then(|b| b.image)
                .and_then(|i| i.url)
            else {
                continue;
            };

            keys.push(CacheKey {
                key: Self::content_key_for_url(&url),
                provider: "hardcover".into(),
                identifier_type: "url".into(),
                identifier_value: url.clone(),
                size: "original".into(),
            });
        }

        Ok(keys)
    }

    async fn fetch(&self, key: &CacheKey) -> Result<FetchedCover, FetchError> {
        let resp = self
            .http
            .get(&key.identifier_value)
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

#[derive(Deserialize)]
struct HardcoverResponse {
    data: Option<HardcoverData>,
    errors: Option<Vec<HardcoverError>>,
}
#[derive(Deserialize)]
struct HardcoverData {
    search: HardcoverSearch,
}
#[derive(Deserialize)]
struct HardcoverSearch {
    results: Vec<HardcoverBookDoc>,
}
#[derive(Deserialize)]
struct HardcoverError {
    message: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HardcoverBookDoc {
    id: Option<String>,
    title: Option<String>,
    author_names: Option<Vec<String>>,
    isbns: Option<Vec<String>>,
    pages: Option<u32>,
    description: Option<String>,
    release_year: Option<i32>,
    image: Option<HardcoverImage>,
    slug: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HardcoverImage {
    url: Option<String>,
}

fn map_hardcover_hit(doc: HardcoverBookDoc) -> Option<RawSearchHit> {
    let id = doc.id?;
    let title = doc.title.unwrap_or_default();
    let authors = doc.author_names.unwrap_or_default();
    let isbns = doc.isbns.unwrap_or_default();
    let isbn_13 = isbns.iter().find(|s| s.len() == 13).cloned();
    let isbn = isbns
        .iter()
        .find(|s| s.len() == 10)
        .cloned()
        .or_else(|| isbn_13.clone());
    Some(RawSearchHit {
        provider_id: ProviderId::Hardcover,
        provider_work_id: id,
        title,
        authors,
        isbn,
        isbn_13,
        publisher: None,
        page_count: doc.pages,
        language: None,
        published_date: doc.release_year.map(|y| y.to_string()),
        cover_url: doc.image.and_then(|i| i.url),
        description: doc.description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_full_response_with_both_isbn_lengths() {
        let json = serde_json::json!({
            "id": "hcv-1",
            "title": "The Lord of the Rings",
            "author_names": ["J.R.R. Tolkien"],
            "isbns": ["0000000000", "9780000000000"],
            "pages": 1200,
            "description": "One ring to rule them all.",
            "release_year": 1954,
            "image": { "url": "https://cdn.hardcover.example/lotr.jpg" },
            "slug": "the-lord-of-the-rings"
        });
        let doc: HardcoverBookDoc = serde_json::from_value(json).unwrap();
        let hit = map_hardcover_hit(doc).unwrap();
        assert_eq!(hit.provider_id, ProviderId::Hardcover);
        assert_eq!(hit.provider_work_id, "hcv-1");
        assert_eq!(hit.title, "The Lord of the Rings");
        assert_eq!(hit.authors, vec!["J.R.R. Tolkien"]);
        assert_eq!(
            hit.isbn.as_deref(),
            Some("0000000000"),
            "ISBN-10 wins as primary"
        );
        assert_eq!(hit.isbn_13.as_deref(), Some("9780000000000"));
        assert_eq!(hit.page_count, Some(1200));
        assert_eq!(hit.published_date.as_deref(), Some("1954"));
        assert_eq!(
            hit.cover_url.as_deref(),
            Some("https://cdn.hardcover.example/lotr.jpg")
        );
        assert_eq!(
            hit.description.as_deref(),
            Some("One ring to rule them all.")
        );
        assert_eq!(hit.publisher, None, "not in the Book search doc");
        assert_eq!(hit.language, None, "not in the Book search doc");
    }

    #[test]
    fn drops_hit_without_id() {
        let doc: HardcoverBookDoc = serde_json::from_value(serde_json::json!({
            "title": "Orphan"
        }))
        .unwrap();
        assert!(map_hardcover_hit(doc).is_none());
    }

    #[test]
    fn tolerates_unknown_fields() {
        let doc: HardcoverBookDoc = serde_json::from_value(serde_json::json!({
            "id": "x",
            "future_field_we_dont_know_about": { "nested": [1, 2, 3] }
        }))
        .unwrap();
        assert!(map_hardcover_hit(doc).is_some());
    }

    #[test]
    fn falls_back_to_isbn13_when_no_isbn10_present() {
        let doc: HardcoverBookDoc = serde_json::from_value(serde_json::json!({
            "id": "x",
            "isbns": ["1234567890123"]
        }))
        .unwrap();
        let hit = map_hardcover_hit(doc).unwrap();
        assert_eq!(hit.isbn.as_deref(), Some("1234567890123"));
        assert_eq!(hit.isbn_13.as_deref(), Some("1234567890123"));
    }

    #[test]
    fn empty_isbns_yields_none() {
        let doc: HardcoverBookDoc = serde_json::from_value(serde_json::json!({
            "id": "x",
            "isbns": []
        }))
        .unwrap();
        let hit = map_hardcover_hit(doc).unwrap();
        assert_eq!(hit.isbn, None);
        assert_eq!(hit.isbn_13, None);
    }

    #[tokio::test]
    #[ignore = "live HTTP test — requires network and a valid API key; run via `cargo test -- --ignored`"]
    async fn live_search_returns_hits() {
        let key = match std::env::var("HARDCOVER_API_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => {
                eprintln!("HARDCOVER_API_KEY not set; skipping live Hardcover test");
                return;
            }
        };
        let http = crate::http::build_client();
        let provider = Hardcover::new(http, Some(key));
        let hits = match provider.search("The Lord of the Rings", 5).await {
            Ok(hits) => hits,
            Err(crate::commands::remote_search::ProviderError::Auth)
            | Err(crate::commands::remote_search::ProviderError::Http {
                status: 400 | 401 | 403,
                ..
            }) => {
                eprintln!("Hardcover rejected the API key; skipping live test");
                return;
            }
            Err(e) => panic!("live Hardcover search failed: {e}"),
        };
        assert!(!hits.is_empty(), "live search returned no hits");
        let hit = &hits[0];
        assert_eq!(hit.provider_id, ProviderId::Hardcover);
        assert!(
            hit.title.to_lowercase().contains("lord of the rings"),
            "expected title to contain 'lord of the rings', got {:?}",
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
