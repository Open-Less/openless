//! Main-App account controls. Never opt iOS into Core's full-snapshot E2EE
//! service: it currently includes documents outside the approved mobile scope.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use openless_core::{
    credentials::{CredentialKey, CredentialNamespace, SecretValue},
    domains::{MarketplaceApi, OAuthPollResult},
    BackendError, BackendErrorCode, CredentialStore, MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(
    tag = "command",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum AccountSyncCommand {
    Status {},
    BeginSignIn {},
    PollSignIn {
        #[serde(rename = "authorizationSessionID")]
        authorization_session_id: String,
    },
    CancelSignIn {
        #[serde(rename = "authorizationSessionID")]
        authorization_session_id: String,
    },
    SignOut {},
    Initialize {},
    PrepareEnable {},
    Lock {},
    Unlock {},
    Recover {},
    SyncNow {},
    PreviewRestore {},
    ApplyRestore {},
    ChangePassword {},
    DeleteRemote {},
    SetEnabled {},
}

struct ActiveFlow {
    id: String,
    expires_at: Instant,
}
struct VerifiedLogin {
    login: String,
    token: SecretValue,
}
#[derive(Default)]
struct State {
    flow: Option<ActiveFlow>,
    verified: Option<VerifiedLogin>,
}

pub(crate) struct AccountSync {
    marketplace: Arc<dyn MarketplaceApi>,
    credentials: Arc<dyn CredentialStore>,
    // Serialize begin/poll/cancel/logout through credential persistence. A late
    // poll cannot publish or re-save a token after a successful sign-out.
    state: Mutex<State>,
}

fn failure(code: BackendErrorCode, reason: &'static str) -> BackendError {
    BackendError::new(code, reason)
}
fn auth_failed(_: BackendError) -> BackendError {
    failure(BackendErrorCode::InvalidState, "account_auth_failed")
}
fn invalid() -> BackendError {
    failure(
        BackendErrorCode::InvalidArgument,
        "invalid_account_sync_request",
    )
}
fn token_key() -> Result<CredentialKey, BackendError> {
    CredentialKey::new(
        CredentialNamespace::Marketplace,
        None,
        MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
    )
}
fn session_id(value: &str) -> Result<(), BackendError> {
    let parsed = Uuid::parse_str(value).map_err(|_| invalid())?;
    if parsed.get_version_num() != 4 || parsed.to_string() != value {
        return Err(invalid());
    }
    Ok(())
}
fn valid_login(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

impl AccountSync {
    pub(crate) fn new(
        marketplace: Arc<dyn MarketplaceApi>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            marketplace,
            credentials,
            state: Mutex::new(State::default()),
        }
    }

    async fn status(&self, state: &mut State) -> Result<Value, BackendError> {
        let present = self
            .marketplace
            .auth_status()
            .await
            .map_err(auth_failed)?
            .signed_in;
        let token = if present {
            self.credentials
                .read(token_key()?)
                .await
                .map_err(auth_failed)?
        } else {
            None
        };
        if state
            .verified
            .as_ref()
            .is_some_and(|verified| token.as_ref() != Some(&verified.token))
        {
            state.verified = None;
        }
        let (auth, account, identity) = match &state.verified {
            Some(verified) => (
                "signed_in",
                json!({"githubID":null,"login":verified.login}),
                "verified_login_only",
            ),
            None if token.is_some() => ("unverified", Value::Null, "unresolved"),
            None => ("signed_out", Value::Null, "unresolved"),
        };
        Ok(json!({
            "schemaVersion":1,"authState":auth,"account":account,"identityState":identity,
            "capabilities":null,
            "sync":{"available":false,"reason":"sync_scope_unavailable",
                "allowedDocuments":["confirmedVocabulary","modeStyleReferences"]}
        }))
    }

    pub(crate) async fn execute(&self, request: Value) -> Result<Value, BackendError> {
        let command: AccountSyncCommand = serde_json::from_value(request).map_err(|_| invalid())?;
        // Reject unsupported sync operations before credentials, network, local
        // capture, recovery, or any write. This is a policy barrier, not a toggle.
        if !matches!(
            command,
            AccountSyncCommand::Status { .. }
                | AccountSyncCommand::BeginSignIn { .. }
                | AccountSyncCommand::PollSignIn { .. }
                | AccountSyncCommand::CancelSignIn { .. }
                | AccountSyncCommand::SignOut { .. }
        ) {
            return Err(failure(
                BackendErrorCode::Unsupported,
                "sync_scope_unavailable",
            ));
        }
        let mut state = self.state.lock().await;
        match command {
            AccountSyncCommand::Status {} => self.status(&mut state).await,
            AccountSyncCommand::BeginSignIn {} => {
                state.flow = None;
                self.marketplace
                    .cancel_device_flow(None)
                    .await
                    .map_err(auth_failed)?;
                let flow = self
                    .marketplace
                    .start_device_flow()
                    .await
                    .map_err(auth_failed)?;
                // Core only guarantees HTTPS. Do not let server-supplied URLs
                // turn the native browser action into an arbitrary navigation.
                if session_id(&flow.flow_id).is_err()
                    || flow.verification_uri != "https://github.com/login/device"
                    || flow.user_code.is_empty()
                    || flow.user_code.len() > 64
                    || !flow
                        .user_code
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    || flow.expires_in_secs == 0
                    || flow.expires_in_secs > 3600
                    || flow.interval_secs == 0
                    || flow.interval_secs > flow.expires_in_secs
                {
                    let _ = self.marketplace.cancel_device_flow(None).await;
                    return Err(failure(
                        BackendErrorCode::InvalidState,
                        "invalid_account_auth_response",
                    ));
                }
                state.flow = Some(ActiveFlow {
                    id: flow.flow_id.clone(),
                    expires_at: Instant::now() + Duration::from_secs(flow.expires_in_secs),
                });
                Ok(
                    json!({"authorizationSessionID":flow.flow_id,"userCode":flow.user_code,
                    "verificationURL":flow.verification_uri,"expiresIn":flow.expires_in_secs,"interval":flow.interval_secs}),
                )
            }
            AccountSyncCommand::PollSignIn {
                authorization_session_id: id,
            } => {
                session_id(&id)?;
                let flow = state
                    .flow
                    .as_ref()
                    .filter(|flow| flow.id == id)
                    .ok_or_else(|| {
                        failure(BackendErrorCode::Cancelled, "account_auth_session_stale")
                    })?;
                if Instant::now() >= flow.expires_at {
                    state.flow = None;
                    self.marketplace
                        .cancel_device_flow(Some(id))
                        .await
                        .map_err(auth_failed)?;
                    return Ok(json!({"status":"expired"}));
                }
                match self
                    .marketplace
                    .poll_device_flow(id)
                    .await
                    .map_err(auth_failed)?
                {
                    OAuthPollResult::Pending => Ok(json!({"status":"pending","slowDown":false})),
                    OAuthPollResult::SlowDown => Ok(json!({"status":"pending","slowDown":true})),
                    OAuthPollResult::Authorized { login } => {
                        state.flow = None;
                        state.verified = None;
                        if !valid_login(&login) {
                            self.marketplace.logout().await.map_err(auth_failed)?;
                            return Err(failure(
                                BackendErrorCode::InvalidState,
                                "invalid_account_auth_response",
                            ));
                        }
                        let token = self
                            .credentials
                            .read(token_key()?)
                            .await
                            .map_err(auth_failed)?
                            .ok_or_else(|| {
                                failure(BackendErrorCode::InvalidState, "account_auth_failed")
                            })?;
                        state.verified = Some(VerifiedLogin { login, token });
                        Ok(json!({"status":"signed_in","state":self.status(&mut state).await?}))
                    }
                    // Core's Error message may contain an OAuth server body.
                    // Never expose or classify by free-form upstream text.
                    OAuthPollResult::Error { .. } => {
                        state.flow = None;
                        Err(failure(
                            BackendErrorCode::InvalidState,
                            "account_authorization_failed",
                        ))
                    }
                }
            }
            AccountSyncCommand::CancelSignIn {
                authorization_session_id: id,
            } => {
                session_id(&id)?;
                if state.flow.as_ref().is_some_and(|flow| flow.id == id) {
                    state.flow = None;
                    self.marketplace
                        .cancel_device_flow(Some(id))
                        .await
                        .map_err(auth_failed)?;
                }
                Ok(json!({"cancelled":true}))
            }
            AccountSyncCommand::SignOut {} => {
                state.flow = None;
                state.verified = None;
                // Attempt both even if cancellation fails. Never clear domain data.
                let cancelled = self.marketplace.cancel_device_flow(None).await;
                let logged_out = self.marketplace.logout().await;
                logged_out.map_err(auth_failed)?;
                cancelled.map_err(auth_failed)?;
                self.status(&mut state).await
            }
            _ => unreachable!("sync operations rejected before I/O"),
        }
    }
}
