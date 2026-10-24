use crate::atproto::{
    APPVIEW_PROFILE_NSID, APPVIEW_URL, CLIENT_ORIGIN, DOH_URL, REDIRECT_URI, client_metadata,
    store::{AtprotoSessionStore, AtprotoStateStore, KeyValueBackend, KeyringBackend},
    types::{AtprotoLoginStart, AtprotoProfileView, AtprotoStatus},
};
use crate::error::AtprotoError;
use atrium_api::agent::{Configure, SessionManager};
use atrium_api::types::string::Did;
use atrium_common::resolver::Resolver;
use atrium_common::store::Store;
use atrium_identity::did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL};
use atrium_identity::handle::{
    AtprotoHandleResolver, AtprotoHandleResolverConfig, DohDnsTxtResolver, DohDnsTxtResolverConfig,
};
use atrium_oauth::{AtprotoClientMetadata, OAuthClient, OAuthClientConfig, OAuthResolverConfig};
use atrium_xrpc::http::Method;
use atrium_xrpc::{XrpcClient, XrpcRequest};
use atrium_xrpc_client::reqwest::ReqwestClient;
use std::sync::Arc;
use tokio::sync::OnceCell;

type AtprotoHttp = ReqwestClient;
type AtprotoDidResolver = CommonDidResolver<AtprotoHttp>;
type AtprotoHandleResolverType = AtprotoHandleResolver<DohDnsTxtResolver<AtprotoHttp>, AtprotoHttp>;
type AtprotoOAuthClient = OAuthClient<
    AtprotoStateStore,
    AtprotoSessionStore,
    AtprotoDidResolver,
    AtprotoHandleResolverType,
    AtprotoHttp,
>;
type AtprotoSession = atrium_oauth::OAuthSession<
    AtprotoHttp,
    AtprotoDidResolver,
    AtprotoHandleResolverType,
    AtprotoSessionStore,
>;

struct ClientBundle {
    client: Arc<AtprotoOAuthClient>,
    metadata: AtprotoClientMetadata,
    handle_resolver: Arc<AtprotoHandleResolverType>,
}

pub(crate) struct AtprotoManager {
    http: reqwest::Client,
    backend: Arc<dyn KeyValueBackend>,
    state_store: AtprotoStateStore,
    session_store: AtprotoSessionStore,
    client: OnceCell<Arc<ClientBundle>>,
}

impl AtprotoManager {
    pub(crate) fn new(http: reqwest::Client) -> Self {
        let backend = Arc::new(KeyringBackend);
        Self {
            http,
            backend: backend.clone(),
            state_store: AtprotoStateStore::new(backend.clone()),
            session_store: AtprotoSessionStore::new(backend.clone()),
            client: OnceCell::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_backend(
        http: reqwest::Client,
        backend: Arc<dyn KeyValueBackend>,
    ) -> Self {
        Self {
            http,
            backend: backend.clone(),
            state_store: AtprotoStateStore::new(backend.clone()),
            session_store: AtprotoSessionStore::new(backend.clone()),
            client: OnceCell::new(),
        }
    }

    async fn build_client(&self) -> Result<Arc<ClientBundle>, AtprotoError> {
        let http = Arc::new(
            atrium_xrpc_client::reqwest::ReqwestClientBuilder::new(CLIENT_ORIGIN)
                .client(self.http.clone())
                .build(),
        );

        let metadata = client_metadata::fetch_client_metadata(&self.http).await?;

        let make_handle_resolver = |http: &Arc<AtprotoHttp>| {
            AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
                dns_txt_resolver: DohDnsTxtResolver::new(DohDnsTxtResolverConfig {
                    service_url: DOH_URL.to_string(),
                    http_client: Arc::clone(http),
                }),
                http_client: Arc::clone(http),
            })
        };

        let did_resolver = CommonDidResolver::new(CommonDidResolverConfig {
            plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
            http_client: Arc::clone(&http),
        });
        let handle_resolver = make_handle_resolver(&http);
        let handle_resolver_for_config = make_handle_resolver(&http);

        let config = OAuthClientConfig {
            client_metadata: metadata.clone(),
            keys: None,
            state_store: self.state_store.clone(),
            session_store: self.session_store.clone(),
            resolver: OAuthResolverConfig {
                did_resolver,
                handle_resolver: handle_resolver_for_config,
                authorization_server_metadata: Default::default(),
                protected_resource_metadata: Default::default(),
            },
            http_client: (*http).clone(),
        };

        let client = AtprotoOAuthClient::new(config)
            .map_err(|error| AtprotoError::metadata(error.to_string()))?;

        let bundle = Arc::new(ClientBundle {
            client: Arc::new(client),
            metadata,
            handle_resolver: Arc::new(handle_resolver),
        });

        Ok(bundle)
    }

