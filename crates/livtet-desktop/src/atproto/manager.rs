use crate::{
    atproto::{
        APPVIEW_PROFILE_NSID, APPVIEW_URL, CLIENT_ORIGIN, DOH_URL, LOOPBACK_REDIRECT_URI,
        MAX_AVATAR_BYTES, PLATFORM_AVATAR_BASE, PROFILE_COLLECTION, PROFILE_RKEY, REDIRECT_URI,
        client_metadata,
        http_log::LoggingHttpClient,
        shelf_records::{
            BOOK_SHELVING_COLLECTION, SHELF_COLLECTION, book_shelving_record, derive_rkey,
            shelf_record, status_shelf_uri,
        },
        store::{AtprotoSessionStore, AtprotoStateStore, KeyValueBackend, KeyringBackend},
        types::{AtprotoLoginStart, AtprotoProfileView, AtprotoStatus},
    },
    error::AtprotoError,
};
use atrium_api::{
    agent::{Configure, SessionManager},
    types::string::Did,
};
use atrium_common::{resolver::Resolver, store::Store};
use atrium_identity::{
    did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL},
    handle::{
        AtprotoHandleResolver, AtprotoHandleResolverConfig, DohDnsTxtResolver,
        DohDnsTxtResolverConfig,
    },
};
use atrium_oauth::{AtprotoClientMetadata, OAuthClient, OAuthClientConfig, OAuthResolverConfig};
use atrium_xrpc::{XrpcClient, XrpcRequest, http::Method};
use livtet_core::shelf::ReadingStatus;
use std::sync::Arc;
use tokio::sync::OnceCell;

type AtprotoHttp = LoggingHttpClient;
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

