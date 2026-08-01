//! Google Books REST provider.
//!
//! Endpoint: https://www.googleapis.com/books/v1/volumes
//! Auth: api_key in query string (compile-time secret)

use async_trait::async_trait;
use serde::Deserialize;
use tracing::debug;
use tracing::warn;

use crate::commands::remote_search::{Provider, ProviderError, ProviderId, RawSearchHit};
use livtet_core::DbId;
use livtet_core::covers::{CacheKey, CoverFetcher, FetchError, FetchedCover};
use livtet_core::data::entities::{editions, identifiers, work_identifiers};
use livtet_core::data::orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

pub struct GoogleBooks {
    http: reqwest::Client,
    api_key: String,
}

impl GoogleBooks {
    pub fn new(http: reqwest::Client, api_key: String) -> Self {
        Self { http, api_key }
    }

    // cover fetcher key encoding helpers
    fn content_key_for_volume(volume_id: &str, zoom: u8) -> String {
        format!("google_books::google_books_id::{volume_id}::{zoom}::jpg")
    }
}

#[async_trait]
impl CoverFetcher for GoogleBooks {
    fn priority(&self) -> u8 {
        0
    }

    async fn keys_for(
        &self,
        edition_id: DbId,
        db: &DatabaseConnection,
    ) -> Result<Vec<CacheKey>, livtet_core::data::orm::DbErr> {
        let edition = editions::Entity::find_by_id(edition_id).one(db).await?;
        let Some(edition) = edition else {
            return Ok(Vec::new());
        };

        let work_idents = work_identifiers::Entity::find()
            .filter(work_identifiers::Column::WorkId.eq(edition.work_id))
            .all(db)
            .await?;

        let mut keys = Vec::new();
        for wi in work_idents {
            let identifier = identifiers::Entity::find_by_id(wi.identifier_id)
                .one(db)
                .await?;
            let Some(identifier) = identifier else {
                continue;
            };

            if identifier.kind != "google_books" {
                continue;
            }

            let volume_id = match identifier.value.strip_prefix("urn:google_books:") {
                Some(v) => v.to_string(),
                None => continue,
            };

            keys.push(CacheKey {
                key: Self::content_key_for_volume(&volume_id, 1),
                provider: "google_books".into(),
                identifier_type: "google_books_id".into(),
                identifier_value: volume_id,
                size: "1".into(),
            });
        }

        Ok(keys)
    }