    async fn client(&self) -> Result<Arc<ClientBundle>, AtprotoError> {
        self.client
            .get_or_try_init(|| async { self.build_client().await })
            .await
            .cloned()
    }

    pub(crate) async fn begin_login(&self, input: &str) -> Result<AtprotoLoginStart, AtprotoError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(AtprotoError::authorization("empty handle or did"));
        }

        self.state_store.clear().await?;

        let bundle = self.client().await?;
        let expected_did = if input.starts_with("did:") {
            input
                .parse::<Did>()
                .map_err(|e| AtprotoError::authorization(e.to_string()))?
        } else {
            let handle = input
                .parse::<atrium_api::types::string::Handle>()
                .map_err(|e| AtprotoError::authorization(e.to_string()))?;
            bundle
                .handle_resolver
                .resolve(&handle)
                .await
                .map_err(|e| AtprotoError::authorization(e.to_string()))?
        };

        let url = tokio::task::spawn({
            let client = bundle.client.clone();
            let input = input.to_string();
            let options = atrium_oauth::AuthorizeOptions {
                redirect_uri: Some(REDIRECT_URI.to_string()),
                scopes: bundle.metadata.scopes.clone(),
                prompt: None,
                state: Some(expected_did.to_string()),
            };
            async move { client.authorize(&input, options).await }
        })
        .await
        .map_err(|e| AtprotoError::authorization(format!("sign-in start failed: {e}")))?
        .map_err(|e| AtprotoError::authorization(e.to_string()))?;

        Ok(AtprotoLoginStart { url })
    }

    pub(crate) async fn complete_login(
        &self,
        callback_url: &str,
    ) -> Result<AtprotoStatus, AtprotoError> {
        let params = parse_callback_url(callback_url)?;
        let bundle = self.client().await?;

        let (session, app_state) = tokio::task::spawn({
            let client = bundle.client.clone();
            let params = params.clone();
            async move { client.callback(params).await }
        })
        .await
        .map_err(|e| AtprotoError::authorization(format!("token exchange failed: {e}")))?
        .map_err(|e| AtprotoError::authorization(e.to_string()))?;

        let expected_did = app_state
            .or(params.state)
            .ok_or_else(|| AtprotoError::authorization("missing app state"))?;
        let did = session
            .did()
            .await
            .ok_or_else(|| AtprotoError::authorization("missing account DID"))?;

        if did.as_str() != expected_did {
            let _ = bundle.client.revoke(&did).await;
            self.session_store.del(&did).await?;
            return Err(AtprotoError::authorization(
                "account did not match the requested identity",
            ));
        }

        let stored = self
            .session_store
            .get(&did)
            .await?
            .ok_or_else(|| AtprotoError::authorization("session was not stored"))?;

        let has_atproto_scope = stored
            .token_set
            .scope
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .any(|scope| scope == "atproto");
        if !has_atproto_scope {
            let _ = bundle.client.revoke(&did).await;
            self.session_store.del(&did).await?;
            return Err(AtprotoError::authorization(
                "authorization did not include the atproto scope",
            ));
        }

        if let Some(current) = crate::atproto::store::get_current_did(self.backend.clone()).await?
            && current != did.to_string()
            && let Some(old_did) = current.parse::<Did>().ok()
        {
            if let Some(old_client) = self.client.get() {
                let _ = old_client.client.revoke(&old_did).await;
            }
            self.session_store.del(&old_did).await?;
        }

        crate::atproto::store::set_current_did(self.backend.clone(), did.as_str()).await?;
        Ok(status_from_session(did.as_str(), &stored))
    }

    pub(crate) async fn status(&self) -> Result<Option<AtprotoStatus>, AtprotoError> {
        let Some(current) = crate::atproto::store::get_current_did(self.backend.clone()).await?
        else {
            return Ok(None);
        };

        let did = current.parse::<Did>().map_err(|_| AtprotoError::Keyring {
            message: "invalid stored DID".to_string(),
        })?;
        let Some(stored) = self.session_store.get(&did).await? else {
            crate::atproto::store::clear_current_did(self.backend.clone()).await?;
            return Ok(None);
        };

        Ok(Some(status_from_session(&current, &stored)))
    }

    pub(crate) async fn logout(&self) -> Result<(), AtprotoError> {
        let current = crate::atproto::store::get_current_did(self.backend.clone()).await?;
        if let Some(did_str) = current
            && let Ok(did) = did_str.parse::<Did>()
        {
            if let Some(bundle) = self.client.get() {
                let _ = bundle.client.revoke(&did).await;
            }
            self.session_store.del(&did).await?;
        }
        crate::atproto::store::clear_current_did(self.backend.clone()).await?;
        self.state_store.clear().await?;
        Ok(())
    }

    async fn restore_session(&self, did: &Did) -> Result<AtprotoSession, AtprotoError> {
        let bundle = self.client().await?;
        let session = bundle
            .client
            .restore(did)
            .await
            .map_err(|e| AtprotoError::authorization(e.to_string()))?;
        Ok(session)
    }

    async fn appview_session(&self, did: &Did) -> Result<AtprotoSession, AtprotoError> {
        let session = self.restore_session(did).await?;
        session.configure_endpoint(APPVIEW_URL.to_string());
        Ok(session)
    }

    pub(crate) async fn get_current_did(&self) -> Result<String, AtprotoError> {
        crate::atproto::store::get_current_did(self.backend.clone())
            .await?
            .ok_or(AtprotoError::NotSignedIn)
    }

    pub(crate) async fn get_profile(&self) -> Result<Option<AtprotoProfileView>, AtprotoError> {
        let did_str = self.get_current_did().await?;
        let did = did_str.parse::<Did>().map_err(|e| AtprotoError::Keyring {
            message: e.to_string(),
        })?;

        self.ensure_fresh(&did).await?;

        let session = self.appview_session(&did).await?;

        let request = appview_profile_request(did.as_str());

        let output: atrium_xrpc::OutputDataOrBytes<AppViewProfileResponse> = session
            .send_xrpc::<GetActorProfileParams, (), AppViewProfileResponse, serde_json::Value>(
                &request,
            )
            .await
            .map_err(|error| AtprotoError::appview(error.to_string()))?;

        match output {
            atrium_xrpc::OutputDataOrBytes::Data(data) => {
                Ok(map_profile_response(did.as_str(), data))
            }
            atrium_xrpc::OutputDataOrBytes::Bytes(_) => {
                Err(AtprotoError::appview("unexpected binary appview response"))
            }
        }
    }

    async fn ensure_fresh(&self, did: &Did) -> Result<(), AtprotoError> {
        let Some(stored) = self.session_store.get(did).await? else {
            return Err(AtprotoError::NotSignedIn);
        };

        if let Some(expires_at) = &stored.token_set.expires_at
            && expires_at > &atrium_api::types::string::Datetime::now()
        {
            return Ok(());
        }

        let session = self.restore_session(did).await?;
        let _: atrium_xrpc::OutputDataOrBytes<serde_json::Value> = session
            .send_xrpc::<(), (), serde_json::Value, serde_json::Value>(&XrpcRequest {
                method: Method::GET,
                nsid: "com.atproto.server.getSession".to_string(),
                parameters: None,
                input: None,
                encoding: None,
            })
            .await
            .map_err(|e| AtprotoError::authorization(e.to_string()))?;

        Ok(())
    }
}