/// The access token lifetime is server-controlled (PDS `expires_in`).
/// We cannot force a 1-week token from the desktop client; the realistic
/// upgrade is a robust refresh-token flow with proactive refresh.
/// Refresh proactively when the token is within this window of expiry.
const REFRESH_BEFORE_EXPIRY_SECS: u64 = 1800; // 30 minutes
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
        let http = Arc::new(LoggingHttpClient::new(CLIENT_ORIGIN, self.http.clone()));

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

    /// The loopback redirect the client document advertises, if any.
    ///
    /// `atrium-oauth` only accepts a redirect URI listed verbatim in the
    /// document, so the advertised string is used as-is.
    pub(crate) async fn loopback_redirect_uri(&self) -> Result<Option<String>, AtprotoError> {
        let bundle = self.client().await?;
        Ok(bundle
            .metadata
            .redirect_uris
            .iter()
            .find(|uri| uri.as_str() == LOOPBACK_REDIRECT_URI)
            .cloned())
    }

    pub(crate) async fn begin_login(
        &self,
        input: &str,
        redirect_uri: Option<&str>,
    ) -> Result<AtprotoLoginStart, AtprotoError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(AtprotoError::authorization("empty handle or did"));
        }

        tracing::info!(input = %input, "ATProto sign-in requested");
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

        let redirect_uri = redirect_uri.unwrap_or(REDIRECT_URI).to_string();
        tracing::info!(
            did = expected_did.as_str(),
            %redirect_uri,
            scopes = bundle.metadata.scopes.len(),
            "ATProto identity resolved; requesting authorization"
        );

        let url = tokio::task::spawn({
            let client = bundle.client.clone();
            let input = input.to_string();
            let redirect_uri = redirect_uri.clone();
            let options = atrium_oauth::AuthorizeOptions {
                redirect_uri: Some(redirect_uri),
                scopes: bundle.metadata.scopes.clone(),
                prompt: None,
                state: Some(expected_did.to_string()),
            };
            async move { client.authorize(&input, options).await }
        })
        .await
        .map_err(|e| AtprotoError::authorization(format!("sign-in start failed: {e}")))?
        .map_err(|e| {
            tracing::warn!(%e, "ATProto authorization request failed");
            AtprotoError::authorization(e.to_string())
        })?;

        tracing::info!(
            authorize_url_host = %url::Url::parse(&url).map(|u| u.host_str().unwrap_or_default().to_string()).unwrap_or_default(),
            "ATProto authorization URL ready"
        );
        Ok(AtprotoLoginStart { url })
    }

    pub(crate) async fn complete_login(
        &self,
        callback_url: &str,
    ) -> Result<AtprotoStatus, AtprotoError> {
        let params = parse_callback_url(callback_url)?;
        tracing::info!(
            has_state = params.state.is_some(),
            has_iss = params.iss.is_some(),
            "ATProto callback received; exchanging the authorization code"
        );
        let bundle = self.client().await?;

        let (session, app_state) = tokio::task::spawn({
            let client = bundle.client.clone();
            let params = params.clone();
            async move { client.callback(params).await }
        })
        .await
        .map_err(|e| {
            tracing::warn!(%e, "ATProto token exchange panicked");
            AtprotoError::authorization(format!("token exchange failed: {e}"))
        })?
        .map_err(|e| {
            tracing::warn!(%e, "ATProto token exchange failed");
            AtprotoError::authorization(e.to_string())
        })?;

        let expected_did = app_state
            .or(params.state)
            .ok_or_else(|| AtprotoError::authorization("missing app state"))?;
        let did = session
            .did()
            .await
            .ok_or_else(|| AtprotoError::authorization("missing account DID"))?;

        if did.as_str() != expected_did {
            tracing::warn!(
                expected = expected_did.as_str(),
                actual = did.as_str(),
                "ATProto account did not match the requested identity"
            );
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
            tracing::warn!(did = did.as_str(), scope = ?stored.token_set.scope, "ATProto authorization lacked the atproto scope");
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
        tracing::info!(
            did = did.as_str(),
            scope = ?stored.token_set.scope,
            expires_at = ?stored.token_set.expires_at.as_ref().map(|d| d.as_str()),
            "ATProto session stored"
        );
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
            tracing::info!(did = did.as_str(), "ATProto sign-out requested");
            if let Some(bundle) = self.client.get() {
                let _ = bundle.client.revoke(&did).await;
            }
            self.session_store.del(&did).await?;
        }
        crate::atproto::store::clear_current_did(self.backend.clone()).await?;
        self.state_store.clear().await?;
        Ok(())
    }

    /// Write the signed-in account's Livtet profile to its own PDS.
    ///
    /// The record lives at `net.olamaelcu.livtet.actor.profile/self`; the
    /// existing record is read first so `avatarCid` and `createdAt` survive an
    /// edit that only touches the text fields.
    pub(crate) async fn update_profile(
        &self,
        display_name: String,
        description: String,
    ) -> Result<AtprotoProfileView, AtprotoError> {
        let did_str = self.get_current_did().await?;
        let did = did_str.parse::<Did>().map_err(|e| AtprotoError::Keyring {
            message: e.to_string(),
        })?;
        self.ensure_fresh(&did).await?;
        let session = self.restore_session(&did).await?;

        let handle = read_handle(&session).await;
        let existing = self.read_profile_record(&session, &did).await?;
        let now = atrium_api::types::string::Datetime::now()
            .as_str()
            .to_string();
        let record = merge_profile_record(
            existing.as_ref(),
            Some(&display_name),
            Some(&description),
            None,
            false,
            &now,
        );
        tracing::info!(
            did = did.as_str(),
            collection = PROFILE_COLLECTION,
            "writing the ATProto profile record"
        );

        let request = put_profile_record_request(did.as_str(), &record);
        let output: atrium_xrpc::OutputDataOrBytes<PutProfileRecordOutput> = session
            .send_xrpc::<(), PutProfileRecordInput, PutProfileRecordOutput, serde_json::Value>(
                &request,
            )
            .await
            .map_err(|error| {
                tracing::warn!(%error, "ATProto profile write failed");
                AtprotoError::authorization(format!("profile write failed: {error}"))
            })?;

        match output {
            atrium_xrpc::OutputDataOrBytes::Data(written) => {
                tracing::info!(uri = %written.uri, cid = %written.cid, "ATProto profile written");
                Ok(profile_view_from_record(
                    did.as_str(),
                    &record,
                    handle,
                    Some(&session.base_uri()),
                ))
            }
            atrium_xrpc::OutputDataOrBytes::Bytes(_) => Err(AtprotoError::authorization(
                "unexpected binary response to putRecord",
            )),
        }
    }

    /// Upload an avatar image and point the profile record at its blob.
    pub(crate) async fn set_avatar(&self, path: &str) -> Result<AtprotoProfileView, AtprotoError> {
        let did_str = self.get_current_did().await?;
        let did = did_str.parse::<Did>().map_err(|e| AtprotoError::Keyring {
            message: e.to_string(),
        })?;
        self.ensure_fresh(&did).await?;
        let session = self.restore_session(&did).await?;

        let bytes = fs_err::tokio::read(path).await.map_err(|error| {
            AtprotoError::authorization(format!("could not read the avatar image: {error}"))
        })?;
        if bytes.len() > MAX_AVATAR_BYTES {
            return Err(AtprotoError::authorization(format!(
                "avatar image is larger than {MAX_AVATAR_BYTES} bytes"
            )));
        }
        let mime = image_mime(&bytes).ok_or_else(|| {
            AtprotoError::authorization("unsupported image type: use PNG, JPEG, WebP, or GIF")
        })?;

        let upload = upload_blob_request(bytes, mime);
        let output: atrium_xrpc::OutputDataOrBytes<UploadBlobOutput> = session
            .send_xrpc::<(), Vec<u8>, UploadBlobOutput, serde_json::Value>(&upload)
            .await
            .map_err(|error| {
                AtprotoError::authorization(format!("avatar upload failed: {error}"))
            })?;
        let cid = match output {
            atrium_xrpc::OutputDataOrBytes::Data(data) => data.blob.reference.link,
            atrium_xrpc::OutputDataOrBytes::Bytes(_) => {
                return Err(AtprotoError::authorization(
                    "unexpected binary response to uploadBlob",
                ));
            }
        };
        tracing::info!(did = did.as_str(), %cid, mime, "ATProto avatar uploaded");

        let handle = read_handle(&session).await;
        let existing = self.read_profile_record(&session, &did).await?;
        let now = atrium_api::types::string::Datetime::now()
            .as_str()
            .to_string();
        let record = merge_profile_record(existing.as_ref(), None, None, Some(&cid), false, &now);
        let request = put_profile_record_request(did.as_str(), &record);
        let output: atrium_xrpc::OutputDataOrBytes<PutProfileRecordOutput> = session
            .send_xrpc::<(), PutProfileRecordInput, PutProfileRecordOutput, serde_json::Value>(
                &request,
            )
            .await
            .map_err(|error| {
                tracing::warn!(%error, "ATProto avatar write failed");
                AtprotoError::authorization(format!("profile write failed: {error}"))
            })?;
        match output {
            atrium_xrpc::OutputDataOrBytes::Data(_) => Ok(profile_view_from_record(
                did.as_str(),
                &record,
                handle,
                Some(&session.base_uri()),
            )),
            atrium_xrpc::OutputDataOrBytes::Bytes(_) => Err(AtprotoError::authorization(
                "unexpected binary response to putRecord",
            )),
        }
    }

    /// Drop the Livtet avatar so the Bluesky avatar is used again.
    #[allow(dead_code)]
    pub(crate) async fn remove_avatar(&self) -> Result<AtprotoProfileView, AtprotoError> {
        let did_str = self.get_current_did().await?;
        let did = did_str.parse::<Did>().map_err(|e| AtprotoError::Keyring {
            message: e.to_string(),
        })?;
        self.ensure_fresh(&did).await?;
        let session = self.restore_session(&did).await?;

        let handle = read_handle(&session).await;
        let existing = self.read_profile_record(&session, &did).await?;
        let now = atrium_api::types::string::Datetime::now()
            .as_str()
            .to_string();
        let record = merge_profile_record(existing.as_ref(), None, None, None, true, &now);
        let request = put_profile_record_request(did.as_str(), &record);
        let output: atrium_xrpc::OutputDataOrBytes<PutProfileRecordOutput> = session
            .send_xrpc::<(), PutProfileRecordInput, PutProfileRecordOutput, serde_json::Value>(
                &request,
            )
            .await
            .map_err(|error| {
                AtprotoError::authorization(format!("profile write failed: {error}"))
            })?;
        match output {
            atrium_xrpc::OutputDataOrBytes::Data(_) => Ok(profile_view_from_record(
                did.as_str(),
                &record,
                handle,
                Some(&session.base_uri()),
            )),
            atrium_xrpc::OutputDataOrBytes::Bytes(_) => Err(AtprotoError::authorization(
                "unexpected binary response to putRecord",
            )),
        }
    }

    /// Shelve a book onto a reading status: write the shelf record and the
    /// bookShelving record to the account's own repository.
    pub(crate) async fn shelve_book(
        &self,
        book_uri: &str,
        index_title: &str,
        status: ReadingStatus,
    ) -> Result<ShelvedBook, AtprotoError> {
        let did_str = self.get_current_did().await?;
        let did = did_str.parse::<Did>().map_err(|e| AtprotoError::Keyring {
            message: e.to_string(),
        })?;
        self.ensure_fresh(&did).await?;
        let session = self.restore_session(&did).await?;
        let now = atrium_api::types::string::Datetime::now()
            .as_str()
            .to_string();

        let shelf_uri = status_shelf_uri(did.as_str(), status);
        let shelf_rkey = status.as_str().to_string();
        let shelf_body = shelf_record(status, &now);
        let request = put_record_request(
            did.as_str(),
            SHELF_COLLECTION,
            &shelf_rkey,
            &shelf_body,
            true,
        );
        let written_shelf: PutProfileRecordOutput =
            self.send_put_record(&session, &request, "shelf").await?;

        let book_rkey = derive_rkey(book_uri)
            .ok_or_else(|| {
                AtprotoError::authorization(format!(
                    "book reference is not an at:// record URI: {book_uri}"
                ))
            })?
            .to_string();
        let shelving_body = book_shelving_record(&shelf_uri, book_uri, index_title, status, &now);
        let request = put_record_request(
            did.as_str(),
            BOOK_SHELVING_COLLECTION,
            &book_rkey,
            &shelving_body,
            true,
        );
        let written: PutProfileRecordOutput = self
            .send_put_record(&session, &request, "bookShelving")
            .await?;

        Ok(ShelvedBook {
            record_uri: written.uri,
            shelf_uri: written_shelf.uri,
        })
    }

    async fn send_put_record(
        &self,
        session: &AtprotoSession,
        request: &XrpcRequest<(), PutProfileRecordInput>,
        label: &str,
    ) -> Result<PutProfileRecordOutput, AtprotoError> {
        let output: atrium_xrpc::OutputDataOrBytes<PutProfileRecordOutput> = session
            .send_xrpc::<(), PutProfileRecordInput, PutProfileRecordOutput, serde_json::Value>(
                request,
            )
            .await
            .map_err(|error| {
                tracing::warn!(%error, %label, "ATProto shelf write failed");
                AtprotoError::authorization(format!("{label} write failed: {error}"))
            })?;
        match output {
            atrium_xrpc::OutputDataOrBytes::Data(written) => Ok(written),
            atrium_xrpc::OutputDataOrBytes::Bytes(_) => Err(AtprotoError::authorization(
                "unexpected binary response to putRecord",
            )),
        }
    }

    async fn read_profile_record(
        &self,
        session: &AtprotoSession,
        did: &Did,
    ) -> Result<Option<serde_json::Value>, AtprotoError> {
        let request = get_profile_record_request(did.as_str());
        match session
            .send_xrpc::<GetProfileRecordParams, (), GetProfileRecordOutput, serde_json::Value>(
                &request,
            )
            .await
        {
            Ok(atrium_xrpc::OutputDataOrBytes::Data(data)) => Ok(data.value),
            Ok(atrium_xrpc::OutputDataOrBytes::Bytes(_)) => Ok(None),
            // A missing record is the normal first-write case.
            Err(atrium_xrpc::Error::XrpcResponse(error))
                if error.status == atrium_xrpc::http::StatusCode::BAD_REQUEST =>
            {
                Ok(None)
            }
            Err(error) => Err(AtprotoError::authorization(format!(
                "could not read the profile record: {error}"
            ))),
        }
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

        // The Livtet profile lives on the user's own PDS; the appview merely
        // indexes it, and may lag or lack the record entirely.
        let pds_session = self.restore_session(&did).await?;
        if let Some(record) = self.read_profile_record(&pds_session, &did).await? {
            tracing::info!(did = did.as_str(), "read the ATProto profile from the PDS");
            let handle = read_handle(&pds_session).await;
            return Ok(Some(profile_view_from_record(
                did.as_str(),
                &record,
                handle,
                Some(&pds_session.base_uri()),
            )));
        }

        tracing::info!(
            did = did.as_str(),
            nsid = APPVIEW_PROFILE_NSID,
            "ATProto appview profile request"
        );
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

        let now = atrium_api::types::string::Datetime::now();
        if let Some(expires_at) = &stored.token_set.expires_at {
            if *expires_at > now {
                // Proactive refresh: when token expires within REFRESH_BEFORE_EXPIRY_SECS.
                let boundary_ts = now.as_ref().timestamp() + REFRESH_BEFORE_EXPIRY_SECS as i64;
                if expires_at.as_ref().timestamp() <= boundary_ts {
                    tracing::info!(
                        did = did.as_str(),
                        expires_at = expires_at.as_str(),
                        "ATProto token expires soon; forcing proactive session refresh"
                    );
                } else {
                    tracing::debug!(
                        expires_at = expires_at.as_str(),
                        "ATProto token still valid (proactive window not reached)"
                    );
                    return Ok(());
                }
            } else {
                tracing::debug!(
                    expires_at = expires_at.as_str(),
                    "ATProto token still valid"
                );
                return Ok(());
            }
        }
        tracing::info!(
            did = did.as_str(),
            "ATProto token expired or absent; forcing a session refresh"
        );

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
struct GetProfileRecordParams {
    repo: String,
    collection: String,
    rkey: String,
}

#[derive(serde::Deserialize)]
struct UploadBlobOutput {
    blob: UploadedBlob,
}

#[derive(serde::Deserialize)]
struct UploadedBlob {
    #[serde(rename = "ref")]
    reference: BlobLink,
}

#[derive(serde::Deserialize)]
struct BlobLink {
    #[serde(rename = "$link")]
    link: String,
}

/// Upload raw image bytes as an avatar blob.
fn upload_blob_request(bytes: Vec<u8>, mime: &str) -> XrpcRequest<(), Vec<u8>> {
    XrpcRequest {
        method: Method::POST,
        nsid: "com.atproto.repo.uploadBlob".to_string(),
        parameters: None,
        input: Some(atrium_xrpc::InputDataOrBytes::Bytes(bytes)),
        encoding: Some(mime.to_string()),
    }
}

/// Identify an image from its magic bytes.
fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

#[derive(serde::Deserialize)]
struct SessionInfoOutput {
    handle: Option<String>,
}

#[derive(serde::Deserialize)]
struct GetProfileRecordOutput {
    value: Option<serde_json::Value>,
}

#[derive(serde::Serialize)]
struct PutProfileRecordInput {
    repo: String,
    collection: String,
    rkey: String,
    record: serde_json::Value,
    // The Livtet lexicons are not published, so the PDS cannot validate.
    validate: bool,
}

/// Result of publishing a shelving record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShelvedBook {
    /// `at://` URI of the shelving record now in the account's repository.
    pub record_uri: String,
    /// `at://` URI of the shelf record the shelving record points at.
    pub shelf_uri: String,
}

