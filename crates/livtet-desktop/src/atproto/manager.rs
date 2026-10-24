use crate::atproto::{
    types::{AtprotoLoginStart, AtprotoProfileView, AtprotoStatus},
    store::{AtprotoStateStore, AtprotoSessionStore, KeyringBackend, KeyValueBackend, AtprotoStoreError},
    client_metadata,
    CLIENT_ORIGIN, BOOTSTRAP_METADATA_URL, REDIRECT_URI, APPVIEW_URL, APPVIEW_PROFILE_NSID, DOH_URL,
};
use crate::error::AtprotoError;
use atrium_identity::did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL};
use atrium_identity::handle::{AtprotoHandleResolver, AtprotoHandleResolverConfig, DohDnsTxtResolver, DohDnsTxtResolverConfig};
use atrium_oauth::{
    AtprotoClientMetadata, AuthMethod, GrantType, KnownScope, OAuthClient, OAuthClientConfig, OAuthResolverConfig, Scope,
};
use atrium_xrpc::http::Method;
use atrium_xrpc::XrpcRequest;
use atrium_xrpc_client::reqwest::ReqwestClient;
use atrium_api::agent::SessionManager;
use atrium_api::types::string::Did;
use atrium_common::resolver::Resolver;
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

    pub(crate) fn new_with_backend(http: reqwest::Client, backend: Arc<dyn KeyValueBackend>) -> Self {
        Self {
            http,
            backend: backend.clone(),
            state_store: AtprotoStateStore::new(backend.clone()),
            session_store: AtprotoSessionStore::new(backend.clone()),
            client: OnceCell::new(),
        }
    }

    async fn build_client(&self) -> Result<Arc<ClientBundle>, AtprotoError> {
        let http = Arc::new(ReqwestClient
            .client(self.http.clone())
            .build());

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
            .get_or_try_init(|| async { self.build_client().await.map(Arc::new) })
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
            input.parse::<Did>().map_err(|e| AtprotoError::authorization(e.to_string()))?
        } else {
            let handle = input.parse::<atrium_api::types::string::Handle>()
                .map_err(|e| AtprotoError::authorization(e.to_string()))?;
            bundle.handle_resolver.resolve(&handle).await
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
            async move {
                client.authorize(&input, options).await
            }
        })
        .await
        .map_err(|e| AtprotoError::authorization(format!("sign-in start failed: {e}")))?
        .map_err(|e| AtprotoError::authorization(e.to_string()))?;

        Ok(AtprotoLoginStart { url })
    }

    pub(crate) async fn complete_login(&self, callback_url: &str) -> Result<AtprotoStatus, AtprotoError> {
        let params = parse_callback_url(callback_url)?;
        let bundle = self.client().await?;

        let session = tokio::task::spawn({
            let client = bundle.client.clone();
            let params = params.clone();
            async move {
                client.callback(params).await
            }
        })
        .await
        .map_err(|e| AtprotoError::authorization(format!("token exchange failed: {e}")))?
        .map_err(|e| AtprotoError::authorization(e.to_string()))?;

        let expected_did = params.state.ok_or_else(|| AtprotoError::authorization("missing app state"))?;
        let did = session.did().await.ok_or_else(|| AtprotoError::authorization("missing account DID"))?;

        if did.as_str() != expected_did {
            let _ = bundle.client.revoke(&did).await;
            self.session_store.delete(&did.to_string()).await?;
            return Err(AtprotoError::authorization("account did not match the requested identity"));
        }

        let stored = self.session_store.get(&did.to_string()).await?
            .ok_or_else(|| AtprotoError::authorization("session was not stored"))?;

        let has_atproto_scope = stored.token_set.scope
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .any(|scope| scope == "atproto");
        if !has_atproto_scope {
            let _ = bundle.client.revoke(&did).await;
            self.session_store.delete(&did.to_string()).await?;
            return Err(AtprotoError::authorization("authorization did not include the atproto scope"));
        }

        if let Some(current) = crate::atproto::store::get_current_did(self.backend.clone()).await? {
            if current != did.to_string() {
                if let Some(old_client) = self.client.get() {
                    let _ = old_client.client.revoke(&current.parse().ok().into_iter().next().unwrap()).await;
                }
                self.session_store.delete(&current).await?;
            }
        }

        crate::atproto::store::set_current_did(self.backend.clone(), &did.to_string()).await?;
        Ok(status_from_session(&did.to_string(), &stored))
    }

    pub(crate) async fn status(&self) -> Result<Option<AtprotoStatus>, AtprotoError> {
        let Some(current) = crate::atproto::store::get_current_did(self.backend.clone()).await? else {
            return Ok(None);
        };

        let did = current.parse::<Did>().map_err(|_| AtprotoError::Keyring("invalid stored DID".to_string()))?;
        let Some(stored) = self.session_store.get(&current).await? else {
            crate::atproto::store::clear_current_did(self.backend.clone()).await?;
            return Ok(None);
        };

        Ok(Some(status_from_session(&current, &stored)))
    }

    pub(crate) async fn logout(&self) -> Result<(), AtprotoError> {
        let current = crate::atproto::store::get_current_did(self.backend.clone()).await?;
        if let Some(did_str) = current {
            if let Some(bundle) = self.client.get() {
                if let Ok(did) = did_str.parse::<Did>() {
                    let _ = bundle.client.revoke(&did).await;
                }
            }
            self.session_store.delete(&did_str).await?;
        }
        crate::atproto::store::clear_current_did(self.backend.clone()).await?;
        self.state_store.clear().await?;
        Ok(())
    }

    async fn restore_session(&self, did: &Did) -> Result<AtprotoSession, AtprotoError> {
        let bundle = self.client().await?;
        let session = bundle.client.restore(did).await
            .map_err(|e| AtprotoError::authorization(e.to_string()))?;
        Ok(session)
    }

    async fn appview_session(&self, did: &Did) -> Result<AtprotoSession, AtprotoError> {
        let session = self.restore_session(did).await?;
        session.configure_endpoint(APPVIEW_URL.to_string());
        Ok(session)
    }

    pub(crate) async fn get_current_did(&self) -> Result<String, AtprotoError> {
        crate::atproto::store::get_current_did(self.backend.clone()).await
    }

    pub(crate) async fn get_profile(&self) -> Result<Option<AtprotoProfileView>, AtprotoError> {
        let did_str = self.get_current_did().await?;
        let did = did_str.parse::<Did>().map_err(|e| AtprotoError::Keyring(e.to_string()))?;

        self.ensure_fresh(&did).await?;

        let session = self.appview_session(&did).await?;

        let request = XrpcRequest {
            method: Method::GET,
            nsid: APPVIEW_PROFILE_NSID.to_string(),
            parameters: Some(GetActorProfileParams { did: did.to_string() }),
            input: None::<atrium_xrpc::InputDataOrBytes<()>>,
            encoding: None,
        };

        let output: atrium_xrpc::OutputDataOrBytes<AppViewProfileResponse> = session.send_xrpc(&request).await
            .map_err(|error| AtprotoError::appview(error.to_string()))?;

        match output {
            atrium_xrpc::OutputDataOrBytes::Data(data) => Ok(map_profile_response(&did.to_string(), data)),
            atrium_xrpc::OutputDataOrBytes::Bytes(_) => Err(AtprotoError::appview("unexpected binary appview response")),
        }
    }

    async fn ensure_fresh(&self, did: &Did) -> Result<(), AtprotoError> {
        let Some(stored) = self.session_store.get(&did.to_string()).await? else {
            return Err(AtprotoError::NotSignedIn);
        };

        if let Some(expires_at) = &stored.token_set.expires_at {
            if expires_at > &atrium_api::types::string::Datetime::now() {
                return Ok(());
            }
        }

        let session = self.restore_session(did).await?;
        let _: atrium_xrpc::OutputDataOrBytes<atrium_api::types::Object> = session.send_xrpc(&XrpcRequest {
            method: Method::GET,
            nsid: "com.atproto.server.getSession".to_string(),
            parameters: None,
            input: None,
            encoding: None,
        }).await.map_err(|e| AtprotoError::authorization(e.to_string()))?;

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

fn status_from_session(did: &str, session: &atrium_oauth::store::session::Session) -> AtprotoStatus {
    AtprotoStatus {
        did: did.to_string(),
        scope: session.token_set.scope.clone(),
        expires_at: session.token_set.expires_at.as_ref().map(|d| format!("{}", d)),
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
    let url = url::Url::parse(raw)
        .map_err(|_| AtprotoError::authorization("invalid callback URL"))?;

    if url.scheme() != "net.olamaelcu.livtet" {
        return Err(AtprotoError::authorization("foreign scheme in callback"));
    }

    let mut params = atrium_oauth::CallbackParams { code: Some(String::new()), state: Some(String::new()), iss: Some(String::new()) };
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => params.code = Some(value.to_string()),
            "state" => params.state = Some(value.to_string()),
            "iss" => params.iss = Some(value.to_string()),
            "error" => {
                let desc = url.query_pairs()
                    .find(|(k, _)| k == "error_description")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default();
                return Err(AtprotoError::authorization(format!("{}: {}", value, desc)));
            }
            _ => {}
        }
    }

    params.code.ok_or_else(|| AtprotoError::authorization("missing code in callback"))?;
    Ok(params)
}