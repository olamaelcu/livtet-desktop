use crate::error::AtprotoError;
use atrium_oauth::{AtprotoClientMetadata, AuthMethod, GrantType, KnownScope, Scope};
use serde::Deserialize;
use specta::Type;

#[derive(Debug, Clone, Deserialize, Type)]
pub struct ClientMetadataDocument {
    pub client_id: String,
    pub client_uri: Option<String>,
    pub redirect_uris: Vec<String>,
    pub scope: String,
    pub grant_types: Vec<String>,
    pub application_type: Option<String>,
    pub token_endpoint_auth_method: String,
    pub dpop_bound_access_tokens: bool,
}

pub(crate) fn parse_client_metadata(
    doc: ClientMetadataDocument,
    fetched_from: &str,
) -> Result<AtprotoClientMetadata, AtprotoError> {
    if doc.client_id != fetched_from {
        return Err(AtprotoError::metadata(
            "client_id does not match the fetched URL",
        ));
    }
    if !doc
        .client_id
        .starts_with(&format!("{}/", crate::atproto::CLIENT_ORIGIN))
    {
        return Err(AtprotoError::metadata(
            "client_id is not under the Livtet origin",
        ));
    }
    if doc.application_type.as_deref() != Some("native") {
        return Err(AtprotoError::metadata(
            "client must be a native application",
        ));
    }
    if !doc
        .redirect_uris
        .contains(&crate::atproto::REDIRECT_URI.to_string())
    {
        return Err(AtprotoError::metadata(
            "redirect_uris must include the custom scheme",
        ));
    }
    if !doc.grant_types.iter().any(|g| g == "authorization_code")
        || !doc.grant_types.iter().any(|g| g == "refresh_token")
    {
        return Err(AtprotoError::metadata(
            "grant_types must include authorization_code and refresh_token",
        ));
    }
    if doc.token_endpoint_auth_method != "none" {
        return Err(AtprotoError::metadata(
            "token_endpoint_auth_method must be none",
        ));
    }
    if !doc.dpop_bound_access_tokens {
        return Err(AtprotoError::metadata(
            "dpop_bound_access_tokens must be true",
        ));
    }
    let has_atproto = doc.scope.split_whitespace().any(|s| s == "atproto");
    if !has_atproto {
        return Err(AtprotoError::metadata(
            "scope must include the atproto scope",
        ));
    }

    let scopes: Vec<Scope> = doc
        .scope
        .split_whitespace()
        .map(|s| {
            if s == "atproto" {
                Scope::Known(KnownScope::Atproto)
            } else {
                Scope::Unknown(s.to_string())
            }
        })
        .collect();

    Ok(AtprotoClientMetadata {
        client_id: doc.client_id,
        client_uri: doc.client_uri,
        redirect_uris: doc.redirect_uris,
        token_endpoint_auth_method: AuthMethod::None,
        grant_types: vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
        scopes,
        jwks_uri: None,
        token_endpoint_auth_signing_alg: None,
    })
}