#[derive(serde::Deserialize)]
struct PutProfileRecordOutput {
    uri: String,
    cid: String,
}

/// Read the current profile record from the account's PDS.
fn get_profile_record_request(did: &str) -> XrpcRequest<GetProfileRecordParams, ()> {
    XrpcRequest {
        method: Method::GET,
        nsid: "com.atproto.repo.getRecord".to_string(),
        parameters: Some(GetProfileRecordParams {
            repo: did.to_string(),
            collection: PROFILE_COLLECTION.to_string(),
            rkey: PROFILE_RKEY.to_string(),
        }),
        input: None::<atrium_xrpc::InputDataOrBytes<()>>,
        encoding: None,
    }
}

/// Create or update the profile record.
/// Generic `com.atproto.repo.putRecord` request for any collection/rkey.
fn put_record_request(
    did: &str,
    collection: &str,
    rkey: &str,
    record: &serde_json::Value,
    validate: bool,
) -> XrpcRequest<(), PutProfileRecordInput> {
    XrpcRequest {
        method: Method::POST,
        nsid: "com.atproto.repo.putRecord".to_string(),
        parameters: None,
        input: Some(atrium_xrpc::InputDataOrBytes::Data(PutProfileRecordInput {
            repo: did.to_string(),
            collection: collection.to_string(),
            rkey: rkey.to_string(),
            record: record.clone(),
            validate,
        })),
        encoding: Some("application/json".to_string()),
    }
}

