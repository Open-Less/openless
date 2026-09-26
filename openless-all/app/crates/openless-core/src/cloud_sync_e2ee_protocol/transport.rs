//! Bounded HTTPS transport for the encrypted sync v1 wire contract.
//!
//! A transport is usable only after its capabilities have been checked against
//! the native application's configured OAuth client ID. Mutations are prepared
//! before sending: cancellation, a network failure, or an invalid response must
//! leave that same prepared operation available for reconciliation or retry.

use std::{borrow::Cow, fmt, sync::Arc, time::Duration};

use reqwest::{
    header::{
        HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CACHE_CONTROL,
        CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_MATCH, IF_NONE_MATCH, RETRY_AFTER,
    },
    Client, Method, RequestBuilder, Response, StatusCode,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use url::Url;
use zeroize::Zeroizing;

use super::{
    types::{
        parse_wire, AuthSession, Capabilities, DeleteVaultRequest, ErrorEnvelope, GithubId,
        OperationKind, OperationPending, OperationReceipt, RemoteErrorCode, Revision, Sha256Digest,
        SnapshotUpload, StrongEtag, UploadKind, UuidV4, Validate, VaultMetadata, VaultState,
        CONTROL_BODY_LIMIT, HTTP_BODY_LIMIT, PROTOCOL_VERSION,
    },
    Error, Result,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const IDEMPOTENCY_KEY: &str = "idempotency-key";
const IDEMPOTENCY_REPLAYED: &str = "idempotency-replayed";
// A valid JSON request stored as a JSON string needs at most twice its original
// bytes for escaping. The headroom covers the fixed metadata and bounded origin.
const PENDING_RECORD_LIMIT: usize = HTTP_BODY_LIMIT * 2 + CONTROL_BODY_LIMIT;
const PENDING_ORIGIN_LIMIT: usize = 2048;

/// One captured Core setting and the effective routing decision for this origin.
/// Loopback stays direct even when the captured setting allows system proxies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProxyPolicy {
    use_system_proxy: bool,
    bypass_proxy: bool,
}

impl ProxyPolicy {
    pub(crate) fn for_origin(origin: &str, use_system_proxy: bool) -> Self {
        Self {
            use_system_proxy,
            bypass_proxy: crate::net::should_bypass_proxy(origin, use_system_proxy),
        }
    }

    fn apply(self, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
        if self.bypass_proxy {
            builder.no_proxy()
        } else {
            builder
        }
    }
}

/// No caller-supplied HTTP client can weaken the transport's TLS or redirect policy.
#[derive(Clone)]
pub(crate) struct Transport {
    origin: Url,
    client: Client,
    capabilities: Arc<Capabilities>,
    proxy_policy: ProxyPolicy,
}

impl fmt::Debug for Transport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Transport").finish_non_exhaustive()
    }
}

/// Native-only, origin-bound credentials; never serialize this into a UI result.
pub(crate) struct SyncSession {
    origin: Url,
    session: AuthSession,
}

impl fmt::Debug for SyncSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyncSession")
            .finish_non_exhaustive()
    }
}

impl SyncSession {
    pub(crate) fn account(&self) -> &super::types::Account {
        &self.session.account
    }

    pub(crate) fn account_id(&self) -> &GithubId {
        &self.session.account.github_id
    }

    pub(crate) fn expires_in(&self) -> u32 {
        self.session.expires_in
    }
}

/// A metadata representation and its opaque ETag, bound to one service origin.
#[derive(Clone, Debug)]
pub(crate) struct Metadata {
    origin: Url,
    value: VaultMetadata,
    etag: StrongEtag,
}

impl Metadata {
    pub(crate) fn value(&self) -> &VaultMetadata {
        &self.value
    }

    #[cfg(test)]
    pub(crate) fn etag(&self) -> &StrongEtag {
        &self.etag
    }
}

#[derive(Debug)]
pub(crate) enum MetadataResult {
    Modified(Box<Metadata>),
    /// The caller retains the same account's supplied cached metadata.
    NotModified,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MutationMethod {
    Upload,
    Delete,
}

impl MutationMethod {
    fn http_method(self) -> Method {
        match self {
            Self::Upload => Method::PUT,
            Self::Delete => Method::DELETE,
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Upload => "/v1/me/vault/snapshot",
            Self::Delete => "/v1/me/vault",
        }
    }

    fn body_limit(self) -> usize {
        match self {
            Self::Upload => HTTP_BODY_LIMIT,
            Self::Delete => CONTROL_BODY_LIMIT,
        }
    }
}

/// Local persistence envelope, never an HTTP request. Cow borrows the original
/// bytes when exporting; deserialization deliberately owns every string.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PendingRecord<'a> {
    record_version: u32,
    origin: Cow<'a, str>,
    owner_github_id: GithubId,
    method: MutationMethod,
    body: Cow<'a, str>,
    if_match: Cow<'a, str>,
    operation_id: UuidV4,
}

impl Validate for PendingRecord<'_> {
    fn validate(&self) -> Result<()> {
        if self.record_version != 1 {
            return Err(Error::InvalidWire("pending record version"));
        }
        if self.origin.len() > PENDING_ORIGIN_LIMIT || self.body.len() > self.method.body_limit() {
            return Err(Error::PayloadTooLarge);
        }
        StrongEtag::parse(&self.if_match)?;
        Ok(())
    }
}

/// Immutable retry material. Retain it before starting a mutation; durable
/// pending-operation storage belongs to the native sync coordinator.
///
/// Never regenerate ciphertext, an operation ID, or an If-Match value because a
/// request timed out. A missing receipt also does not prove a write failed.
#[derive(Clone)]
pub(crate) struct PreparedOperation {
    origin: Url,
    owner: GithubId,
    method: MutationMethod,
    body: Vec<u8>,
    if_match: StrongEtag,
    operation_id: UuidV4,
    expected_kind: OperationKind,
    expected_revision: Revision,
    expected_vault: UuidV4,
    expected_sha256: Option<Sha256Digest>,
}

impl fmt::Debug for PreparedOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedOperation")
            .field("method", &self.method)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

impl PreparedOperation {
    #[cfg(test)]
    pub(crate) fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    #[cfg(test)]
    pub(crate) fn if_match(&self) -> &StrongEtag {
        &self.if_match
    }

    pub(crate) fn operation_id(&self) -> &UuidV4 {
        &self.operation_id
    }

    /// Export retry material without credentials, filesystem access or network
    /// access. The native coordinator owns durable storage and its lifecycle.
    /// Body bytes are kept verbatim, including JSON whitespace and key ordering.
    pub(crate) fn to_pending_record(&self) -> Result<Vec<u8>> {
        let body = std::str::from_utf8(&self.body)
            .map_err(|_| Error::InvalidWire("pending request encoding"))?;
        let record = PendingRecord {
            record_version: 1,
            origin: Cow::Borrowed(self.origin.as_str()),
            owner_github_id: self.owner.clone(),
            method: self.method,
            body: Cow::Borrowed(body),
            if_match: Cow::Borrowed(self.if_match.as_str()),
            operation_id: self.operation_id.clone(),
        };
        record.validate()?;
        serialize_body(&record, PENDING_RECORD_LIMIT)
    }