#[derive(serde::Serialize)]
struct GetActorProfileParams {
    did: String,
}

#[derive(serde::Deserialize)]
struct AppViewProfileResponse {
    profile: Option<AppViewProfileRecord>,
    handle: Option<String>,
}

#[derive(serde::Deserialize)]
struct AppViewProfileRecord {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    description: Option<String>,
    #[serde(rename = "avatarCid")]
    avatar_cid: Option<String>,
}

fn status_from_session(
    did: &str,
    session: &atrium_oauth::store::session::Session,
) -> AtprotoStatus {
    AtprotoStatus {
        did: did.to_string(),
        scope: session.token_set.scope.clone(),
        expires_at: session
            .token_set
            .expires_at
            .as_ref()
            .map(|d| d.as_str().to_string()),
    }
}

/// The XRPC request for the appview's actor-profile query.
fn appview_profile_request(did: &str) -> XrpcRequest<GetActorProfileParams, ()> {
    XrpcRequest {
        method: Method::GET,
        nsid: APPVIEW_PROFILE_NSID.to_string(),
        parameters: Some(GetActorProfileParams {
            did: did.to_string(),
        }),
        input: None::<atrium_xrpc::InputDataOrBytes<()>>,
        encoding: None,
    }
}

fn map_profile_response(did: &str, response: AppViewProfileResponse) -> Option<AtprotoProfileView> {
    response.profile.map(|p| AtprotoProfileView {
        did: did.to_string(),
        handle: response.handle,
        display_name: p.display_name,
        description: p.description,
        avatar_cid: p.avatar_cid,
    })
}