fn put_profile_record_request(
    did: &str,
    record: &serde_json::Value,
) -> XrpcRequest<(), PutProfileRecordInput> {
    XrpcRequest {
        method: Method::POST,
        nsid: "com.atproto.repo.putRecord".to_string(),
        parameters: None,
        input: Some(atrium_xrpc::InputDataOrBytes::Data(PutProfileRecordInput {
            repo: did.to_string(),
            collection: PROFILE_COLLECTION.to_string(),
            rkey: PROFILE_RKEY.to_string(),
            record: record.clone(),
            validate: false,
        })),
        encoding: Some("application/json".to_string()),
    }
}

/// Merge the edited text into the record, preserving the fields the editor
/// does not touch. Empty text clears a field to `null`.
fn merge_profile_record(
    existing: Option<&serde_json::Value>,
    display_name: Option<&str>,
    description: Option<&str>,
    avatar_cid: Option<&str>,
    clear_avatar: bool,
    now: &str,
) -> serde_json::Value {
    let mut record = match existing {
        Some(serde_json::Value::Object(map)) => map.clone(),
        _ => serde_json::Map::new(),
    };
    record.insert(
        "$type".to_string(),
        serde_json::Value::String(PROFILE_COLLECTION.to_string()),
    );
    set_optional_text(&mut record, "displayName", display_name);
    set_optional_text(&mut record, "description", description);
    if clear_avatar {
        record.insert("avatarCid".to_string(), serde_json::Value::Null);
    } else {
        match avatar_cid {
            Some(cid) => {
                record.insert(
                    "avatarCid".to_string(),
                    serde_json::Value::String(cid.to_string()),
                );
            }
            None => {
                record
                    .entry("avatarCid".to_string())
                    .or_insert(serde_json::Value::Null);
            }
        }
    }
    record
        .entry("createdAt".to_string())
        .or_insert_with(|| serde_json::Value::String(now.to_string()));
    record.insert(
        "updatedAt".to_string(),
        serde_json::Value::String(now.to_string()),
    );
    serde_json::Value::Object(record)
}