    fn validate_receipt(&self, receipt: &OperationReceipt) -> Result<()> {
        receipt.validate()?;
        if receipt.operation_id != self.operation_id
            || receipt.kind != self.expected_kind
            || receipt.committed_revision != self.expected_revision
            || receipt.vault_id != self.expected_vault
            || receipt.ciphertext_sha256 != self.expected_sha256
        {
            return Err(Error::InvalidResponse("receipt does not match operation"));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct MutationReceipt {
    pub(crate) receipt: OperationReceipt,
    /// This confirms an earlier commit, never that its revision is still current.
    pub(crate) replayed: bool,
}

#[derive(Debug)]
pub(crate) enum OperationStatus {
    Committed(OperationReceipt),
    Pending(OperationPending),
    /// Keep the original operation. It may still be in flight or have expired
    /// from the server's receipt retention window.
    NotFound,
}

impl Transport {
    /// Check capabilities without credentials before allowing authentication.
    pub(crate) async fn new(
        origin: &str,
        expected_github_client_id: &str,
        proxy_policy: ProxyPolicy,
    ) -> Result<Self> {
        let origin = parse_origin(origin, false)?;
        let proxy_policy = ProxyPolicy::for_origin(origin.as_str(), proxy_policy.use_system_proxy);
        let client = client_builder(proxy_policy)
            .https_only(true)
            .build()
            .map_err(|_| Error::Transport)?;
        Self::check_capabilities(origin, client, expected_github_client_id, proxy_policy).await
    }

    async fn check_capabilities(
        origin: Url,
        client: Client,
        expected_github_client_id: &str,
        proxy_policy: ProxyPolicy,
    ) -> Result<Self> {
        if expected_github_client_id.is_empty()
            || expected_github_client_id.len() > 128
            || !expected_github_client_id
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err(Error::InvalidEndpoint);
        }
        let mut endpoint = origin.clone();
        endpoint.set_path("/v1/capabilities");
        let response = send(client.get(endpoint).header(ACCEPT, "application/json")).await?;
        let capabilities: Capabilities = success_json(response, CONTROL_BODY_LIMIT).await?;
        if capabilities.github_client_id != expected_github_client_id {
            return Err(Error::InvalidResponse("OAuth application mismatch"));
        }
        Ok(Self {
            origin,
            client,
            capabilities: Arc::new(capabilities),
            proxy_policy,
        })
    }

    pub(crate) fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub(crate) fn proxy_policy(&self) -> ProxyPolicy {
        self.proxy_policy
    }

    /// Canonical HTTPS origin for native secure-store/journal scope binding.
    pub(crate) fn service_origin(&self) -> &str {
        self.origin.as_str()
    }

    pub(crate) async fn exchange(
        &self,
        github_token: &str,
        expected_account: &GithubId,
    ) -> Result<SyncSession> {
        let response = send(
            self.request(Method::POST, "/v1/auth/github")
                .header(AUTHORIZATION, bearer_header(github_token)?),
        )
        .await?;
        let session: AuthSession = success_json(response, CONTROL_BODY_LIMIT).await?;
        if &session.account.github_id != expected_account {
            return Err(Error::AccountMismatch);
        }
        Ok(SyncSession {
            origin: self.origin.clone(),
            session,
        })
    }

    /// Consume the local credential even when remote logout fails. A validated
    /// 401 means the credential is already unusable and is treated as logged out.
    pub(crate) async fn revoke_session(&self, session: SyncSession) -> Result<()> {
        let response =
            send(self.authenticated_request(&session, Method::DELETE, "/v1/auth/session")?).await?;
        if response.status() == StatusCode::NO_CONTENT {
            return empty_response(response).await;
        }
        let error = api_error(response).await;
        match error {
            Error::Api {
                status: 401,
                code: RemoteErrorCode::Unauthenticated | RemoteErrorCode::SessionExpired,
                ..
            } => Ok(()),
            error => Err(error),
        }
    }

    pub(crate) async fn metadata(
        &self,
        session: &SyncSession,
        cached: Option<&Metadata>,
    ) -> Result<MetadataResult> {
        let mut request = self.authenticated_request(session, Method::GET, "/v1/me/vault")?;
        if let Some(metadata) = cached {
            self.check_metadata(session, metadata)?;
            request = request.header(IF_NONE_MATCH, metadata.etag.as_str());
        }
        let response = send(request).await?;
        if response.status() == StatusCode::NOT_MODIFIED {
            let metadata =
                cached.ok_or(Error::InvalidResponse("unsolicited not-modified response"))?;
            if response_etag(response.headers())? != metadata.etag {
                return Err(Error::InvalidResponse("not-modified ETag mismatch"));
            }
            empty_response(response).await?;
            return Ok(MetadataResult::NotModified);
        }
        // This route always represents empty/deleted state with a 200 DTO.
        if response.status() == StatusCode::NOT_FOUND {
            return Err(Error::InvalidResponse("metadata route unavailable"));
        }
        if response.status() != StatusCode::OK {
            return Err(api_error(response).await);
        }
        let etag = response_etag(response.headers())?;
        let value: VaultMetadata = response_json(response, CONTROL_BODY_LIMIT).await?;
        if &value.owner_github_id != session.account_id() {
            return Err(Error::AccountMismatch);
        }
        Ok(MetadataResult::Modified(Box::new(Metadata {
            origin: self.origin.clone(),
            value,
            etag,
        })))
    }

    pub(crate) async fn snapshot(
        &self,
        session: &SyncSession,
        metadata: &Metadata,
    ) -> Result<SnapshotUpload> {
        self.check_metadata(session, metadata)?;
        if metadata.value.state != VaultState::Active {
            return Err(Error::ContextMismatch);
        }
        let response = send(
            self.authenticated_request(session, Method::GET, "/v1/me/vault/snapshot")?
                .header(IF_MATCH, metadata.etag.as_str()),
        )
        .await?;
        if response.status() != StatusCode::OK {
            return Err(api_error(response).await);
        }
        if response_etag(response.headers())? != metadata.etag {
            return Err(Error::InvalidResponse("snapshot ETag mismatch"));
        }
        let snapshot: SnapshotUpload = response_json(response, HTTP_BODY_LIMIT).await?;
        snapshot.validate_against_metadata(&metadata.value, session.account_id())?;
        Ok(snapshot)
    }

    /// Active uploads also require their downloaded predecessor, because metadata
    /// deliberately does not expose the KDF salt that must stay in its key epoch.
    pub(crate) fn prepare_upload(
        &self,
        session: &SyncSession,
        metadata: &Metadata,
        upload: &SnapshotUpload,
        previous_snapshot: Option<&SnapshotUpload>,
    ) -> Result<PreparedOperation> {
        self.check_metadata(session, metadata)?;
        upload.validate()?;
        if &upload.owner_github_id != session.account_id() {
            return Err(Error::AccountMismatch);
        }
        if upload.base_revision != metadata.value.revision
            || upload.revision != metadata.value.revision.checked_next()?
            || metadata.value.last_operation_id.as_ref() == Some(&upload.operation_id)
        {
            return Err(Error::ContextMismatch);
        }
        let expected_kind = match upload.kind {
            UploadKind::Create => {
                if metadata.value.state == VaultState::Active
                    || metadata.value.vault_id.as_ref() == Some(&upload.vault_id)
                    || previous_snapshot.is_some()
                {
                    return Err(Error::ContextMismatch);
                }
                OperationKind::Create
            }
            UploadKind::Snapshot | UploadKind::PasswordChange => {
                let previous = previous_snapshot.ok_or(Error::ContextMismatch)?;
                previous.validate_against_metadata(&metadata.value, session.account_id())?;
                if upload.vault_id != previous.vault_id
                    || upload.operation_id == previous.operation_id
                    || upload.nonce == previous.nonce
                {
                    return Err(Error::ContextMismatch);
                }
                let same_key = upload.key_id == previous.key_id;
                let same_salt = upload.kdf.salt == previous.kdf.salt;
                if (upload.kind == UploadKind::Snapshot && (!same_key || !same_salt))
                    || (upload.kind == UploadKind::PasswordChange && (same_key || same_salt))
                {
                    return Err(Error::ContextMismatch);
                }
                OperationKind::from(upload.kind)
            }
        };
        let body = serialize_body(upload, HTTP_BODY_LIMIT)?;
        Ok(PreparedOperation {
            origin: self.origin.clone(),
            owner: session.account_id().clone(),
            method: MutationMethod::Upload,
            body,
            if_match: metadata.etag.clone(),
            operation_id: upload.operation_id.clone(),
            expected_kind,
            expected_revision: upload.revision,
            expected_vault: upload.vault_id.clone(),
            expected_sha256: Some(upload.ciphertext_sha256.clone()),
        })
    }

    pub(crate) fn prepare_delete(
        &self,
        session: &SyncSession,
        metadata: &Metadata,
        operation_id: UuidV4,
    ) -> Result<PreparedOperation> {
        self.check_metadata(session, metadata)?;
        if metadata.value.state != VaultState::Active
            || metadata.value.last_operation_id.as_ref() == Some(&operation_id)
        {
            return Err(Error::ContextMismatch);
        }
        let vault_id = metadata
            .value
            .vault_id
            .clone()
            .ok_or(Error::ContextMismatch)?;
        let expected_revision = metadata.value.revision.checked_next()?;
        let request = DeleteVaultRequest {
            protocol_version: PROTOCOL_VERSION,
            base_revision: metadata.value.revision,
            operation_id: operation_id.clone(),
            expected_vault_id: vault_id.clone(),
        };
        request.validate()?;
        Ok(PreparedOperation {
            origin: self.origin.clone(),
            owner: session.account_id().clone(),
            method: MutationMethod::Delete,
            body: serialize_body(&request, CONTROL_BODY_LIMIT)?,
            if_match: metadata.etag.clone(),
            operation_id,
            expected_kind: OperationKind::Delete,
            expected_revision,
            expected_vault: vault_id,
            expected_sha256: None,
        })
    }

    /// Recover an already prepared operation after a restart. This performs only
    /// local validation: current metadata must not replace the original CAS
    /// precondition, and receipt retention/reconciliation belongs to the caller.
    /// Expected receipt fields are derived from the validated original body.
    pub(crate) fn restore_pending(
        &self,
        session: &SyncSession,
        bytes: &[u8],
    ) -> Result<PreparedOperation> {
        self.check_session(session)?;
        let record: PendingRecord<'static> = parse_wire(bytes, PENDING_RECORD_LIMIT)?;
        // The HTTP exception is compiled exclusively for the loopback unit tests;
        // production records must have an HTTPS origin just like Transport::new.
        let origin = parse_origin(&record.origin, cfg!(test))?;
        if origin != self.origin {
            return Err(Error::ContextMismatch);
        }
        if &record.owner_github_id != session.account_id() {
            return Err(Error::AccountMismatch);
        }
        let if_match = StrongEtag::parse(&record.if_match)?;
        let (expected_kind, expected_revision, expected_vault, expected_sha256) = match record
            .method
        {
            MutationMethod::Upload => {
                let upload: SnapshotUpload = parse_wire(record.body.as_bytes(), HTTP_BODY_LIMIT)?;
                if &upload.owner_github_id != session.account_id() {
                    return Err(Error::AccountMismatch);
                }
                if upload.operation_id != record.operation_id {
                    return Err(Error::ContextMismatch);
                }
                if upload.kind != UploadKind::Create && upload.base_revision.get() == 0 {
                    return Err(Error::InvalidWire("pending upload revision"));
                }
                (
                    OperationKind::from(upload.kind),
                    upload.revision,
                    upload.vault_id,
                    Some(upload.ciphertext_sha256),
                )
            }
            MutationMethod::Delete => {
                let request: DeleteVaultRequest =
                    parse_wire(record.body.as_bytes(), CONTROL_BODY_LIMIT)?;
                if request.operation_id != record.operation_id {
                    return Err(Error::ContextMismatch);
                }
                (
                    OperationKind::Delete,
                    request.base_revision.checked_next()?,
                    request.expected_vault_id,
                    None,
                )
            }
        };
        Ok(PreparedOperation {
            origin,
            owner: record.owner_github_id,
            method: record.method,
            body: record.body.into_owned().into_bytes(),
            if_match,
            operation_id: record.operation_id,
            expected_kind,
            expected_revision,
            expected_vault,
            expected_sha256,
        })
    }

    /// Every error (including cancellation) leaves the borrowed operation intact.
    /// The caller must reconcile it, never infer from an error that no commit occurred.
    pub(crate) async fn submit(
        &self,
        session: &SyncSession,
        operation: &PreparedOperation,
    ) -> Result<MutationReceipt> {
        self.check_operation(session, operation)?;
        let response = send(
            self.authenticated_request(
                session,
                operation.method.http_method(),
                operation.method.path(),
            )?
            .header(CONTENT_TYPE, "application/json")
            .header(IF_MATCH, operation.if_match.as_str())
            .header(IDEMPOTENCY_KEY, operation.operation_id.as_str())
            .body(operation.body.clone()),
        )
        .await?;
        if response.status() != StatusCode::OK {
            return Err(api_error(response).await);
        }
        let replayed = match single_header(response.headers(), IDEMPOTENCY_REPLAYED)? {
            "true" => true,
            "false" => false,
            _ => return Err(Error::InvalidResponse("invalid replay header")),
        };
        let receipt: OperationReceipt = response_json(response, CONTROL_BODY_LIMIT).await?;
        operation.validate_receipt(&receipt)?;
        Ok(MutationReceipt { receipt, replayed })
    }

    pub(crate) async fn operation(
        &self,
        session: &SyncSession,
        operation: &PreparedOperation,
    ) -> Result<OperationStatus> {
        self.check_operation(session, operation)?;
        let path = format!("/v1/me/operations/{}", operation.operation_id);
        let response = send(self.authenticated_request(session, Method::GET, &path)?).await?;
        match response.status() {
            StatusCode::OK => {
                let receipt: OperationReceipt = response_json(response, CONTROL_BODY_LIMIT).await?;
                operation.validate_receipt(&receipt)?;
                Ok(OperationStatus::Committed(receipt))
            }
            StatusCode::ACCEPTED => {
                let pending: OperationPending = response_json(response, CONTROL_BODY_LIMIT).await?;
                if pending.operation_id != operation.operation_id {
                    return Err(Error::InvalidResponse("pending operation mismatch"));
                }
                Ok(OperationStatus::Pending(pending))
            }
            _ => match api_error(response).await {
                Error::Api {
                    status: 404,
                    code: RemoteErrorCode::OperationNotFound,
                    ..
                } => Ok(OperationStatus::NotFound),
                error => Err(error),
            },
        }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let mut endpoint = self.origin.clone();
        endpoint.set_path(path);
        self.client
            .request(method, endpoint)
            .header(ACCEPT, "application/json")
    }

    fn authenticated_request(
        &self,
        session: &SyncSession,
        method: Method,
        path: &str,
    ) -> Result<RequestBuilder> {
        self.check_session(session)?;
        Ok(self.request(method, path).header(
            AUTHORIZATION,
            bearer_header(session.session.access_token.expose())?,
        ))
    }

    fn check_session(&self, session: &SyncSession) -> Result<()> {
        if session.origin != self.origin {
            return Err(Error::ContextMismatch);
        }
        session.session.validate()
    }

    fn check_metadata(&self, session: &SyncSession, metadata: &Metadata) -> Result<()> {
        self.check_session(session)?;
        if metadata.origin != self.origin {
            return Err(Error::ContextMismatch);
        }
        if &metadata.value.owner_github_id != session.account_id() {
            return Err(Error::AccountMismatch);
        }
        metadata.value.validate()
    }

    fn check_operation(&self, session: &SyncSession, operation: &PreparedOperation) -> Result<()> {
        self.check_session(session)?;
        if operation.origin != self.origin {
            return Err(Error::ContextMismatch);
        }
        if &operation.owner != session.account_id() {
            return Err(Error::AccountMismatch);
        }
        Ok(())
    }

    /// Loopback HTTP exists only in unit tests, with the same redirect and body policy.
    #[cfg(test)]
    pub(crate) async fn for_test(origin: &str, expected_client_id: &str) -> Result<Self> {
        Self::for_test_with_proxy_policy(
            origin,
            expected_client_id,
            ProxyPolicy::for_origin(origin, false),
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn for_test_with_proxy_policy(
        origin: &str,
        expected_client_id: &str,
        proxy_policy: ProxyPolicy,
    ) -> Result<Self> {
        let origin = parse_origin(origin, true)?;
        let proxy_policy = ProxyPolicy::for_origin(origin.as_str(), proxy_policy.use_system_proxy);
        let client = client_builder(proxy_policy)
            .build()
            .map_err(|_| Error::Transport)?;
        Self::check_capabilities(origin, client, expected_client_id, proxy_policy).await
    }
}

fn client_builder(proxy_policy: ProxyPolicy) -> reqwest::ClientBuilder {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    proxy_policy.apply(
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            // Keep byte limits meaningful even if another workspace dependency enables
            // reqwest compression features through Cargo feature unification.
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .default_headers(headers),
    )
}

fn parse_origin(input: &str, test_loopback: bool) -> Result<Url> {
    if input.chars().any(char::is_whitespace) {
        return Err(Error::InvalidEndpoint);
    }
    let url = Url::parse(input).map_err(|_| Error::InvalidEndpoint)?;
    let https = url.scheme() == "https";
    let loopback = cfg!(test)
        && test_loopback
        && url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1");
    if (!https && !loopback)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidEndpoint);
    }
    Ok(url)
}

fn bearer_header(token: &str) -> Result<HeaderValue> {
    if token.is_empty() || token.len() > 2048 || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(Error::InvalidWire("invalid token"));
    }
    let value = Zeroizing::new(format!("Bearer {token}"));
    let mut header =
        HeaderValue::from_str(&value).map_err(|_| Error::InvalidWire("invalid token"))?;
    header.set_sensitive(true);
    Ok(header)
}

async fn send(request: RequestBuilder) -> Result<Response> {
    let response = request.send().await.map_err(|_| Error::Transport)?;
    if response.status().is_redirection() && response.status() != StatusCode::NOT_MODIFIED {
        return Err(Error::InvalidResponse("redirect refused"));
    }
    Ok(response)
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .ok_or(Error::InvalidResponse("required header missing"))?;
    if values.next().is_some() {
        return Err(Error::InvalidResponse("duplicate response header"));
    }
    value
        .to_str()
        .map_err(|_| Error::InvalidResponse("invalid response header"))
}

fn response_etag(headers: &HeaderMap) -> Result<StrongEtag> {
    StrongEtag::parse(single_header(headers, ETAG.as_str())?)
        .map_err(|_| Error::InvalidResponse("invalid strong ETag"))
}

fn private_response_headers(headers: &HeaderMap, json: bool) -> Result<()> {
    if !single_header(headers, CACHE_CONTROL.as_str())?.eq_ignore_ascii_case("no-store") {
        return Err(Error::InvalidResponse("response must be no-store"));
    }
    if headers.contains_key(CONTENT_ENCODING)
        && !single_header(headers, CONTENT_ENCODING.as_str())?.eq_ignore_ascii_case("identity")
    {
        return Err(Error::InvalidResponse("encoded response refused"));
    }
    if json {
        let value = single_header(headers, CONTENT_TYPE.as_str())?;
        let mut pieces = value.split(';');
        if !pieces
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
        {
            return Err(Error::InvalidResponse("response must be JSON"));
        }
        if let Some(parameter) = pieces.next() {
            let Some((key, value)) = parameter.trim().split_once('=') else {
                return Err(Error::InvalidResponse("invalid JSON content type"));
            };
            if !key.trim().eq_ignore_ascii_case("charset")
                || !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "utf-8" | "\"utf-8\""
                )
                || pieces.next().is_some()
            {
                return Err(Error::InvalidResponse("invalid JSON content type"));
            }
        }
    }
    Ok(())
}

