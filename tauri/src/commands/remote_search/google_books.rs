//! Google Books REST provider.
//!
//! Endpoint: https://www.googleapis.com/books/v1/volumes
//! Auth: api_key in query string (compile-time secret)

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::commands::remote_search::{Provider, ProviderError, ProviderId, RawSearchHit};

pub struct GoogleBooks {
    http: reqwest::Client,
    api_key: String,
}

impl GoogleBooks {
    pub fn new(http: reqwest::Client, api_key: String) -> Self {
        Self { http, api_key }
    }
}

#[async_trait]
impl Provider for GoogleBooks {
    fn id(&self) -> ProviderId { ProviderId::GoogleBooks }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<RawSearchHit>, ProviderError> {
        let url = format!(
            "https://www.googleapis.com/books/v1/volumes?q={}&maxResults={}&key={}",
            urlencoding::encode(query),
            limit,
            self.api_key,
        );
        let res = self.http
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        let status = res.status();
        if status.as_u16() == 429 {
            let retry = res.headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(60);
            return Err(ProviderError::RateLimited { retry_after_seconds: retry });
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::Auth);
        }
        if !status.is_success() {
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: res.text().await.unwrap_or_default(),
            });
        }
        let body: GoogleBooksResponse = res.json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(body.items
            .unwrap_or_default()
            .into_iter()
            .filter_map(map_google_books_hit)
            .collect())
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
    let isbn_13 = identifiers.iter()
        .find(|id| id.kind.as_deref() == Some("ISBN_13"))
        .and_then(|id| id.identifier.clone());
    let isbn_10 = identifiers.iter()
        .find(|id| id.kind.as_deref() == Some("ISBN_10"))
        .and_then(|id| id.identifier.clone());
    let isbn = isbn_10.or_else(|| isbn_13.clone());
    let cover_url = info.image_links
        .and_then(|i| i.thumbnail.or(i.small_thumbnail))
        .map(|s| s.replace("http://", "https://"));
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
}