fn set_optional_text(
    record: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&str>,
) {
    let Some(value) = value else {
        return;
    };
    let trimmed = value.trim();
    let stored = if trimmed.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(trimmed.to_string())
    };
    record.insert(key.to_string(), stored);
}

async fn read_handle(session: &AtprotoSession) -> Option<String> {
    let request = XrpcRequest {
        method: Method::GET,
        nsid: "com.atproto.server.getSession".to_string(),
        parameters: None,
        input: None::<atrium_xrpc::InputDataOrBytes<()>>,
        encoding: None,
    };
    match session
        .send_xrpc::<(), (), SessionInfoOutput, serde_json::Value>(&request)
        .await
    {
        Ok(atrium_xrpc::OutputDataOrBytes::Data(info)) => info.handle,
        _ => None,
    }
}

fn profile_view_from_record(
    did: &str,
    record: &serde_json::Value,
    handle: Option<String>,
    pds_uri: Option<&str>,
) -> AtprotoProfileView {
    let text = |key: &str| {
        record
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let avatar_cid = text("avatarCid");
    AtprotoProfileView {
        did: did.to_string(),
        handle,
        display_name: text("displayName"),
        description: text("description"),
        avatar_url: avatar_url(did, avatar_cid.as_deref(), pds_uri),
        avatar_cid,
    }
}

/// Resolve the avatar image URL.
///
/// A Livtet avatar is a blob on the user's PDS, so it is fetched with
/// `com.atproto.sync.getBlob`. Without one, the Livtet platform's proxy serves
/// the account's Bluesky avatar, which keeps parity with the web app.
fn avatar_url(did: &str, avatar_cid: Option<&str>, pds_uri: Option<&str>) -> Option<String> {
    match (avatar_cid, pds_uri) {
        (Some(cid), Some(base)) => Some(format!(
            "{base}/xrpc/com.atproto.sync.getBlob?did={did}&cid={cid}"
        )),
        _ => Some(format!("{PLATFORM_AVATAR_BASE}/{did}")),
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
    // A null record is not the same as a missing identity: the appview can
    // still resolve the handle, which is worth showing.
    if response.profile.is_none() && response.handle.is_none() {
        return None;
    }
    Some(AtprotoProfileView {
        did: did.to_string(),
        handle: response.handle,
        display_name: response
            .profile
            .as_ref()
            .and_then(|p| p.display_name.clone()),
        description: response
            .profile
            .as_ref()
            .and_then(|p| p.description.clone()),
        avatar_cid: response.profile.as_ref().and_then(|p| p.avatar_cid.clone()),
        avatar_url: avatar_url(
            did,
            response
                .profile
                .as_ref()
                .and_then(|p| p.avatar_cid.as_deref()),
            None,
        ),
    })
}

fn parse_callback_url(raw: &str) -> Result<atrium_oauth::CallbackParams, AtprotoError> {
    let url =
        url::Url::parse(raw).map_err(|_| AtprotoError::authorization("invalid callback URL"))?;

    let loopback = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.path() == "/oauth-redirect";
    if url.scheme() != "net.olamaelcu.livtet" && !loopback {
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
    fn parse_callback_accepts_the_loopback_redirect() {
        let params = parse_callback_url(
            "http://127.0.0.1:47821/oauth-redirect?code=abc&state=xyz&iss=https%3A%2F%2Fpds.example",
        )
        .expect("parses");
        assert_eq!(params.code, "abc");
        assert_eq!(params.state.as_deref(), Some("xyz"));
    }

    #[test]
    fn parse_callback_rejects_loopback_on_a_foreign_path() {
        let error =
            parse_callback_url("http://127.0.0.1:47821/evil?code=abc").expect_err("wrong path");
        assert!(error.to_string().contains("foreign scheme"));
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
    fn merge_profile_record_creates_a_record() {
        let record = merge_profile_record(
            None,
            Some(" Reader "),
            Some("  "),
            None,
            false,
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(record["$type"], PROFILE_COLLECTION);
        assert_eq!(record["displayName"], "Reader");
        assert!(record["description"].is_null());
        assert!(record["avatarCid"].is_null());
        assert_eq!(record["createdAt"], "2026-01-01T00:00:00Z");
        assert_eq!(record["updatedAt"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn merge_profile_record_preserves_untouched_fields() {
        let existing = serde_json::json!({
            "$type": PROFILE_COLLECTION,
            "displayName": "Old",
            "description": "Old bio",
            "avatarCid": "bafyavatar",
            "createdAt": "2025-05-05T00:00:00Z",
            "updatedAt": "2025-05-06T00:00:00Z",
        });

        let record = merge_profile_record(
            Some(&existing),
            Some("New"),
            Some("New bio"),
            None,
            false,
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(record["displayName"], "New");
        assert_eq!(record["description"], "New bio");
        assert_eq!(record["avatarCid"], "bafyavatar");
        assert_eq!(record["createdAt"], "2025-05-05T00:00:00Z");
        assert_eq!(record["updatedAt"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn merge_profile_record_clears_empty_text() {
        let existing = serde_json::json!({
            "$type": PROFILE_COLLECTION,
            "displayName": "Old",
            "description": "Old bio",
        });

        let record = merge_profile_record(
            Some(&existing),
            Some("   "),
            Some("New bio"),
            None,
            false,
            "2026-01-01T00:00:00Z",
        );
        assert!(record["displayName"].is_null());
        assert_eq!(record["description"], "New bio");
    }

    #[test]
    fn profile_requests_target_the_profile_collection() {
        let get = get_profile_record_request(DID_A);
        assert_eq!(get.nsid, "com.atproto.repo.getRecord");
        let params = get.parameters.expect("parameters");
        assert_eq!(params.collection, PROFILE_COLLECTION);
        assert_eq!(params.rkey, PROFILE_RKEY);

        let record = serde_json::json!({ "displayName": "Reader" });
        let put = put_profile_record_request(DID_A, &record);
        assert_eq!(put.nsid, "com.atproto.repo.putRecord");
        assert_eq!(put.method, Method::POST);
        let Some(atrium_xrpc::InputDataOrBytes::Data(input)) = put.input else {
            panic!("expected a JSON body");
        };
        assert_eq!(input.repo, DID_A);
        assert_eq!(input.collection, PROFILE_COLLECTION);
        assert_eq!(input.rkey, PROFILE_RKEY);
        assert!(!input.validate);
    }

    #[test]
    fn profile_view_from_record_reads_the_text_fields() {
        let record = serde_json::json!({
            "displayName": "Reader",
            "description": "Reads books",
            "avatarCid": "bafyavatar",
        });
        let view = profile_view_from_record(
            DID_A,
            &record,
            Some("reader.example".to_string()),
            Some("https://pds.example"),
        );
        assert_eq!(view.did, DID_A);
        assert_eq!(view.display_name.as_deref(), Some("Reader"));
        assert_eq!(view.description.as_deref(), Some("Reads books"));
        assert_eq!(view.avatar_cid.as_deref(), Some("bafyavatar"));
        assert_eq!(view.handle.as_deref(), Some("reader.example"));
    }

    #[test]
    fn merge_profile_record_sets_a_new_avatar() {
        let existing = serde_json::json!({
            "$type": PROFILE_COLLECTION,
            "displayName": "Old",
            "description": "Old bio",
            "avatarCid": "bafyold",
        });
        let record = merge_profile_record(
            Some(&existing),
            None,
            None,
            Some("bafynew"),
            false,
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(record["avatarCid"], "bafynew");
        assert_eq!(record["displayName"], "Old");
        assert_eq!(record["description"], "Old bio");
    }

    #[test]
    fn avatar_url_prefers_the_livtet_blob() {
        let url = avatar_url(DID_A, Some("bafyavatar"), Some("https://pds.example"));
        assert_eq!(
            url.as_deref(),
            Some(
                "https://pds.example/xrpc/com.atproto.sync.getBlob?did=did:plc:aaaaaaaaaaaaaaaaaaaaaaaa&cid=bafyavatar"
            )
        );
    }

    #[test]
    fn avatar_url_falls_back_to_the_platform_proxy() {
        let url = avatar_url(DID_A, None, Some("https://pds.example"));
        assert_eq!(
            url.as_deref(),
            Some("https://livtet.olamaelcu.net/user/profileImage/did:plc:aaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn image_mime_identifies_supported_formats() {
        assert_eq!(
            image_mime(&[0x89, b'P', b'N', b'G', 0x0D]),
            Some("image/png")
        );
        assert_eq!(image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(image_mime(b"GIF89a"), Some("image/gif"));
        assert_eq!(image_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_mime(b"not an image"), None);
    }

    #[test]
    fn upload_blob_request_carries_the_bytes_and_mime() {
        let request = upload_blob_request(vec![1, 2, 3], "image/png");
        assert_eq!(request.nsid, "com.atproto.repo.uploadBlob");
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.encoding.as_deref(), Some("image/png"));
        match request.input {
            Some(atrium_xrpc::InputDataOrBytes::Bytes(bytes)) => assert_eq!(bytes, vec![1, 2, 3]),
            _ => panic!("expected raw bytes"),
        }
    }

    #[test]
    fn map_profile_response_returns_none_for_null() {
        let response: AppViewProfileResponse =
            serde_json::from_value(serde_json::json!({ "profile": null })).expect("response");
        assert!(map_profile_response(DID_A, response).is_none());
    }

    #[test]
    fn map_profile_response_keeps_the_handle_without_a_record() {
        let response: AppViewProfileResponse = serde_json::from_value(
            serde_json::json!({ "profile": null, "handle": "reader.example" }),
        )
        .expect("response");

        let view = map_profile_response(DID_A, response).expect("a handle-only view");
        assert_eq!(view.handle.as_deref(), Some("reader.example"));
        assert!(view.display_name.is_none());
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
        assert_eq!(view.handle.as_deref(), Some("reader.example"));
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