async fn bounded_body(mut response: Response, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    if response.headers().contains_key(CONTENT_LENGTH) {
        let declared = single_header(response.headers(), CONTENT_LENGTH.as_str())?
            .parse::<u64>()
            .map_err(|_| Error::InvalidResponse("invalid content length"))?;
        if declared > u64::try_from(limit).map_err(|_| Error::PayloadTooLarge)? {
            return Err(Error::PayloadTooLarge);
        }
    }
    // Control responses can contain a sync token. Reserve their bounded capacity
    // once so Vec growth does not leave an older allocation containing that token.
    let mut bytes = Zeroizing::new(Vec::with_capacity(limit.min(CONTROL_BODY_LIMIT)));
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Transport)? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(Error::PayloadTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn response_json<T: DeserializeOwned + Validate>(
    response: Response,
    limit: usize,
) -> Result<T> {
    private_response_headers(response.headers(), true)?;
    let body = bounded_body(response, limit).await?;
    parse_wire(&body, limit)
}

async fn success_json<T: DeserializeOwned + Validate>(
    response: Response,
    limit: usize,
) -> Result<T> {
    if response.status() != StatusCode::OK {
        return Err(api_error(response).await);
    }
    response_json(response, limit).await
}

async fn empty_response(mut response: Response) -> Result<()> {
    private_response_headers(response.headers(), false)?;
    // HTTP permits a 304 Content-Length to describe the full representation,
    // even though the 304 itself has no body. Do not mistake it for payload.
    if response.status() == StatusCode::NOT_MODIFIED {
        response.headers_mut().remove(CONTENT_LENGTH);
    }
    let body = bounded_body(response, 0).await?;
    if !body.is_empty() {
        return Err(Error::InvalidResponse("unexpected response body"));
    }
    Ok(())
}

fn serialize_body<T: serde::Serialize>(value: &T, limit: usize) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(value).map_err(|_| Error::InvalidWire("serialization failed"))?;
    if body.len() > limit {
        return Err(Error::PayloadTooLarge);
    }
    Ok(body)
}