pub(crate) async fn fetch_client_metadata(
    http: &reqwest::Client,
) -> Result<AtprotoClientMetadata, AtprotoError> {
    let resp = http
        .get(crate::atproto::BOOTSTRAP_METADATA_URL)
        .send()
        .await
        .map_err(|_| AtprotoError::metadata("failed to fetch client metadata"))?;
    if !resp.status().is_success() {
        return Err(AtprotoError::metadata(
            "client metadata endpoint returned non-2xx",
        ));
    }
    let doc: ClientMetadataDocument = resp
        .json()
        .await
        .map_err(|e| AtprotoError::metadata(e.to_string()))?;

    let client_id = doc.client_id.clone();
    if !client_id.starts_with(&format!("{}/", crate::atproto::CLIENT_ORIGIN)) {
        return Err(AtprotoError::metadata(
            "client_id is not under the Livtet origin",
        ));
    }

    if client_id == crate::atproto::BOOTSTRAP_METADATA_URL {
        parse_client_metadata(doc, &client_id)
    } else {
        let resp2 =
            http.get(&client_id).send().await.map_err(|_| {
                AtprotoError::metadata("failed to fetch client metadata at client_id")
            })?;
        if !resp2.status().is_success() {
            return Err(AtprotoError::metadata(
                "client metadata at client_id returned non-2xx",
            ));
        }
        let doc2: ClientMetadataDocument = resp2
            .json()
            .await
            .map_err(|e| AtprotoError::metadata(e.to_string()))?;
        parse_client_metadata(doc2, &client_id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const CLIENT_ID: &str = "https://livtet.olamaelcu.net/oauth-client-metadata.json";

    fn live_document() -> ClientMetadataDocument {
        serde_json::from_value(serde_json::json!({
            "client_id": CLIENT_ID,
            "client_name": "Livtet",
            "client_uri": "https://livtet.olamaelcu.net",
            "redirect_uris": [
                "https://livtet.olamaelcu.net/oauth/callback",
                "net.olamaelcu.livtet:/oauth-redirect"
            ],
            "scope": "atproto repo:net.olamaelcu.livtet.actor.profile \
                      repo:net.olamaelcu.livtet.biblio.readingGoal \
                      repo:net.olamaelcu.livtet.biblio.shelf \
                      repo:net.olamaelcu.livtet.biblio.bookShelving \
                      repo:net.olamaelcu.livtet.bookReview blob:image/*",
            "grant_types": ["authorization_code", "refresh_token"],
            "application_type": "native",
            "token_endpoint_auth_method": "none",
            "dpop_bound_access_tokens": true
        }))
        .expect("document")
    }

    #[test]
    fn parses_the_native_client_metadata() {
        let metadata = parse_client_metadata(live_document(), CLIENT_ID).expect("parses");

        assert_eq!(metadata.client_id, CLIENT_ID);
        assert_eq!(
            metadata.client_uri.as_deref(),
            Some("https://livtet.olamaelcu.net")
        );
        assert!(
            metadata
                .redirect_uris
                .iter()
                .any(|uri| uri == "net.olamaelcu.livtet:/oauth-redirect")
        );
        assert_eq!(metadata.scopes.len(), 7);
        assert!(matches!(
            metadata.scopes[0],
            Scope::Known(KnownScope::Atproto)
        ));
        assert_eq!(
            metadata.grant_types,
            vec![GrantType::AuthorizationCode, GrantType::RefreshToken]
        );
        assert_eq!(metadata.token_endpoint_auth_method, AuthMethod::None);
    }

    #[test]
    fn rejects_a_mismatched_client_id() {
        let error =
            parse_client_metadata(live_document(), "https://livtet.olamaelcu.net/other.json")
                .expect_err("client_id mismatch");
        assert!(error.to_string().contains("does not match"));
    }

    #[test]
    fn rejects_a_missing_custom_scheme_redirect() {
        let mut document = live_document();
        document
            .redirect_uris
            .retain(|uri| uri != "net.olamaelcu.livtet:/oauth-redirect");

        let error = parse_client_metadata(document, CLIENT_ID).expect_err("missing redirect");
        assert!(error.to_string().contains("redirect_uris"));
    }

    #[test]
    fn rejects_a_web_client() {
        let mut document = live_document();
        document.application_type = Some("web".to_string());

        let error = parse_client_metadata(document, CLIENT_ID).expect_err("web client");
        assert!(error.to_string().contains("native"));
    }

    #[test]
    fn rejects_missing_dpop_binding() {
        let mut document = live_document();
        document.dpop_bound_access_tokens = false;

        let error = parse_client_metadata(document, CLIENT_ID).expect_err("no dpop");
        assert!(error.to_string().contains("dpop_bound_access_tokens"));
    }

    #[test]
    fn rejects_a_scope_without_atproto() {
        let mut document = live_document();
        document.scope = "repo:net.olamaelcu.livtet.biblio.shelf".to_string();

        let error = parse_client_metadata(document, CLIENT_ID).expect_err("no atproto scope");
        assert!(error.to_string().contains("atproto"));
    }

    #[test]
    fn rejects_a_metadata_url_outside_the_livet_origin() {
        let mut document = live_document();
        document.client_id = "https://evil.example/oauth-client-metadata.json".to_string();

        let error =
            parse_client_metadata(document, "https://evil.example/oauth-client-metadata.json")
                .expect_err("foreign origin");
        assert!(error.to_string().contains("Livtet origin"));
    }
}