    async fn fetch(&self, key: &CacheKey) -> Result<FetchedCover, FetchError> {
        let url = format!(
            "https://books.google.com/books/content?id={}&printsec=frontcover&img=1&zoom={}",
            key.identifier_value, key.size,
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

fn to_iso_639_1(code: &str) -> Option<&'static str> {
    match code {
        "eng" => Some("en"),
        "fra" | "fre" => Some("fr"),
        "spa" => Some("es"),
        "deu" | "ger" => Some("de"),
        "ita" => Some("it"),
        "por" => Some("pt"),
        "rus" => Some("ru"),
        "jpn" => Some("ja"),
        "zho" | "chi" => Some("zh"),
        "ara" => Some("ar"),
        "nld" | "dut" => Some("nl"),
        "pol" => Some("pl"),
        "tur" => Some("tr"),
        "ces" | "cze" => Some("cs"),
        "swe" => Some("sv"),
        _ => None,
    }
}

#[async_trait]
impl Provider for GoogleBooks {
    fn id(&self) -> ProviderId {
        ProviderId::GoogleBooks
    }

    /// Google Books API rejects `maxResults` greater than 40 with
    /// HTTP 400. See <https://developers.google.com/docs/api-parameters>.
    fn max_limit(&self) -> u32 {
        40
    }

    async fn search(
        &self,
        query: &str,
        limit: u32,
        language: Option<&str>,
    ) -> Result<Vec<RawSearchHit>, ProviderError> {
        debug!(
            query = %query,
            limit,
            ?language,
            provider = "google_books",
            "sending search request"
        );
        let mut url = format!(
            "https://www.googleapis.com/books/v1/volumes?q={}&maxResults={}&key={}",
            urlencoding::encode(query),
            limit,
            self.api_key,
        );
        if let Some(lang) = language.and_then(to_iso_639_1) {
            url.push_str("&langRestrict=");
            url.push_str(lang);
        }
        let res = self.http.get(&url).send().await?;
        let status = res.status();
        debug!(
            status = %status,
            provider = "google_books",
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
                provider = "google_books",
                "auth rejected — API key missing or invalid"
            );
            return Err(ProviderError::Auth);
        }
        if !status.is_success() {
            warn!(
                status = status.as_u16(),
                provider = "google_books",
                "non-success status"
            );
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: res.text().await.unwrap_or_default(),
            });
        }
        let body: GoogleBooksResponse = res.json().await.map_err(|e| {
            warn!(error = %e, provider = "google_books", "failed to parse response body");
            ProviderError::Parse(e.to_string())
        })?;
        let hits: Vec<RawSearchHit> = body
            .items
            .unwrap_or_default()
            .into_iter()
            .filter_map(map_google_books_hit)
            .collect();
        debug!(
            hit_count = hits.len(),
            provider = "google_books",
            "search completed"
        );
        Ok(hits)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GoogleBooksResponse {
    items: Option<Vec<GoogleBooksItem>>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GoogleBooksItem {
    id: Option<String>,
    volume_info: Option<GoogleBooksVolumeInfo>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct GoogleBooksVolumeInfo {
    title: Option<String>,
    authors: Option<Vec<String>>,
    publisher: Option<String>,
    published_date: Option<String>,
    description: Option<String>,
    page_count: Option<u32>,
    language: Option<String>,
    industry_identifiers: Option<Vec<GoogleBooksIdentifier>>,
    image_links: Option<GoogleBooksImageLinks>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GoogleBooksIdentifier {
    #[serde(rename = "type")]
    kind: Option<String>,
    identifier: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GoogleBooksImageLinks {
    thumbnail: Option<String>,
    small_thumbnail: Option<String>,
}

fn map_google_books_hit(item: GoogleBooksItem) -> Option<RawSearchHit> {
    let id = item.id?;
    let info = item.volume_info.unwrap_or_default();
    let title = info.title.unwrap_or_default();
    let authors = info.authors.unwrap_or_default();
    let identifiers = info.industry_identifiers.unwrap_or_default();
    let isbn_13 = identifiers
        .iter()
        .find(|id| id.kind.as_deref() == Some("ISBN_13"))
        .and_then(|id| id.identifier.clone());
    let isbn_10 = identifiers
        .iter()
        .find(|id| id.kind.as_deref() == Some("ISBN_10"))
        .and_then(|id| id.identifier.clone());
    let isbn = isbn_10.or_else(|| isbn_13.clone());
    let cover_url = info
        .image_links
        .and_then(|i| i.thumbnail.or(i.small_thumbnail))
        .map(|s| s.replace("http://", "https://").replace("&edge=curl", ""));
    Some(RawSearchHit {
        provider_id: ProviderId::GoogleBooks,
        provider_work_id: id,
        title,
        authors,
        isbn,
        isbn_13,
        publisher: info.publisher,
        page_count: info.page_count,
        language: info.language,
        published_date: info.published_date,
        cover_url,
        description: info.description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_full_response() {
        let json = serde_json::json!({
            "id": "abc123",
            "volumeInfo": {
                "title": "The Hobbit",
                "authors": ["J.R.R. Tolkien"],
                "publisher": "HarperCollins",
                "publishedDate": "1937",
                "description": "Bilbo's adventure.",
                "pageCount": 310,
                "language": "en",
                "industryIdentifiers": [
                    { "type": "ISBN_10", "identifier": "0261103289" },
                    { "type": "ISBN_13", "identifier": "9780261103283" }
                ],
                "imageLinks": {
                    "thumbnail": "http://books.google.com/img.jpg"
                }
            }
        });
        let item: GoogleBooksItem = serde_json::from_value(json).unwrap();
        let hit = map_google_books_hit(item).unwrap();
        assert_eq!(hit.provider_id, ProviderId::GoogleBooks);
        assert_eq!(hit.provider_work_id, "abc123");
        assert_eq!(hit.title, "The Hobbit");
        assert_eq!(hit.authors, vec!["J.R.R. Tolkien"]);
        assert_eq!(hit.isbn.as_deref(), Some("0261103289"));
        assert_eq!(hit.isbn_13.as_deref(), Some("9780261103283"));
        assert_eq!(hit.publisher.as_deref(), Some("HarperCollins"));
        assert_eq!(hit.page_count, Some(310));
        assert_eq!(hit.language.as_deref(), Some("en"));
        assert_eq!(hit.published_date.as_deref(), Some("1937"));
        assert_eq!(
            hit.cover_url.as_deref(),
            Some("https://books.google.com/img.jpg"),
            "http:// should be rewritten to https://"
        );
    }

    #[test]
    fn drops_hit_without_id() {
        let json = serde_json::json!({ "volumeInfo": { "title": "Orphan" } });
        let item: GoogleBooksItem = serde_json::from_value(json).unwrap();
        assert!(map_google_books_hit(item).is_none());
    }

    #[test]
    fn tolerates_unknown_fields() {
        let json = serde_json::json!({
            "id": "x",
            "volumeInfo": { "title": "Future" },
            "future_field_we_dont_know_about": { "nested": [1, 2, 3] }
        });
        let item: GoogleBooksItem = serde_json::from_value(json).unwrap();
        assert!(map_google_books_hit(item).is_some());
    }

    #[test]
    fn prefers_isbn_10_for_primary_isbn() {
        let json = serde_json::json!({
            "id": "x",
            "volumeInfo": {
                "title": "T",
                "industryIdentifiers": [
                    { "type": "ISBN_13", "identifier": "9780000000000" },
                    { "type": "ISBN_10", "identifier": "0000000000" }
                ]
            }
        });
        let item: GoogleBooksItem = serde_json::from_value(json).unwrap();
        let hit = map_google_books_hit(item).unwrap();
        assert_eq!(hit.isbn.as_deref(), Some("0000000000"));
        assert_eq!(hit.isbn_13.as_deref(), Some("9780000000000"));
    }

    #[tokio::test]
    #[ignore = "live HTTP test — requires network and a valid API key; run via `cargo test -- --ignored`"]
    async fn live_search_returns_hits() {
        let key = crate::secrets::GOOGLE_BOOKS_API_KEY;
        if key.is_empty() {
            eprintln!("GOOGLE_BOOKS_API_KEY not set; skipping live Google Books test");
            return;
        }
        let http = crate::http::build_client();
        let provider = GoogleBooks::new(http, key.to_string());
        let hits = match provider.search("The Hobbit", 5, None).await {
            Ok(hits) => hits,
            Err(crate::commands::remote_search::ProviderError::Auth)
            | Err(crate::commands::remote_search::ProviderError::Http {
                status: 400 | 401 | 403,
                ..
            }) => {
                eprintln!("Google Books rejected the API key; skipping live test");
                return;
            }
            Err(e) => panic!("live Google Books search failed: {e}"),
        };
        assert!(!hits.is_empty(), "live search returned no hits");
        let hit = &hits[0];
        assert_eq!(hit.provider_id, ProviderId::GoogleBooks);
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