async fn api_error(response: Response) -> Error {
    async fn decode(response: Response) -> Result<Error> {
        let status = response.status().as_u16();
        if !(400..600).contains(&status) {
            return Err(Error::InvalidResponse("unexpected response status"));
        }
        let retry_after_seconds = if status == 429 {
            let value = single_header(response.headers(), RETRY_AFTER.as_str())?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(Error::InvalidResponse("invalid retry delay"));
            }
            let seconds = value
                .parse::<u32>()
                .map_err(|_| Error::InvalidResponse("invalid retry delay"))?;
            if !(1..=86400).contains(&seconds) {
                return Err(Error::InvalidResponse("invalid retry delay"));
            }
            Some(seconds)
        } else {
            None
        };
        let envelope: ErrorEnvelope = response_json(response, CONTROL_BODY_LIMIT).await?;
        if !error_status_matches(envelope.error.code, status) {
            return Err(Error::InvalidResponse("error code and status mismatch"));
        }
        Ok(Error::Api {
            status,
            code: envelope.error.code,
            retry_after_seconds,
            current_revision: envelope.error.current_revision,
        })
    }
    match decode(response).await {
        Ok(error) | Err(error) => error,
    }
}

fn error_status_matches(code: RemoteErrorCode, status: u16) -> bool {
    match code {
        RemoteErrorCode::InvalidRequest
        | RemoteErrorCode::UnsupportedProtocol
        | RemoteErrorCode::InvalidCryptoHeader => status == 400,
        RemoteErrorCode::Unauthenticated | RemoteErrorCode::SessionExpired => status == 401,
        RemoteErrorCode::WrongOauthApp | RemoteErrorCode::OwnerMismatch => status == 403,
        RemoteErrorCode::VaultEmpty | RemoteErrorCode::VaultDeleted => matches!(status, 404 | 409),
        RemoteErrorCode::OperationNotFound => status == 404,
        RemoteErrorCode::IdempotencyKeyReused
        | RemoteErrorCode::KeyEpochChanged
        | RemoteErrorCode::VaultExists
        | RemoteErrorCode::RevisionExhausted => status == 409,
        RemoteErrorCode::RevisionConflict => status == 412,
        RemoteErrorCode::PayloadTooLarge => status == 413,
        RemoteErrorCode::UnsupportedMediaType => status == 415,
        RemoteErrorCode::PreconditionRequired => status == 428,
        RemoteErrorCode::RateLimited => status == 429,
        RemoteErrorCode::InternalError => status == 500,
        RemoteErrorCode::ServiceUnavailable => status == 503,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
    };

    use super::*;

    const CLIENT_ID: &str = "test-openless-oauth-app";
    const OWNER: &str = "12345";
    const VAULT: &str = "be406ca5-37fa-4b26-9a9a-74c1aad48eae";
    const KEY: &str = "2b1889ab-7278-48e7-8d2f-284195de2484";
    const OLD_OPERATION: &str = "e1d8c32e-d209-4e56-8735-bc66b8684c71";
    const NEW_OPERATION: &str = "f1d8c32e-d209-4e56-8735-bc66b8684c71";

    struct Reply {
        status: &'static str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        chunked: bool,
        disconnect: bool,
    }

    impl Reply {
        fn json(status: &'static str, value: Value) -> Self {
            Self::raw(status, serde_json::to_vec(&value).unwrap())
        }

        fn raw(status: &'static str, body: Vec<u8>) -> Self {
            Self {
                status,
                headers: vec![
                    ("Cache-Control".into(), "no-store".into()),
                    ("Content-Type".into(), "application/json".into()),
                ],
                body,
                chunked: false,
                disconnect: false,
            }
        }

        fn header(mut self, key: &str, value: &str) -> Self {
            self.headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case(key));
            self.headers.push((key.into(), value.into()));
            self
        }

        fn without_header(mut self, key: &str) -> Self {
            self.headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case(key));
            self
        }

        fn chunked(mut self) -> Self {
            self.chunked = true;
            self
        }

        fn disconnect() -> Self {
            let mut reply = Self::raw("200 OK", Vec::new());
            reply.disconnect = true;
            reply
        }
    }

    #[derive(Debug)]
    struct RecordedRequest {
        method: String,
        path: String,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    }

    struct FakeServer {
        origin: String,
        task: Option<JoinHandle<Vec<RecordedRequest>>>,
    }

    impl Drop for FakeServer {
        fn drop(&mut self) {
            if let Some(task) = &self.task {
                task.abort();
            }
        }
    }

    impl FakeServer {
        async fn start(replies: Vec<Reply>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                let mut requests = Vec::new();
                for reply in replies {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut wire = Vec::new();
                    let mut buffer = [0_u8; 8192];
                    let header_end = loop {
                        let size = socket.read(&mut buffer).await.unwrap();
                        assert_ne!(size, 0, "request closed before headers");
                        wire.extend_from_slice(&buffer[..size]);
                        if let Some(index) = wire.windows(4).position(|part| part == b"\r\n\r\n") {
                            break index + 4;
                        }
                    };
                    let head = std::str::from_utf8(&wire[..header_end]).unwrap();
                    let mut lines = head.split("\r\n");
                    let mut request_line = lines.next().unwrap().split_whitespace();
                    let method = request_line.next().unwrap().to_owned();
                    let path = request_line.next().unwrap().to_owned();
                    let headers: BTreeMap<String, String> = lines
                        .filter_map(|line| line.split_once(':'))
                        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
                        .collect();
                    let body_size = headers
                        .get("content-length")
                        .map_or(0, |value| value.parse::<usize>().unwrap());
                    while wire.len() < header_end + body_size {
                        let size = socket.read(&mut buffer).await.unwrap();
                        assert_ne!(size, 0, "request closed before body");
                        wire.extend_from_slice(&buffer[..size]);
                    }
                    requests.push(RecordedRequest {
                        method,
                        path,
                        headers,
                        body: wire[header_end..header_end + body_size].to_vec(),
                    });
                    if reply.disconnect {
                        continue;
                    }
                    let mut response =
                        format!("HTTP/1.1 {}\r\nConnection: close\r\n", reply.status);
                    for (key, value) in &reply.headers {
                        response.push_str(&format!("{key}: {value}\r\n"));
                    }
                    if reply.chunked {
                        response.push_str("Transfer-Encoding: chunked\r\n");
                    } else if !reply
                        .headers
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case("Content-Length"))
                    {
                        response.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
                    }
                    response.push_str("\r\n");
                    if socket.write_all(response.as_bytes()).await.is_err() {
                        continue;
                    }
                    if reply.chunked {
                        for chunk in reply.body.chunks(4096) {
                            if socket
                                .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                                .await
                                .is_err()
                                || socket.write_all(chunk).await.is_err()
                                || socket.write_all(b"\r\n").await.is_err()
                            {
                                break;
                            }
                        }
                        let _ = socket.write_all(b"0\r\n\r\n").await;
                    } else {
                        let _ = socket.write_all(&reply.body).await;
                    }
                }
                requests
            });
            Self {
                origin,
                task: Some(task),
            }
        }

        async fn finish(mut self) -> Vec<RecordedRequest> {
            let mut task = self.task.take().unwrap();
            match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
                Ok(result) => result.unwrap(),
                Err(_) => {
                    task.abort();
                    panic!("fake HTTP server did not receive all expected requests");
                }
            }
        }
    }

    fn capabilities() -> Value {
        json!({
            "protocolVersion": 1,
            "cryptoProfile": "argon2id-xchacha20poly1305-v1",
            "githubClientId": CLIENT_ID,
            "maxHttpBodyBytes": 25165824,
            "maxCiphertextBytes": 16777232,
            "maxPlaintextJsonBytes": 15728640,
            "idempotencyRetentionSeconds": 604800,
            "maxBackupRetentionDays": 30
        })
    }

    fn caps_reply() -> Reply {
        Reply::json("200 OK", capabilities())
    }

    fn auth(owner: &str, token_character: char) -> Value {
        json!({
            "protocolVersion": 1,
            "accessToken": token_character.to_string().repeat(43),
            "tokenType": "Bearer",
            "expiresIn": 900,
            "account": { "githubId": owner, "login": "fixture-user" }
        })
    }

    fn empty_metadata() -> Value {
        json!({
            "protocolVersion": 1, "state": "empty", "ownerGithubId": OWNER,
            "revision": "0", "vaultId": null, "keyId": null, "updatedAt": null,
            "payloadSchemaVersion": null, "ciphertextBytes": 0,
            "ciphertextSha256": null, "lastOperationId": null
        })
    }

    fn snapshot() -> Value {
        let ciphertext = vec![0_u8; 65552];
        json!({
            "protocolVersion": 1, "payloadSchemaVersion": 1, "ownerGithubId": OWNER,
            "vaultId": VAULT, "keyId": KEY, "baseRevision": "7", "revision": "8",
            "operationId": OLD_OPERATION, "kind": "snapshot",
            "cryptoProfile": "argon2id-xchacha20poly1305-v1",
            "kdf": { "name": "argon2id", "version": 19, "memoryKiB": 65536,
                "iterations": 3, "parallelism": 4, "salt": URL_SAFE_NO_PAD.encode([0_u8; 16]) },
            "aead": "xchacha20poly1305-ietf", "codec": "json-pad64k-v1",
            "nonce": URL_SAFE_NO_PAD.encode([0_u8; 24]),
            "ciphertextSha256": format!("{:x}", Sha256::digest(&ciphertext)),
            "ciphertext": URL_SAFE_NO_PAD.encode(ciphertext)
        })
    }

    fn active_metadata() -> Value {
        json!({
            "protocolVersion": 1, "state": "active", "ownerGithubId": OWNER,
            "revision": "8", "vaultId": VAULT, "keyId": KEY,
            "updatedAt": "2026-09-23T12:00:00Z", "payloadSchemaVersion": 1,
            "ciphertextBytes": 65552, "ciphertextSha256": snapshot()["ciphertextSha256"],
            "lastOperationId": OLD_OPERATION
        })
    }

    fn next_snapshot() -> Value {
        let mut upload = snapshot();
        upload["baseRevision"] = json!("8");
        upload["revision"] = json!("9");
        upload["operationId"] = json!(NEW_OPERATION);
        upload["nonce"] = json!(URL_SAFE_NO_PAD.encode([1_u8; 24]));
        upload
    }

    fn receipt(kind: &str) -> Value {
        json!({
            "operationId": NEW_OPERATION, "status": "committed", "kind": kind,
            "committedRevision": "9", "committedAt": "2026-09-23T12:00:00Z", "vaultId": VAULT,
            "ciphertextSha256": if kind == "delete" { Value::Null } else { snapshot()["ciphertextSha256"].clone() }
        })
    }

    fn error_envelope(code: &str) -> Value {
        json!({ "error": { "code": code, "message": "fixture error", "requestId": OLD_OPERATION } })
    }

    fn typed<T: DeserializeOwned + Validate>(value: &Value) -> T {
        parse_wire(&serde_json::to_vec(value).unwrap(), HTTP_BODY_LIMIT).unwrap()
    }

    fn session(transport: &Transport, owner: &str) -> SyncSession {
        SyncSession {
            origin: transport.origin.clone(),
            session: typed(&auth(owner, 'a')),
        }
    }

    fn metadata(transport: &Transport, value: Value) -> Metadata {
        Metadata {
            origin: transport.origin.clone(),
            value: typed(&value),
            etag: StrongEtag::parse("\"opaque-etag-8\"").unwrap(),
        }
    }

    fn prepared_upload(transport: &Transport, session: &SyncSession) -> PreparedOperation {
        transport
            .prepare_upload(
                session,
                &metadata(transport, active_metadata()),
                &typed(&next_snapshot()),
                Some(&typed(&snapshot())),
            )
            .unwrap()
    }

    #[tokio::test]
    async fn production_constructor_rejects_insecure_or_ambiguous_origins() {
        for origin in [
            "http://127.0.0.1:9",
            "ftp://sync.example",
            "https://user:secret@sync.example",
            "https://sync.example/path",
            "https://sync.example?token=secret",
            "https://sync.example/#secret",
            " https://sync.example",
            "https://sync.\nexample",
        ] {
            assert!(matches!(
                Transport::new(origin, CLIENT_ID, ProxyPolicy::for_origin(origin, false)).await,
                Err(Error::InvalidEndpoint)
            ));
        }
        assert!(parse_origin("https://sync.example:9443", false).is_ok());
        assert!(parse_origin("http://example.com", true).is_err());
    }

    #[test]
    fn proxy_policy_records_one_setting_and_the_origin_bypass_decision() {
        let remote = "https://sync.example:9443";
        let enabled = ProxyPolicy::for_origin(remote, true);
        let disabled = ProxyPolicy::for_origin(remote, false);
        assert!(!enabled.bypass_proxy);
        assert!(disabled.bypass_proxy);
        assert_ne!(enabled, disabled);
        for origin in [
            "https://localhost:9443",
            "https://127.0.0.1:9443",
            "https://[::1]:9443",
        ] {
            for setting in [false, true] {
                let policy = ProxyPolicy::for_origin(origin, setting);
                assert_eq!(policy.use_system_proxy, setting);
                assert!(policy.bypass_proxy);
            }
        }
    }

    #[tokio::test]
    async fn loopback_transport_records_policy_without_allowing_a_proxy() {
        for setting in [false, true] {
            let server = FakeServer::start(vec![caps_reply()]).await;
            // Even a policy derived for a public origin is normalized to the
            // actual constructor endpoint before client creation and recording.
            let transport = Transport::for_test_with_proxy_policy(
                &server.origin,
                CLIENT_ID,
                ProxyPolicy::for_origin("https://sync.example", setting),
            )
            .await
            .unwrap();
            assert_eq!(
                transport.proxy_policy(),
                ProxyPolicy::for_origin(&server.origin, setting)
            );
            assert!(transport.proxy_policy().bypass_proxy);
            assert_eq!(server.finish().await.len(), 1);
        }
    }

    #[tokio::test]
    async fn direct_policy_clears_explicit_proxy_without_changing_process_environment() {
        for setting in [false, true] {
            let direct = FakeServer::start(vec![caps_reply()]).await;
            let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
            let seeded = client_builder(ProxyPolicy::for_origin("https://sync.example", true))
                .proxy(reqwest::Proxy::all(proxy_url).unwrap());
            let policy = ProxyPolicy::for_origin(&direct.origin, setting);
            let response = policy
                .apply(seeded)
                .build()
                .unwrap()
                .get(format!("{}/v1/capabilities", direct.origin))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(direct.finish().await.len(), 1);
            assert!(
                tokio::time::timeout(Duration::from_millis(100), proxy.accept())
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn system_policy_retains_a_controlled_proxy_without_external_requests() {
        let proxy = FakeServer::start(vec![caps_reply()]).await;
        let direct =
            FakeServer::start(vec![Reply::raw("503 Service Unavailable", Vec::new())]).await;
        let address: std::net::SocketAddr =
            direct.origin.trim_start_matches("http://").parse().unwrap();
        let origin = format!("http://sync.invalid:{}", address.port());
        let policy = ProxyPolicy::for_origin(&origin, true);
        // Apply the production policy after seeding the controlled proxy. The
        // DNS override also keeps a faulty direct branch inside this fixture.
        let seeded = client_builder(ProxyPolicy::for_origin(&origin, false))
            .proxy(reqwest::Proxy::all(&proxy.origin).unwrap())
            .resolve("sync.invalid", address);
        let client = policy.apply(seeded).build().unwrap();
        assert_eq!(
            client
                .get(format!("{origin}/v1/capabilities"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let requests = proxy.finish().await;
        assert_eq!(requests[0].path, format!("{origin}/v1/capabilities"));
    }

    #[tokio::test]
    async fn capabilities_are_checked_before_auth_and_auth_account_is_bound() {
        let server =
            FakeServer::start(vec![caps_reply(), Reply::json("200 OK", auth(OWNER, 'a'))]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = transport
            .exchange("github-fixture-token", &GithubId::parse(OWNER).unwrap())
            .await
            .unwrap();
        assert_eq!(session.account_id().as_str(), OWNER);
        assert_eq!(session.expires_in(), 900);
        let requests = server.finish().await;
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/v1/capabilities");
        assert!(!requests[0].headers.contains_key("authorization"));
        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].path, "/v1/auth/github");
        assert_eq!(
            requests[1].headers["authorization"],
            "Bearer github-fixture-token"
        );
        assert!(requests[1].body.is_empty());
        assert_eq!(requests[1].headers["accept-encoding"], "identity");
    }

    #[tokio::test]
    async fn wrong_oauth_capability_or_duplicate_wire_key_prevents_authentication() {
        let mut wrong = capabilities();
        wrong["githubClientId"] = json!("another-app");
        let duplicate = serde_json::to_string(&capabilities()).unwrap().replacen(
            '{',
            "{\"protocolVersion\":1,",
            1,
        );
        for reply in [
            Reply::json("200 OK", wrong),
            Reply::raw("200 OK", duplicate.into_bytes()),
        ] {
            let server = FakeServer::start(vec![reply]).await;
            assert!(Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .is_err());
            let requests = server.finish().await;
            assert_eq!(requests.len(), 1);
            assert!(!requests[0].headers.contains_key("authorization"));
        }
    }

    #[tokio::test]
    async fn rejects_auth_response_from_another_account() {
        let server =
            FakeServer::start(vec![caps_reply(), Reply::json("200 OK", auth("999", 'a'))]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        assert!(matches!(
            transport
                .exchange("github-fixture-token", &GithubId::parse(OWNER).unwrap())
                .await,
            Err(Error::AccountMismatch)
        ));
        server.finish().await;
    }

    #[tokio::test]
    async fn refuses_redirect_without_following_or_exposing_location() {
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::raw("307 Temporary Redirect", b"secret body".to_vec())
                .header("Location", "http://127.0.0.1:9/leaked-token"),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let error = transport
            .metadata(&session(&transport, OWNER), None)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::InvalidResponse("redirect refused")));
        assert!(!format!("{error:?}").contains("leaked-token"));
        assert_eq!(server.finish().await.len(), 2);
    }

    #[tokio::test]
    async fn metadata_preserves_etag_and_validates_conditional_304() {
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::json("200 OK", empty_metadata()).header("ETag", "\"opaque-zero\""),
            Reply::raw("304 Not Modified", Vec::new())
                .without_header("Content-Type")
                .header("Content-Length", "240")
                .header("ETag", "\"opaque-zero\""),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = session(&transport, OWNER);
        let MetadataResult::Modified(current) = transport.metadata(&session, None).await.unwrap()
        else {
            panic!("expected metadata")
        };
        assert_eq!(current.value().state, VaultState::Empty);
        assert_eq!(current.etag().as_str(), "\"opaque-zero\"");
        assert!(matches!(
            transport
                .metadata(&session, Some(current.as_ref()))
                .await
                .unwrap(),
            MetadataResult::NotModified
        ));
        let requests = server.finish().await;
        assert_eq!(requests[2].headers["if-none-match"], "\"opaque-zero\"");
    }

    #[tokio::test]
    async fn metadata_rejects_unbound_304_and_never_maps_404_to_empty() {
        for reply in [
            Reply::raw("304 Not Modified", Vec::new()).header("ETag", "\"opaque-zero\""),
            Reply::json("404 Not Found", error_envelope("vault_empty")),
            Reply::json("200 OK", empty_metadata()).header("ETag", "W/\"weak\""),
        ] {
            let server = FakeServer::start(vec![caps_reply(), reply]).await;
            let transport = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            assert!(matches!(
                transport.metadata(&session(&transport, OWNER), None).await,
                Err(Error::InvalidResponse(_))
            ));
            server.finish().await;
        }
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::raw("304 Not Modified", Vec::new()).header("ETag", "\"different\""),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        assert!(matches!(
            transport
                .metadata(
                    &session(&transport, OWNER),
                    Some(&metadata(&transport, active_metadata()))
                )
                .await,
            Err(Error::InvalidResponse(_))
        ));
        server.finish().await;
    }

    #[tokio::test]
    async fn rejects_non_json_non_private_encoded_and_duplicate_headers() {
        let mut duplicate = Reply::json("200 OK", empty_metadata()).header("ETag", "\"tag\"");
        duplicate.headers.push(("ETag".into(), "\"second\"".into()));
        let good = || Reply::json("200 OK", empty_metadata()).header("ETag", "\"tag\"");
        for reply in [
            good().without_header("Cache-Control"),
            good().header("Cache-Control", "max-age=60"),
            good().header("Cache-Control", "no-store, public"),
            good().header("Content-Type", "text/html"),
            good().header("Content-Type", "application/json; charset=utf-16"),
            good().header("Content-Encoding", "gzip"),
            duplicate,
        ] {
            let server = FakeServer::start(vec![caps_reply(), reply]).await;
            let transport = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            assert!(matches!(
                transport.metadata(&session(&transport, OWNER), None).await,
                Err(Error::InvalidResponse(_))
            ));
            server.finish().await;
        }
    }

    #[tokio::test]
    async fn snapshot_is_bound_to_metadata_account_and_strong_etag() {
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::json("200 OK", snapshot()).header("ETag", "\"opaque-etag-8\""),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let downloaded = transport
            .snapshot(
                &session(&transport, OWNER),
                &metadata(&transport, active_metadata()),
            )
            .await
            .unwrap();
        assert_eq!(downloaded.revision.get(), 8);
        let requests = server.finish().await;
        assert_eq!(requests[1].headers["if-match"], "\"opaque-etag-8\"");
        for (field, value) in [
            ("ownerGithubId", json!("999")),
            ("vaultId", json!(KEY)),
            ("keyId", json!(VAULT)),
            ("operationId", json!(NEW_OPERATION)),
            ("ciphertextSha256", json!("0".repeat(64))),
        ] {
            let mut bad = snapshot();
            bad[field] = value;
            let server = FakeServer::start(vec![
                caps_reply(),
                Reply::json("200 OK", bad).header("ETag", "\"opaque-etag-8\""),
            ])
            .await;
            let transport = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            assert!(transport
                .snapshot(
                    &session(&transport, OWNER),
                    &metadata(&transport, active_metadata())
                )
                .await
                .is_err());
            server.finish().await;
        }
    }

    #[tokio::test]
    async fn lost_mutation_response_can_be_retried_with_identical_bytes_after_token_renewal() {
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::disconnect(),
            Reply::json("200 OK", auth(OWNER, 'b')),
            Reply::json("200 OK", receipt("snapshot")).header("Idempotency-Replayed", "true"),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let first_session = session(&transport, OWNER);
        let operation = prepared_upload(&transport, &first_session);
        let exact_bytes = operation.body_bytes().to_vec();
        assert!(matches!(
            transport.submit(&first_session, &operation).await,
            Err(Error::Transport)
        ));
        let renewed = transport
            .exchange("github-fixture-token", &GithubId::parse(OWNER).unwrap())
            .await
            .unwrap();
        let result = transport.submit(&renewed, &operation).await.unwrap();
        assert!(result.replayed);
        assert_eq!(result.receipt.committed_revision.get(), 9);
        let requests = server.finish().await;
        for index in [1, 3] {
            assert_eq!(requests[index].method, "PUT");
            assert_eq!(requests[index].path, "/v1/me/vault/snapshot");
            assert_eq!(requests[index].body, exact_bytes);
            assert_eq!(
                requests[index].headers["if-match"],
                operation.if_match().as_str()
            );
            assert_eq!(
                requests[index].headers["idempotency-key"],
                operation.operation_id().as_str()
            );
        }
        assert_ne!(
            requests[1].headers["authorization"],
            requests[3].headers["authorization"]
        );
    }

    #[tokio::test]
    async fn prepared_operations_cannot_cross_account_or_service_boundaries() {
        let server = FakeServer::start(vec![caps_reply()]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session_a = session(&transport, OWNER);
        let operation = prepared_upload(&transport, &session_a);
        let session_b = session(&transport, "999");
        assert!(matches!(
            transport.submit(&session_b, &operation).await,
            Err(Error::AccountMismatch)
        ));
        assert!(matches!(
            transport
                .metadata(&session_b, Some(&metadata(&transport, active_metadata())))
                .await,
            Err(Error::AccountMismatch)
        ));
        let mut another_origin = transport.clone();
        another_origin.origin = "https://other.example/".parse().unwrap();
        assert!(matches!(
            another_origin.submit(&session_a, &operation).await,
            Err(Error::ContextMismatch)
        ));
        assert_eq!(server.finish().await.len(), 1);
    }

    #[tokio::test]
    async fn active_upload_checks_key_epoch_nonce_and_base_revision_before_sending() {
        let server = FakeServer::start(vec![caps_reply()]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = session(&transport, OWNER);
        let current = metadata(&transport, active_metadata());
        let previous = typed(&snapshot());
        assert!(transport
            .prepare_upload(&session, &current, &typed(&next_snapshot()), None)
            .is_err());
        for (field, value) in [
            ("keyId", json!(VAULT)),
            ("nonce", snapshot()["nonce"].clone()),
            ("vaultId", json!(KEY)),
        ] {
            let mut upload = next_snapshot();
            upload[field] = value;
            assert!(transport
                .prepare_upload(&session, &current, &typed(&upload), Some(&previous))
                .is_err());
        }
        let mut wrong_salt = next_snapshot();
        wrong_salt["kdf"]["salt"] = json!(URL_SAFE_NO_PAD.encode([1_u8; 16]));
        assert!(transport
            .prepare_upload(&session, &current, &typed(&wrong_salt), Some(&previous))
            .is_err());
        let mut changed = next_snapshot();
        changed["kind"] = json!("password_change");
        assert!(transport
            .prepare_upload(&session, &current, &typed(&changed), Some(&previous))
            .is_err());
        changed["keyId"] = json!(VAULT);
        changed["kdf"]["salt"] = json!(URL_SAFE_NO_PAD.encode([1_u8; 16]));
        assert!(transport
            .prepare_upload(&session, &current, &typed(&changed), Some(&previous))
            .is_ok());
        let mut create = snapshot();
        create["kind"] = json!("create");
        create["baseRevision"] = json!("0");
        create["revision"] = json!("1");
        assert!(transport
            .prepare_upload(
                &session,
                &metadata(&transport, empty_metadata()),
                &typed(&create),
                None
            )
            .is_ok());
        server.finish().await;
    }

    #[tokio::test]
    async fn delete_and_receipt_reconciliation_preserve_the_original_operation() {
        let pending =
            json!({"operationId":NEW_OPERATION,"status":"pending","retryAfterSeconds":30});
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::json("200 OK", receipt("delete")).header("Idempotency-Replayed", "false"),
            Reply::json("202 Accepted", pending),
            Reply::json("404 Not Found", error_envelope("operation_not_found")),
            Reply::json("200 OK", receipt("delete")),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = session(&transport, OWNER);
        let operation = transport
            .prepare_delete(
                &session,
                &metadata(&transport, active_metadata()),
                UuidV4::parse(NEW_OPERATION).unwrap(),
            )
            .unwrap();
        let result = transport.submit(&session, &operation).await.unwrap();
        assert!(!result.replayed);
        assert_eq!(result.receipt.kind, OperationKind::Delete);
        assert!(matches!(
            transport.operation(&session, &operation).await.unwrap(),
            OperationStatus::Pending(_)
        ));
        assert!(matches!(
            transport.operation(&session, &operation).await.unwrap(),
            OperationStatus::NotFound
        ));
        assert!(matches!(
            transport.operation(&session, &operation).await.unwrap(),
            OperationStatus::Committed(_)
        ));
        let requests = server.finish().await;
        assert_eq!(requests[1].method, "DELETE");
        assert_eq!(requests[1].headers["idempotency-key"], NEW_OPERATION);
        assert_eq!(requests[1].body, operation.body_bytes());
        for request in &requests[2..] {
            assert_eq!(request.path, format!("/v1/me/operations/{NEW_OPERATION}"));
        }
    }

    #[tokio::test]
    async fn every_mutation_receipt_field_is_checked_against_the_prepared_request() {
        for (field, value) in [
            ("operationId", json!(OLD_OPERATION)),
            ("kind", json!("create")),
            ("committedRevision", json!("10")),
            ("vaultId", json!(KEY)),
            ("ciphertextSha256", json!("0".repeat(64))),
        ] {
            let mut bad = receipt("snapshot");
            bad[field] = value;
            let server = FakeServer::start(vec![
                caps_reply(),
                Reply::json("200 OK", bad).header("Idempotency-Replayed", "false"),
            ])
            .await;
            let transport = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            let session = session(&transport, OWNER);
            let operation = prepared_upload(&transport, &session);
            assert!(matches!(
                transport.submit(&session, &operation).await,
                Err(Error::InvalidResponse(_))
            ));
            server.finish().await;
        }
    }

    #[tokio::test]
    async fn control_and_snapshot_responses_are_limited_while_streaming() {
        for limit in [CONTROL_BODY_LIMIT, HTTP_BODY_LIMIT] {
            for length in [limit, limit + 1] {
                let server =
                    FakeServer::start(vec![Reply::raw("200 OK", vec![b' '; length]).chunked()])
                        .await;
                let response = client_builder(ProxyPolicy::for_origin(&server.origin, false))
                    .no_proxy()
                    .build()
                    .unwrap()
                    .get(&server.origin)
                    .send()
                    .await
                    .unwrap();
                let result = bounded_body(response, limit).await;
                if length == limit {
                    assert_eq!(result.unwrap().len(), limit);
                } else {
                    assert!(matches!(result, Err(Error::PayloadTooLarge)));
                }
                server.finish().await;
            }
        }
        let server = FakeServer::start(vec![
            Reply::raw("200 OK", Vec::new()).header("Content-Length", "25165825")
        ])
        .await;
        let response = client_builder(ProxyPolicy::for_origin(&server.origin, false))
            .no_proxy()
            .build()
            .unwrap()
            .get(&server.origin)
            .send()
            .await
            .unwrap();
        assert!(matches!(
            bounded_body(response, HTTP_BODY_LIMIT).await,
            Err(Error::PayloadTooLarge)
        ));
        server.finish().await;
    }

    #[tokio::test]
    async fn errors_retain_only_allowlisted_code_and_validated_retry_delay() {
        let marker = "SENSITIVE_BODY_TOKEN_https://secret.example";
        let mut remote = error_envelope("rate_limited");
        remote["error"]["message"] = json!(marker);
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::json("429 Too Many Requests", remote).header("Retry-After", "42"),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let error = transport
            .metadata(&session(&transport, OWNER), None)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Api {
                status: 429,
                code: RemoteErrorCode::RateLimited,
                retry_after_seconds: Some(42),
                ..
            }
        ));
        assert!(!format!("{error:?} {error}").contains(marker));
        server.finish().await;
        for (status, code, delay) in [
            ("429 Too Many Requests", "rate_limited", "0"),
            ("429 Too Many Requests", "rate_limited", "86401"),
            ("429 Too Many Requests", "rate_limited", "tomorrow"),
            ("503 Service Unavailable", "rate_limited", "42"),
        ] {
            let server = FakeServer::start(vec![
                caps_reply(),
                Reply::json(status, error_envelope(code)).header("Retry-After", delay),
            ])
            .await;
            let transport = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            assert!(matches!(
                transport.metadata(&session(&transport, OWNER), None).await,
                Err(Error::InvalidResponse(_))
            ));
            server.finish().await;
        }
    }

    #[tokio::test]
    async fn logout_accepts_only_no_body_success_or_validated_already_revoked_error() {
        for reply in [
            Reply::raw("204 No Content", Vec::new()),
            Reply::json("401 Unauthorized", error_envelope("session_expired")),
        ] {
            let server = FakeServer::start(vec![caps_reply(), reply]).await;
            let transport = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            transport
                .revoke_session(session(&transport, OWNER))
                .await
                .unwrap();
            let requests = server.finish().await;
            assert_eq!(requests[1].method, "DELETE");
            assert_eq!(requests[1].path, "/v1/auth/session");
            assert!(requests[1].body.is_empty());
        }
    }

    #[tokio::test]
    async fn snapshot_rejects_another_revision_etag_or_owner_before_returning_data() {
        for (snapshot_value, etag) in [
            (next_snapshot(), "\"opaque-etag-8\""),
            (snapshot(), "\"another-etag\""),
        ] {
            let server = FakeServer::start(vec![
                caps_reply(),
                Reply::json("200 OK", snapshot_value).header("ETag", etag),
            ])
            .await;
            let transport = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            assert!(transport
                .snapshot(
                    &session(&transport, OWNER),
                    &metadata(&transport, active_metadata())
                )
                .await
                .is_err());
            server.finish().await;
        }
        let mut foreign = empty_metadata();
        foreign["ownerGithubId"] = json!("999");
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::json("200 OK", foreign).header("ETag", "\"foreign\""),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        assert!(matches!(
            transport.metadata(&session(&transport, OWNER), None).await,
            Err(Error::AccountMismatch)
        ));
        server.finish().await;
    }

    #[tokio::test]
    async fn missing_replay_header_and_mismatched_pending_receipt_stay_unconfirmed() {
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::json("200 OK", receipt("snapshot")),
            Reply::json(
                "202 Accepted",
                json!({"operationId":OLD_OPERATION,"status":"pending","retryAfterSeconds":1}),
            ),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = session(&transport, OWNER);
        let operation = prepared_upload(&transport, &session);
        assert!(matches!(
            transport.submit(&session, &operation).await,
            Err(Error::InvalidResponse(_))
        ));
        assert!(matches!(
            transport.operation(&session, &operation).await,
            Err(Error::InvalidResponse(_))
        ));
        assert_eq!(operation.operation_id().as_str(), NEW_OPERATION);
        server.finish().await;
    }

    #[tokio::test]
    async fn error_response_uses_the_control_limit_even_on_the_snapshot_route() {
        let server = FakeServer::start(vec![
            caps_reply(),
            Reply::raw(
                "503 Service Unavailable",
                vec![b' '; CONTROL_BODY_LIMIT + 1],
            )
            .chunked(),
        ])
        .await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        assert!(matches!(
            transport
                .snapshot(
                    &session(&transport, OWNER),
                    &metadata(&transport, active_metadata())
                )
                .await,
            Err(Error::PayloadTooLarge)
        ));
        server.finish().await;
    }

    #[tokio::test]
    async fn deleted_vault_recreation_keeps_tombstone_revision_and_requires_a_new_vault() {
        let server = FakeServer::start(vec![caps_reply()]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = session(&transport, OWNER);
        let mut deleted = active_metadata();
        deleted["state"] = json!("deleted");
        for field in ["keyId", "payloadSchemaVersion", "ciphertextSha256"] {
            deleted[field] = Value::Null;
        }
        deleted["ciphertextBytes"] = json!(0);
        let tombstone = metadata(&transport, deleted);
        let mut create = next_snapshot();
        create["kind"] = json!("create");
        assert!(transport
            .prepare_upload(&session, &tombstone, &typed(&create), None)
            .is_err());
        create["vaultId"] = json!(KEY);
        assert!(transport
            .prepare_upload(&session, &tombstone, &typed(&create), None)
            .is_ok());
        create["baseRevision"] = json!("0");
        create["revision"] = json!("1");
        assert!(transport
            .prepare_upload(&session, &tombstone, &typed(&create), None)
            .is_err());
        assert!(transport
            .prepare_delete(&session, &tombstone, UuidV4::parse(NEW_OPERATION).unwrap())
            .is_err());
        let mut exhausted = active_metadata();
        exhausted["revision"] = json!(u64::MAX.to_string());
        assert!(matches!(
            transport.prepare_delete(
                &session,
                &metadata(&transport, exhausted),
                UuidV4::parse(NEW_OPERATION).unwrap()
            ),
            Err(Error::RevisionExhausted)
        ));
        server.finish().await;
    }

    #[tokio::test]
    async fn debug_output_and_malformed_bearer_values_do_not_expose_credentials() {
        let server = FakeServer::start(vec![caps_reply()]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = session(&transport, OWNER);
        let operation = prepared_upload(&transport, &session);
        let text = format!("{transport:?} {session:?} {operation:?}");
        assert!(!text.contains(&"a".repeat(43)));
        assert!(!text.contains(&server.origin));
        assert!(!text.contains(snapshot()["ciphertext"].as_str().unwrap()));
        for token in ["", "secret\nvalue", "secret value"] {
            let error = bearer_header(token).unwrap_err();
            assert!(!format!("{error:?} {error}").contains("secret"));
        }
        assert!(bearer_header("fixture-token").unwrap().is_sensitive());
        server.finish().await;
    }

    #[tokio::test]
    async fn pending_records_preserve_original_upload_and_delete_bytes_after_restart() {
        for kind in ["snapshot", "delete"] {
            let server = FakeServer::start(vec![
                caps_reply(),
                Reply::disconnect(),
                caps_reply(),
                Reply::json("200 OK", auth(OWNER, 'b')),
                Reply::json("200 OK", receipt(kind)).header("Idempotency-Replayed", "true"),
            ])
            .await;
            let transport = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            let first_session = session(&transport, OWNER);
            let mut operation = if kind == "snapshot" {
                prepared_upload(&transport, &first_session)
            } else {
                transport
                    .prepare_delete(
                        &first_session,
                        &metadata(&transport, active_metadata()),
                        UuidV4::parse(NEW_OPERATION).unwrap(),
                    )
                    .unwrap()
            };
            // Simulate bytes produced by a previous client serializer. Restoring
            // must not reserialize this valid, differently ordered/formatted JSON.
            let original_value: Value = serde_json::from_slice(operation.body_bytes()).unwrap();
            operation.body = format!(
                "\n\t{}\n",
                serde_json::to_string_pretty(&original_value).unwrap()
            )
            .into_bytes();
            operation.if_match = StrongEtag::parse("\"opaque-\\tag-8\"").unwrap();
            let original_body = operation.body_bytes().to_vec();
            let original_etag = operation.if_match().as_str().to_owned();
            let record = operation.to_pending_record().unwrap();
            assert!(!std::str::from_utf8(&record)
                .unwrap()
                .contains(&"a".repeat(43)));
            assert!(matches!(
                transport.submit(&first_session, &operation).await,
                Err(Error::Transport)
            ));
            drop(operation);
            drop(first_session);
            drop(transport);

            let restarted = Transport::for_test(&server.origin, CLIENT_ID)
                .await
                .unwrap();
            let renewed = restarted
                .exchange("github-fixture-token", &GithubId::parse(OWNER).unwrap())
                .await
                .unwrap();
            let restored = restarted.restore_pending(&renewed, &record).unwrap();
            assert_eq!(restored.body_bytes(), original_body);
            assert_eq!(restored.if_match().as_str(), original_etag);
            assert_eq!(restored.operation_id().as_str(), NEW_OPERATION);
            let accepted = restarted.submit(&renewed, &restored).await.unwrap();
            assert!(accepted.replayed);
            let requests = server.finish().await;
            for index in [1, 4] {
                assert_eq!(requests[index].body, original_body);
                assert_eq!(requests[index].headers["if-match"], original_etag);
                assert_eq!(requests[index].headers["idempotency-key"], NEW_OPERATION);
                assert_eq!(
                    requests[index].method,
                    if kind == "snapshot" { "PUT" } else { "DELETE" }
                );
            }
            assert_ne!(
                requests[1].headers["authorization"],
                requests[4].headers["authorization"]
            );
            assert_eq!(requests[2].path, "/v1/capabilities");
            assert_eq!(requests[3].path, "/v1/auth/github");
        }
    }

    #[tokio::test]
    async fn pending_record_recovery_rejects_changed_identity_and_untrusted_envelope_fields() {
        let server = FakeServer::start(vec![caps_reply()]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let account_session = session(&transport, OWNER);
        let operation = prepared_upload(&transport, &account_session);
        let bytes = operation.to_pending_record().unwrap();
        let original: Value = serde_json::from_slice(&bytes).unwrap();
        for (field, value) in [
            ("origin", json!("https://another.example/")),
            ("origin", json!("http://another.example/")),
            ("origin", json!("https://user:password@another.example/")),
            ("ownerGithubId", json!("999")),
            ("operationId", json!(OLD_OPERATION)),
            ("method", json!("post")),
            ("method", json!("delete")),
            ("recordVersion", json!(2)),
            ("ifMatch", json!("W/\"weak\"")),
            ("url", json!("https://another.example/arbitrary")),
            ("expectedRevision", json!("9")),
        ] {
            let mut modified = original.clone();
            modified[field] = value;
            assert!(
                transport
                    .restore_pending(&account_session, &serde_json::to_vec(&modified).unwrap())
                    .is_err(),
                "accepted invalid field {field}"
            );
        }
        let duplicated =
            std::str::from_utf8(&bytes)
                .unwrap()
                .replacen('{', "{\"recordVersion\":1,", 1);
        assert!(transport
            .restore_pending(&account_session, duplicated.as_bytes())
            .is_err());
        let mut wrong_service = transport.clone();
        wrong_service.origin = "https://another.example/".parse().unwrap();
        let other_session = session(&wrong_service, OWNER);
        assert!(matches!(
            wrong_service.restore_pending(&other_session, &bytes),
            Err(Error::ContextMismatch)
        ));
        let other_account = session(&transport, "999");
        assert!(matches!(
            transport.restore_pending(&other_account, &bytes),
            Err(Error::AccountMismatch)
        ));
        // Restore is synchronous and performs no request at all.
        assert_eq!(server.finish().await.len(), 1);
    }

    #[tokio::test]
    async fn pending_record_recovery_revalidates_original_crypto_header_hash_and_cas_body() {
        let server = FakeServer::start(vec![caps_reply()]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = session(&transport, OWNER);
        let operation = prepared_upload(&transport, &session);
        let mut record: Value =
            serde_json::from_slice(&operation.to_pending_record().unwrap()).unwrap();
        let body: Value = serde_json::from_str(record["body"].as_str().unwrap()).unwrap();
        for (field, value) in [
            ("ownerGithubId", json!("999")),
            ("operationId", json!(OLD_OPERATION)),
            ("revision", json!("10")),
            ("baseRevision", json!(u64::MAX.to_string())),
            ("protocolVersion", json!(2)),
            ("cryptoProfile", json!("untrusted-profile")),
            ("ciphertextSha256", json!("0".repeat(64))),
            ("nonce", json!("invalid")),
        ] {
            let mut changed = body.clone();
            changed[field] = value;
            record["body"] = json!(serde_json::to_string(&changed).unwrap());
            assert!(
                transport
                    .restore_pending(&session, &serde_json::to_vec(&record).unwrap())
                    .is_err(),
                "accepted invalid body field {field}"
            );
        }
        let mut zero_base = body.clone();
        zero_base["baseRevision"] = json!("0");
        zero_base["revision"] = json!("1");
        record["body"] = json!(serde_json::to_string(&zero_base).unwrap());
        assert!(transport
            .restore_pending(&session, &serde_json::to_vec(&record).unwrap())
            .is_err());
        let mut weak_kdf = body;
        weak_kdf["kdf"]["iterations"] = json!(1);
        record["body"] = json!(serde_json::to_string(&weak_kdf).unwrap());
        assert!(transport
            .restore_pending(&session, &serde_json::to_vec(&record).unwrap())
            .is_err());
        record["body"] = json!(std::str::from_utf8(operation.body_bytes())
            .unwrap()
            .replacen('{', "{\"protocolVersion\":1,", 1));
        assert!(transport
            .restore_pending(&session, &serde_json::to_vec(&record).unwrap())
            .is_err());

        let deletion = transport
            .prepare_delete(
                &session,
                &metadata(&transport, active_metadata()),
                UuidV4::parse(NEW_OPERATION).unwrap(),
            )
            .unwrap();
        let mut record: Value =
            serde_json::from_slice(&deletion.to_pending_record().unwrap()).unwrap();
        let mut body: Value = serde_json::from_str(record["body"].as_str().unwrap()).unwrap();
        for value in [json!("0"), json!(u64::MAX.to_string())] {
            body["baseRevision"] = value;
            record["body"] = json!(serde_json::to_string(&body).unwrap());
            assert!(transport
                .restore_pending(&session, &serde_json::to_vec(&record).unwrap())
                .is_err());
        }
        assert_eq!(server.finish().await.len(), 1);
    }

    #[tokio::test]
    async fn pending_record_recovery_enforces_outer_and_per_method_inner_size_limits() {
        let server = FakeServer::start(vec![caps_reply()]).await;
        let transport = Transport::for_test(&server.origin, CLIENT_ID)
            .await
            .unwrap();
        let session = session(&transport, OWNER);
        assert!(matches!(
            transport.restore_pending(&session, &vec![b' '; PENDING_RECORD_LIMIT + 1]),
            Err(Error::PayloadTooLarge)
        ));
        for method in [MutationMethod::Upload, MutationMethod::Delete] {
            let operation = match method {
                MutationMethod::Upload => prepared_upload(&transport, &session),
                MutationMethod::Delete => transport
                    .prepare_delete(
                        &session,
                        &metadata(&transport, active_metadata()),
                        UuidV4::parse(NEW_OPERATION).unwrap(),
                    )
                    .unwrap(),
            };
            let mut record: Value =
                serde_json::from_slice(&operation.to_pending_record().unwrap()).unwrap();
            record["body"] = json!(" ".repeat(method.body_limit() + 1));
            assert!(matches!(
                transport.restore_pending(&session, &serde_json::to_vec(&record).unwrap()),
                Err(Error::PayloadTooLarge)
            ));
        }
        assert_eq!(server.finish().await.len(), 1);
    }
}