fn parse_callback_url(raw: &str) -> Result<atrium_oauth::CallbackParams, AtprotoError> {
    let url =
        url::Url::parse(raw).map_err(|_| AtprotoError::authorization("invalid callback URL"))?;

    if url.scheme() != "net.olamaelcu.livtet" {
        return Err(AtprotoError::authorization("foreign scheme in callback"));
    }

    let mut params = atrium_oauth::CallbackParams {
        code: String::new(),
        state: None,
        iss: None,
    };
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => params.code = value.to_string(),
            "state" => params.state = Some(value.to_string()),
            "iss" => params.iss = Some(value.to_string()),
            "error" => {
                let desc = url
                    .query_pairs()
                    .find(|(k, _)| k == "error_description")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default();
                return Err(AtprotoError::authorization(format!("{}: {}", value, desc)));
            }
            _ => {}
        }
    }

    if params.code.is_empty() {
        return Err(AtprotoError::authorization("missing code in callback"));
    }
    Ok(params)
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::atproto::store::{
        AtprotoSessionStore, MemoryBackend, get_current_did, set_current_did,
    };
    use atrium_common::store::Store;
    use atrium_oauth::store::session::Session;

    const DID_A: &str = "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa";

    fn backend() -> Arc<MemoryBackend> {
        Arc::new(MemoryBackend::new())
    }

    fn did(value: &str) -> Did {
        value.parse().expect("did")
    }

    fn jwk() -> serde_json::Value {
        let coord = "A".repeat(43);
        serde_json::json!({
            "kty": "EC",
            "crv": "P-256",
            "x": coord,
            "y": coord,
            "d": coord,
        })
    }

    fn session_fixture(sub: &str, scope: Option<&str>, expires_at: Option<&str>) -> Session {
        serde_json::from_value(serde_json::json!({
            "dpop_key": jwk(),
            "token_set": {
                "iss": "https://pds.example",
                "sub": sub,
                "aud": "https://pds.example",
                "scope": scope,
                "refresh_token": "refresh-token",
                "access_token": "access-token",
                "token_type": "DPoP",
                "expires_at": expires_at,
            },
        }))
        .expect("session fixture")
    }

    fn manager(backend: Arc<MemoryBackend>) -> AtprotoManager {
        AtprotoManager::new_with_backend(reqwest::Client::new(), backend)
    }

    #[tokio::test]
    async fn status_without_current_is_none() {
        let backend: Arc<dyn KeyValueBackend> = backend();
        let manager = AtprotoManager::new_with_backend(reqwest::Client::new(), backend);

        assert!(manager.status().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn status_clears_current_when_the_session_is_missing() {
        let backend: Arc<MemoryBackend> = backend();
        let manager = manager(backend.clone());
        set_current_did(backend.clone(), DID_A).await.unwrap();

        assert!(manager.status().await.unwrap().is_none());
        assert!(
            get_current_did(backend).await.unwrap().is_none(),
            "a dangling current DID must be cleared"
        );
    }

    #[tokio::test]
    async fn status_reports_stored_did_scope_and_expiry() {
        let backend: Arc<MemoryBackend> = backend();
        let store = AtprotoSessionStore::new(backend.clone());
        store
            .set(
                did(DID_A),
                session_fixture(
                    DID_A,
                    Some("atproto repo:x"),
                    Some("2026-01-02T03:04:05.000Z"),
                ),
            )
            .await
            .unwrap();
        set_current_did(backend.clone(), DID_A).await.unwrap();

        let status = manager(backend).status().await.unwrap().expect("a status");
        assert_eq!(status.did, DID_A);
        assert_eq!(status.scope.as_deref(), Some("atproto repo:x"));
        assert_eq!(
            status.expires_at.as_deref(),
            Some("2026-01-02T03:04:05.000Z")
        );
    }

    #[tokio::test]
    async fn logout_without_current_is_ok() {
        let backend: Arc<MemoryBackend> = backend();
        manager(backend).logout().await.unwrap();
    }

    #[test]
    fn parse_callback_accepts_one_and_two_slash_forms() {
        for raw in [
            "net.olamaelcu.livtet:/oauth-redirect?code=abc&state=xyz&iss=https%3A%2F%2Fpds.example",
            "net.olamaelcu.livtet://oauth-redirect?code=abc&state=xyz&iss=https%3A%2F%2Fpds.example",
        ] {
            let params = parse_callback_url(raw).expect("parses");
            assert_eq!(params.code, "abc");
            assert_eq!(params.state.as_deref(), Some("xyz"));
            assert_eq!(params.iss.as_deref(), Some("https://pds.example"));
        }
    }

    #[test]
    fn parse_callback_surfaces_access_denied() {
        let error = parse_callback_url(
            "net.olamaelcu.livtet:/oauth-redirect?error=access_denied&error_description=denied",
        )
        .expect_err("denied");
        let rendered = error.to_string();
        assert!(rendered.contains("access_denied"));
        assert!(rendered.contains("denied"));
    }

    #[test]
    fn parse_callback_rejects_foreign_scheme() {
        let error = parse_callback_url("https://evil.example/oauth-redirect?code=abc")
            .expect_err("foreign scheme");
        assert!(error.to_string().contains("foreign scheme"));
    }

    #[test]
    fn status_from_session_maps_scope_and_expiry() {
        let session = session_fixture(DID_A, Some("atproto"), Some("2026-05-06T07:08:09.000Z"));
        let status = status_from_session(DID_A, &session);

        assert_eq!(status.did, DID_A);
        assert_eq!(status.scope.as_deref(), Some("atproto"));
        assert_eq!(
            status.expires_at.as_deref(),
            Some("2026-05-06T07:08:09.000Z")
        );

        let bare = status_from_session(DID_A, &session_fixture(DID_A, None, None));
        assert!(bare.scope.is_none());
        assert!(bare.expires_at.is_none());
    }

    #[test]
    fn map_profile_response_returns_none_for_null() {
        let response: AppViewProfileResponse =
            serde_json::from_value(serde_json::json!({ "profile": null })).expect("response");
        assert!(map_profile_response(DID_A, response).is_none());
    }

    #[test]
    fn map_profile_response_maps_display_name_description_and_avatar() {
        let response: AppViewProfileResponse = serde_json::from_value(serde_json::json!({
            "profile": {
                "displayName": "Reader",
                "description": "Reads books",
                "avatarCid": "bafyavatar",
            },
            "handle": "reader.example",
        }))
        .expect("response");

        let view = map_profile_response(DID_A, response).expect("a profile view");
        assert_eq!(view.did, DID_A);
        assert_eq!(view.handle.as_deref(), Some("reader.example"));
        assert_eq!(view.display_name.as_deref(), Some("Reader"));
        assert_eq!(view.description.as_deref(), Some("Reads books"));
        assert_eq!(view.avatar_cid.as_deref(), Some("bafyavatar"));
    }

    #[test]
    fn appview_profile_request_targets_get_actor_profile() {
        let request = appview_profile_request(DID_A);

        assert_eq!(request.nsid, APPVIEW_PROFILE_NSID);
        assert_eq!(request.method, Method::GET);
        let params = request.parameters.expect("parameters");
        assert_eq!(params.did, DID_A);
    }
}
