use crate::{atproto::{AtprotoClientMetadata, OAuthResolverConfig, Scope, KnownScope, AuthMethod, GrantType}, error::AtprotoError};
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
        return Err(AtprotoError::metadata("client_id does not match the fetched URL"));
    }
    if !doc.client_id.starts_with(&format!("{}/", crate::atproto::CLIENT_ORIGIN)) {
        return Err(AtprotoError::metadata("client_id is not under the Livtet origin"));
    }
    if doc.application_type.as_deref() != Some("native") {
        return Err(AtprotoError::metadata("client must be a native application"));
    }
    if !doc.redirect_uris.contains(&crate::atproto::REDIRECT_URI.to_string()) {
        return Err(AtprotoError::metadata("redirect_uris must include the custom scheme"));
    }
    if !doc.grant_types.iter().any(|g| g == "authorization_code")
        || !doc.grant_types.iter().any(|g| g == "refresh_token")
    {
        return Err(AtprotoError::metadata("grant_types must include authorization_code and refresh_token"));
    }
    if doc.token_endpoint_auth_method != "none" {
        return Err(AtprotoError::metadata("token_endpoint_auth_method must be none"));
    }
    if !doc.dpop_bound_access_tokens {
        return Err(AtprotoError::metadata("dpop_bound_access_tokens must be true"));
    }
    let has_atproto = doc.scope.split_whitespace().any(|s| s == "atproto");
    if !has_atproto {
        return Err(AtprotoError::metadata("scope must include the atproto scope"));
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
        return Err(AtprotoError::metadata("client metadata endpoint returned non-2xx"));
    }
    let doc: ClientMetadataDocument = resp
        .json()
        .await
        .map_err(|e| AtprotoError::metadata(e.to_string()))?;

    let client_id = doc.client_id.clone();
    if !client_id.starts_with(&format!("{}/", crate::atproto::CLIENT_ORIGIN)) {
        return Err(AtprotoError::metadata("client_id is not under the Livtet origin"));
    }

    if client_id == crate::atproto::BOOTSTRAP_METADATA_URL {
        parse_client_metadata(doc, &client_id)
    } else {
        let resp2 = http
            .get(client_id)
            .send()
            .await
            .map_err(|_| AtprotoError::metadata("failed to fetch client metadata at client_id"))?;
        if !resp2.status().is_success() {
            return Err(AtprotoError::metadata("client metadata at client_id returned non-2xx"));
        }
        let doc2: ClientMetadataDocument = resp2
            .json()
            .await
            .map_err(|e| AtprotoError::metadata(e.to_string()))?;
        parse_client_metadata(doc2, &client_id)
    }
}
