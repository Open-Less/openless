use super::*;
use std::sync::Mutex;
#[derive(Default)]
struct MemoryVault(Mutex<BTreeMap<String, String>>);
impl SiwcVault for MemoryVault {
    fn read(&self, key: &str) -> Result<Option<SecretValue>, SiwcError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .map(SecretValue::new))
    }
    fn write(&self, key: &str, value: &str) -> Result<(), SiwcError> {
        assert!(value.len() <= 1024);
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn remove(&self, key: &str) -> Result<(), SiwcError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
    fn compare_head(&self, expected: Option<&str>, next: &str) -> Result<bool, SiwcError> {
        let mut values = self.0.lock().unwrap();
        if values.get("head").map(String::as_str) != expected {
            return Ok(false);
        }
        values.insert("head".into(), next.into());
        Ok(true)
    }
}
fn registration(id: &str) -> Registration {
    Registration {
        id: id.into(),
        issuer: ISSUER.into(),
        subject: "subject".into(),
        client_id: format!("oaiapp_{id}"),
        email: Some("same@example.test".into()),
        epoch: 1,
        tokens: Some(Tokens {
            access: "access-secret".repeat(900),
            refresh: "refresh-secret".into(),
            id: "id-secret".into(),
            nonce: "nonce".into(),
            expires_at: now() + 3600,
            scopes: vec![DIRECT.into(), "resource.invoke".into()],
        }),
        refreshing: None,
        failure: None,
        welcomed: false,
        revocation_confirmed: None,
    }
}
async fn connected(vault: Arc<MemoryVault>) -> Arc<SiwcService> {
    let service = SiwcService::new(vault);
    service
        .change(|data| {
            data.registrations
                .insert("account".into(), registration("account"));
            data.channels.insert(
                "channel".into(),
                Binding {
                    registration: Some("account".into()),
                    attempt: None,
                },
            );
            Ok(())
        })
        .await
        .unwrap();
    service
}
#[tokio::test]
async fn siwc_vault_chunks_long_tokens_and_never_serializes_secrets_to_status() {
    let vault = Arc::new(MemoryVault::default());
    let service = connected(vault.clone()).await;
    let host = service.load().await.unwrap().data.host_id;
    let status = serde_json::to_string(&service.status("channel").await.unwrap()).unwrap();
    for secret in ["access-secret", "refresh-secret", "id-secret", &host] {
        assert!(!status.contains(secret));
    }
    assert_eq!(
        SiwcService::new(vault).load().await.unwrap().data.host_id,
        host
    );
    assert_eq!(service.status("channel").await.unwrap().state, "connected");
}
#[tokio::test]
async fn siwc_registration_identity_is_not_email_and_account_switch_preserves_other_session() {
    let service = connected(Arc::new(MemoryVault::default())).await;
    service
        .change(|data| {
            data.registrations
                .insert("other".into(), registration("other"));
            Ok(())
        })
        .await
        .unwrap();
    let old = service.access("channel").await.unwrap();
    service
        .select_account("channel".into(), "other".into())
        .await
        .unwrap();
    assert_eq!(service.status("channel").await.unwrap().accounts.len(), 2);
    old.check().await.unwrap();
    assert_eq!(
        service.status("channel").await.unwrap().account.unwrap().id,
        "other"
    );
}
#[tokio::test]
async fn siwc_vault_cas_does_not_overwrite_concurrent_writes() {
    let vault = Arc::new(MemoryVault::default());
    let store = store::Store(vault);
    let first = store.load().unwrap();
    let second = store.load().unwrap();
    assert!(store.commit(&first, &first.data).unwrap());
    assert!(!store.commit(&second, &second.data).unwrap());
    assert_eq!(store.load().unwrap().data.host_id, first.data.host_id);
}
#[tokio::test]
async fn siwc_rotated_or_revoked_session_invalidates_inflight_access() {
    let service = connected(Arc::new(MemoryVault::default())).await;
    let access = service.access("channel").await.unwrap();
    access.check().await.unwrap();
    service
        .change(|data| {
            let r = data.registrations.get_mut("account").unwrap();
            r.epoch += 1;
            r.tokens = None;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(access.check().await.unwrap_err().code, "session_changed");
    assert_eq!(
        service.status("channel").await.unwrap().state,
        "reauthorization_required"
    );
    assert!(service.has_binding("channel").await.unwrap());
}
#[tokio::test]
async fn siwc_missing_scope_keeps_identity_without_allowing_inference() {
    let service = connected(Arc::new(MemoryVault::default())).await;
    service
        .change(|data| {
            data.registrations
                .get_mut("account")
                .unwrap()
                .tokens
                .as_mut()
                .unwrap()
                .scopes
                .clear();
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        service.status("channel").await.unwrap().state,
        "usage_disabled"
    );
    assert_eq!(
        service
            .access("channel")
            .await
            .unwrap()
            .token()
            .await
            .unwrap_err()
            .code,
        "usage_disabled"
    );
}
#[test]
fn siwc_request_whitelist_and_terminal_events() {
    use crate::llm_protocol::{LlmRequestFormat, TextEventStream};
    let body = inference::request_body(
        "available-model",
        vec![
            json!({"role":"system","content":"polish"}),
            json!({"role":"user","content":"hello"}),
        ],
    );
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["input"][0]["role"], "developer");
    assert_eq!(body.as_object().unwrap().len(), 4);
    for terminal in ["response.failed", "response.incomplete"] {
        let mut stream =
            TextEventStream::new(LlmRequestFormat::Responses).for_siwc(Some("req_123".into()));
        stream.push(format!("data: {{\"type\":\"{terminal}\",\"response\":{{\"error\":{{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}}}}}\n\n").as_bytes()).unwrap();
        assert!(stream.next().is_err());
    }
    let mut stream = TextEventStream::new(LlmRequestFormat::Responses);
    stream
        .push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\r\n\r\n")
        .unwrap();
    stream.next().unwrap();
    assert!(stream.finish().is_err());
}

#[derive(Default)]
struct HttpState {
    nonce: String,
    subject: String,
    scope: String,
    refreshes: usize,
    revocations: usize,
    forms: Vec<BTreeMap<String, String>>,
    refresh_error: Option<String>,
    authorization_error: Option<String>,
    revoke_status: u16,
    refresh_delay: u64,
    refresh_gate: Option<Arc<tokio::sync::Notify>>,
    inference_error: bool,
    incomplete: bool,
    requests: Vec<Value>,
}
struct Fixture {
    service: Arc<SiwcService>,
    vault: Arc<MemoryVault>,
    state: Arc<Mutex<HttpState>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
fn signing_key() -> &'static (jsonwebtoken::EncodingKey, Value) {
    static KEY: std::sync::OnceLock<(jsonwebtoken::EncodingKey, Value)> =
        std::sync::OnceLock::new();
    KEY.get_or_init(|| {
        use rsa::{pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
        let key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
        let public = key.to_public_key();
        let jwk = json!({"kty":"RSA","kid":"test-key","alg":"RS256","use":"sig","n":URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())});
        (jsonwebtoken::EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes()), jwk)
    })
}
fn signed(claims: &Value) -> String {
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("test-key".into());
    jsonwebtoken::encode(&header, claims, &signing_key().0).unwrap()
}
impl Fixture {
    async fn new() -> Self {
        let _ = signing_key();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let vault = Arc::new(MemoryVault::default());
        let mut service = SiwcService::new(vault.clone());
        Arc::get_mut(&mut service).unwrap().endpoints = Endpoints {
            issuer: base.clone(),
            discovery: format!("{base}/discovery"),
            api: format!("{base}/v1"),
        };
        let state = Arc::new(Mutex::new(HttpState {
            subject: "subject".into(),
            scope: SCOPES.into(),
            ..Default::default()
        }));
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let base = base.clone();
                let state = server_state.clone();
                tokio::spawn(async move {
                    let mut data = Vec::new();
                    let mut part = [0u8; 4096];
                    let end = loop {
                        let count = socket.read(&mut part).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        data.extend_from_slice(&part[..count]);
                        if let Some(end) = data.windows(4).position(|p| p == b"\r\n\r\n") {
                            break end + 4;
                        }
                    };
                    let headers = String::from_utf8_lossy(&data[..end]).to_string();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    while data.len() < end + length {
                        let count = socket.read(&mut part).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        data.extend_from_slice(&part[..count]);
                    }
                    let route = headers.split_whitespace().nth(1).unwrap();
                    let mut status = 200;
                    let mut mime = "application/json";
                    let mut delay = 0;
                    let mut refresh_gate = None;
                    if route.starts_with("/v1/") {
                        assert!(headers.to_ascii_lowercase().contains("authorization: bearer access-"));
                    }
                    let body = match route {
                        "/discovery" => json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"jwks_uri":format!("{base}/jwks"),"revocation_endpoint":format!("{base}/revoke")}).to_string(),
                        "/jwks" => json!({"keys":[signing_key().1.clone()]}).to_string(),
                        "/token" => {
                            let form: BTreeMap<String,String> = url::form_urlencoded::parse(&data[end..end+length]).into_owned().collect();
                            let mut state = state.lock().unwrap(); state.forms.push(form.clone());
                            assert_eq!(form["client_id"], "oaiapp_fixture_0001"); assert_eq!(form["resource"], API_BASE);
                            let refresh = form["grant_type"] == "refresh_token";
                            if refresh { state.refreshes += 1; delay = state.refresh_delay; refresh_gate = state.refresh_gate.clone(); assert!(!form.contains_key("scope")); }
                            if !refresh && state.authorization_error.is_some() {
                                status = 400; json!({"error":state.authorization_error}).to_string()
                            } else if refresh && state.refresh_error.is_some() {
                                status = 400; json!({"error":state.refresh_error}).to_string()
                            } else {
                                let claims = json!({"iss":base,"sub":state.subject,"aud":"oaiapp_fixture_0001","iat":now(),"exp":now()+3600,"nonce":state.nonce,"email":"same@example.test"});
                                json!({"access_token":format!("access-{}",state.refreshes),"refresh_token":format!("refresh-{}",state.refreshes),"id_token":signed(&claims),"expires_in":3600,"token_type":"Bearer","scope":state.scope}).to_string()
                            }
                        }
                        "/revoke" => { let mut state = state.lock().unwrap(); state.revocations += 1; if state.revoke_status > 0 { status = state.revoke_status; } String::new() }
                        "/v1/models" => json!({"models":[{"slug":"z","display_name":"Preferred model","visibility":"list"},{"slug":"hidden","visibility":"hide"},{"slug":"a","display_name":"Another model","visibility":"list"}]}).to_string(),
                        "/v1/responses" => {
                            mime = "text/event-stream";
                            let request: Value = serde_json::from_slice(&data[end..end+length]).unwrap();
                            let mut state = state.lock().unwrap(); state.requests.push(request);
                            let terminal = if state.inference_error { "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\",\"param\":\"model\"}}}\r\n\r\n" }
                                else if state.incomplete { "" } else { "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\r\n\r\n" };
                            format!("data: {{\"type\":\"response.output_text.delta\",\"delta\":\"你好🙂\"}}\r\n\r\n{terminal}")
                        }
                        _ => { status = 404; "{}".into() }
                    };
                    if let Some(gate) = refresh_gate {
                        gate.notified().await;
                    }
                    if delay > 0 {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                    let reply = format!("HTTP/1.1 {status} OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nX-Request-Id: req_fixture\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        Self {
            service,
            vault,
            state,
            server,
        }
    }
    async fn authorize(&self) -> url::Url {
        let launch = self
            .service
            .begin_sign_in("channel".into(), false, false)
            .await
            .unwrap();
        let url = url::Url::parse(launch.authorization_url()).unwrap();
        let params: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        self.state.lock().unwrap().nonce = params["nonce"].clone();
        let mut callback = url::Url::parse(&params["redirect_uri"]).unwrap();
        callback
            .query_pairs_mut()
            .append_pair("state", &params["state"])
            .append_pair("code", "fixture-code")
            .append_pair("client_id", "oaiapp_fixture_0001");
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(callback)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        for _ in 0..100 {
            if !self.service.authorizing("channel", &launch.attempt_id) {
                return url;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("authorization did not finish")
    }
    async fn expire(&self) {
        self.service
            .change(|data| {
                selected_mut(data, "channel")?
                    .tokens
                    .as_mut()
                    .unwrap()
                    .expires_at = now();
                Ok(())
            })
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn siwc_oauth_real_loopback_pkce_jwks_and_returning_registration() {
    let f = Fixture::new().await;
    let first = f.authorize().await;
    assert_eq!(
        f.service.status("channel").await.unwrap().state,
        "connected"
    );
    let params: BTreeMap<_, _> = first.query_pairs().into_owned().collect();
    assert_eq!(params["client_id"], "dynamic_agent_client");
    assert_eq!(params["agent_name_hint"], "OpenLess");
    assert!(params["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert!(params["redirect_uri"].ends_with("/auth/callback"));
    let form = f.state.lock().unwrap().forms[0].clone();
    assert_eq!(
        params["code_challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes()))
    );
    assert_eq!(params["redirect_uri"], form["redirect_uri"]);
    let second = f.authorize().await;
    let returning: BTreeMap<_, _> = second.query_pairs().into_owned().collect();
    assert_eq!(returning["client_id"], "oaiapp_fixture_0001");
    assert!(!returning.contains_key("agent_name_hint"));
    assert_eq!(params["ext_agent_host_id"], returning["ext_agent_host_id"]);
    assert_ne!(params["nonce"], returning["nonce"]);
    assert!(returning.contains_key("id_token_hint"));
    assert_eq!(f.service.status("channel").await.unwrap().accounts.len(), 1);
}
#[tokio::test]
async fn siwc_jwt_signature_issuer_audience_nonce_and_expiry_are_verified() {
    let f = Fixture::new().await;
    let discovery = f.service.discovery().await.unwrap();
    let valid = json!({"iss":f.service.endpoints.issuer,"sub":"subject","aud":"oaiapp_fixture_0001","iat":now(),"exp":now()+3600,"nonce":"correct"});
    f.service
        .verify_identity(
            &signed(&valid),
            "oaiapp_fixture_0001",
            Some("correct"),
            &discovery,
        )
        .await
        .unwrap();
    for (key, value) in [
        ("iss", json!("https://attacker.test")),
        ("aud", json!("other-client")),
        ("nonce", json!("wrong")),
        ("exp", json!(1)),
        ("sub", json!("")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(
            f.service
                .verify_identity(
                    &signed(&invalid),
                    "oaiapp_fixture_0001",
                    Some("correct"),
                    &discovery
                )
                .await
                .is_err(),
            "{key}"
        );
    }
    let mut forged = signed(&valid);
    let last = forged.len() - 20;
    forged.replace_range(last..last + 1, "!");
    assert!(f
        .service
        .verify_identity(&forged, "oaiapp_fixture_0001", Some("correct"), &discovery)
        .await
        .is_err());
}
#[tokio::test]
async fn siwc_refresh_is_serial_across_service_instances_and_rotates_atomically() {
    let f = Fixture::new().await;
    f.authorize().await;
    f.expire().await;
    f.state.lock().unwrap().refresh_delay = 100;
    let mut second = SiwcService::new(f.vault.clone());
    Arc::get_mut(&mut second).unwrap().endpoints = f.service.endpoints.clone();
    let first_access = f.service.access("channel").await.unwrap();
    let second_access = second.access("channel").await.unwrap();
    let (a, b) = tokio::join!(first_access.token(), second_access.token());
    assert_eq!(a.unwrap().expose_secret(), "access-1");
    assert_eq!(b.unwrap().expose_secret(), "access-1");
    assert_eq!(f.state.lock().unwrap().refreshes, 1);
    let data = f.service.load().await.unwrap().data;
    let tokens = selected(&data, "channel").unwrap().tokens.as_ref().unwrap();
    assert_eq!(tokens.refresh, "refresh-1");
    assert!(tokens.expires_at > now() + 60);
}
#[tokio::test]
async fn siwc_terminal_refresh_keeps_registration_and_temporary_errors_keep_tokens() {
    for code in ["invalid_grant", "temporarily_unavailable"] {
        let f = Fixture::new().await;
        f.authorize().await;
        f.expire().await;
        f.state.lock().unwrap().refresh_error = Some(code.into());
        let failure = f
            .service
            .access("channel")
            .await
            .unwrap()
            .token()
            .await
            .unwrap_err();
        assert_eq!(failure.code, code);
        let data = f.service.load().await.unwrap().data;
        let r = selected(&data, "channel").unwrap();
        assert_eq!(r.client_id, "oaiapp_fixture_0001");
        assert_eq!(r.tokens.is_some(), code != "invalid_grant");
    }
}
#[tokio::test]
async fn siwc_wrong_identity_never_overwrites_an_existing_registration() {
    let f = Fixture::new().await;
    f.authorize().await;
    f.state.lock().unwrap().subject = "another-subject".into();
    f.authorize().await;
    let status = f.service.status("channel").await.unwrap();
    assert_eq!(status.error.unwrap().code, "identity_mismatch");
    let data = f.service.load().await.unwrap().data;
    assert_eq!(selected(&data, "channel").unwrap().subject, "subject");
}
#[tokio::test]
async fn siwc_cancelled_callback_cannot_reconnect_and_signout_reuses_registration() {
    let f = Fixture::new().await;
    f.authorize().await;
    let lease = f.service.access("channel").await.unwrap();
    let launch = f
        .service
        .begin_sign_in("channel".into(), false, false)
        .await
        .unwrap();
    f.service.cancel_sign_in("channel").await.unwrap();
    assert!(!f.service.authorizing("channel", &launch.attempt_id));
    assert!(
        f.service
            .disconnect("channel")
            .await
            .unwrap()
            .remote_revoked
    );
    assert_eq!(f.state.lock().unwrap().revocations, 1);
    assert!(lease.check().await.is_err());
    let launch = f
        .service
        .begin_sign_in("channel".into(), false, false)
        .await
        .unwrap();
    let params: BTreeMap<_, _> = url::Url::parse(launch.authorization_url())
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(params["client_id"], "oaiapp_fixture_0001");
    assert!(!params.contains_key("id_token_hint"));
    f.service.cancel_sign_in("channel").await.unwrap();
}
#[tokio::test]
async fn siwc_catalog_and_full_sse_lifecycle_use_the_account_token() {
    use crate::polish::StreamingTimeouts;
    let f = Fixture::new().await;
    f.authorize().await;
    let models = f.service.list_models("channel").await.unwrap();
    assert_eq!(
        models.iter().map(|m| m.slug.as_str()).collect::<Vec<_>>(),
        vec!["z", "a"]
    );
    assert_eq!(models[0].display_name, "Preferred model");
    let access = f.service.access("channel").await.unwrap();
    let body = inference::request_body(
        "z",
        vec![
            json!({"role":"system","content":"polish"}),
            json!({"role":"user","content":"text"}),
        ],
    );
    let timeouts = StreamingTimeouts {
        first_token: Duration::from_secs(3),
        idle: Duration::from_secs(3),
    };
    assert_eq!(
        access
            .infer(&body, timeouts, |_| {}, || false)
            .await
            .unwrap(),
        "你好🙂"
    );
    f.state.lock().unwrap().incomplete = true;
    let incomplete = access
        .infer(&body, timeouts, |_| {}, || false)
        .await
        .unwrap_err();
    match incomplete {
        crate::polish::LLMError::Siwc(error) => {
            assert_eq!(error.code, "response_incomplete");
            assert_eq!(error.http_status, Some(200));
            assert_eq!(error.request_id.as_deref(), Some("req_fixture"));
        }
        other => panic!("{other}"),
    }
    f.state.lock().unwrap().inference_error = true;
    let failure = access
        .infer(&body, timeouts, |_| {}, || false)
        .await
        .unwrap_err();
    match failure {
        crate::polish::LLMError::Siwc(e) => {
            assert_eq!(e.code, "subscription_sharing_usage_limit_exceeded");
            assert_eq!(e.request_id.as_deref(), Some("req_fixture"));
        }
        other => panic!("{other}"),
    }
    assert_eq!(
        f.service.status("channel").await.unwrap().state,
        "usage_limited"
    );
    let before = f.state.lock().unwrap().requests.len();
    assert!(access
        .infer(&body, timeouts, |_| {}, || false)
        .await
        .is_err());
    assert_eq!(f.state.lock().unwrap().requests.len(), before);
}

#[tokio::test]
async fn siwc_cancelled_inference_does_not_abandon_refresh_rotation() {
    let f = Fixture::new().await;
    f.authorize().await;
    f.expire().await;
    let gate = Arc::new(tokio::sync::Notify::new());
    f.state.lock().unwrap().refresh_gate = Some(gate.clone());
    let access = f.service.access("channel").await.unwrap();
    let task = tokio::spawn(async move { access.token().await });
    while f.state.lock().unwrap().refreshes == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    task.abort();
    let _ = task.await;
    gate.notify_one();
    let token = f
        .service
        .access("channel")
        .await
        .unwrap()
        .token()
        .await
        .unwrap();
    assert_eq!(token.expose_secret(), "access-1");
    assert_eq!(f.state.lock().unwrap().refreshes, 1);
}
#[tokio::test]
async fn siwc_logout_wins_over_a_late_refresh_and_remote_failure_is_explicit() {
    let f = Fixture::new().await;
    f.authorize().await;
    f.expire().await;
    let gate = Arc::new(tokio::sync::Notify::new());
    {
        let mut s = f.state.lock().unwrap();
        s.refresh_gate = Some(gate.clone());
        s.revoke_status = 503;
    }
    let access = f.service.access("channel").await.unwrap();
    let task = tokio::spawn(async move { access.token().await });
    while f.state.lock().unwrap().refreshes == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        !f.service
            .disconnect("channel")
            .await
            .unwrap()
            .remote_revoked
    );
    gate.notify_one();
    assert!(task.await.unwrap().is_err());
    assert_eq!(
        f.service.status("channel").await.unwrap().state,
        "reauthorization_required"
    );
    assert_eq!(f.state.lock().unwrap().revocations, 2);
}
struct FailingCommit {
    inner: Arc<MemoryVault>,
    heads: std::sync::atomic::AtomicUsize,
}
impl SiwcVault for FailingCommit {
    fn read(&self, key: &str) -> Result<Option<SecretValue>, SiwcError> {
        self.inner.read(key)
    }
    fn write(&self, key: &str, value: &str) -> Result<(), SiwcError> {
        self.inner.write(key, value)
    }
    fn remove(&self, key: &str) -> Result<(), SiwcError> {
        self.inner.remove(key)
    }
    fn compare_head(&self, expected: Option<&str>, next: &str) -> Result<bool, SiwcError> {
        if self.heads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            return Err(error("storage_failed"));
        }
        self.inner.compare_head(expected, next)
    }
}
#[tokio::test]
async fn siwc_failed_rotated_token_commit_never_replays_the_old_refresh_token() {
    let f = Fixture::new().await;
    f.authorize().await;
    f.expire().await;
    let mut service = SiwcService::new(Arc::new(FailingCommit {
        inner: f.vault.clone(),
        heads: Default::default(),
    }));
    Arc::get_mut(&mut service).unwrap().endpoints = f.service.endpoints.clone();
    let access = service.access("channel").await.unwrap();
    assert_eq!(access.token().await.unwrap_err().code, "storage_failed");
    assert_eq!(f.state.lock().unwrap().refreshes, 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), access.token())
            .await
            .is_err()
    );
    f.service
        .change(|data| {
            selected_mut(data, "channel")?
                .refreshing
                .as_mut()
                .unwrap()
                .1 = now() - 121;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        access.token().await.unwrap_err().code,
        "refresh_interrupted"
    );
    assert_eq!(f.state.lock().unwrap().refreshes, 1);
}
#[tokio::test]
async fn siwc_invalid_code_retains_issued_client_without_trusting_identity() {
    let f = Fixture::new().await;
    f.state.lock().unwrap().authorization_error = Some("invalid_grant".into());
    f.authorize().await;
    assert!(f.service.status("channel").await.unwrap().account.is_none());
    f.state.lock().unwrap().authorization_error = None;
    let returning = f.authorize().await;
    let params: BTreeMap<_, _> = returning.query_pairs().into_owned().collect();
    assert_eq!(params["client_id"], "oaiapp_fixture_0001");
    assert!(!params.contains_key("agent_name_hint"));
    assert_eq!(
        f.service.status("channel").await.unwrap().state,
        "connected"
    );
}
#[tokio::test]
async fn siwc_identity_only_authorization_does_not_run_models_or_inference() {
    let f = Fixture::new().await;
    f.state.lock().unwrap().scope = "openid profile email".into();
    f.authorize().await;
    let status = f.service.status("channel").await.unwrap();
    assert!(status.account.is_some());
    assert_eq!(status.state, "usage_disabled");
    assert_eq!(
        f.service.list_models("channel").await.unwrap_err().code,
        "usage_disabled"
    );
    assert!(f.state.lock().unwrap().requests.is_empty());
}

#[tokio::test]
async fn siwc_start_does_not_block_on_discovery_and_cancel_is_immediate() {
    let mut f = Fixture::new().await;
    Arc::get_mut(&mut f.service).unwrap().endpoints.discovery =
        "http://127.0.0.1:1/unreachable".into();
    let launch = f
        .service
        .begin_sign_in("channel".into(), false, false)
        .await
        .unwrap();
    assert!(f.service.authorizing("channel", &launch.attempt_id));
    f.service.cancel_sign_in("channel").await.unwrap();
    assert!(!f.service.authorizing("channel", &launch.attempt_id));
    assert!(f.state.lock().unwrap().forms.is_empty());
}
#[tokio::test]
async fn siwc_state_and_declined_consent_never_exchange_a_code() {
    let f = Fixture::new().await;
    let launch = f
        .service
        .begin_sign_in("channel".into(), false, false)
        .await
        .unwrap();
    let params: BTreeMap<_, _> = url::Url::parse(launch.authorization_url())
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    let mut callback = url::Url::parse(&params["redirect_uri"]).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", "incorrect")
        .append_pair("code", "untrusted");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        client
            .get(callback.clone())
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        400
    );
    assert!(f.state.lock().unwrap().forms.is_empty());
    callback.set_query(None);
    callback
        .query_pairs_mut()
        .append_pair("state", &params["state"])
        .append_pair("error", "access_denied");
    client
        .get(callback)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    for _ in 0..100 {
        if !f.service.authorizing("channel", &launch.attempt_id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        f.service
            .status("channel")
            .await
            .unwrap()
            .error
            .unwrap()
            .code,
        "access_denied"
    );
    assert!(f.state.lock().unwrap().forms.is_empty());
}

struct ChannelCredentials(Arc<SiwcService>);
impl crate::CredentialStore for ChannelCredentials {
    fn siwc(&self) -> Option<Arc<SiwcService>> {
        Some(self.0.clone())
    }
    fn status(
        &self,
        _: crate::UserPreferences,
    ) -> futures_util::future::BoxFuture<'static, Result<crate::CredentialsStatus, BackendError>>
    {
        Box::pin(async {
            Ok(crate::CredentialsStatus {
                active_llm_provider: PROVIDER_ID.into(),
                ..Default::default()
            })
        })
    }
    fn read(
        &self,
        key: crate::CredentialKey,
    ) -> futures_util::future::BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        Box::pin(async move {
            Ok(match key.account.as_str() {
                crate::credentials::LLM_MODEL_ACCOUNT => Some(SecretValue::new("z")),
                crate::credentials::LLM_API_KEY_ACCOUNT => {
                    Some(SecretValue::new("legacy-secret-must-not-be-sent"))
                }
                crate::credentials::LLM_ENDPOINT_ACCOUNT => {
                    Some(SecretValue::new("https://invalid.example/never-use-this"))
                }
                crate::credentials::LLM_EXTRA_HEADERS_ACCOUNT => Some(SecretValue::new("not-json")),
                _ => None,
            })
        })
    }
    fn write(
        &self,
        _: crate::CredentialKey,
        _: SecretValue,
    ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
    fn remove(
        &self,
        _: crate::CredentialKey,
    ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
    fn list_channels(
        &self,
        _: crate::ChannelKind,
    ) -> futures_util::future::BoxFuture<'static, Result<Vec<crate::ChannelSummary>, BackendError>>
    {
        Box::pin(async {
            Ok(vec![crate::ChannelSummary {
                id: "channel".into(),
                name: "ChatGPT Plan".into(),
                provider_type: PROVIDER_ID.into(),
                enabled: true,
                order: 0,
                last_test: None,
            }])
        })
    }
    fn active_provider(
        &self,
        _: crate::ProviderSlot,
    ) -> futures_util::future::BoxFuture<'static, Result<String, BackendError>> {
        Box::pin(async { Ok("channel".into()) })
    }
}
#[derive(Default)]
struct TextSink(Mutex<String>);
impl crate::TextStreamSink for TextSink {
    fn publish(&self, chunk: crate::TextStreamChunk) -> Result<(), BackendError> {
        self.0.lock().unwrap().push_str(&chunk.text);
        Ok(())
    }
}
#[tokio::test]
async fn siwc_shared_polish_translation_and_provider_validation_ignore_legacy_api_credentials() {
    use crate::{ProviderApi, TextPolisher};
    let f = Fixture::new().await;
    f.authorize().await;
    let credentials: Arc<dyn crate::CredentialStore> =
        Arc::new(ChannelCredentials(f.service.clone()));
    let provider =
        crate::ProviderService::new(credentials.clone(), Arc::new(crate::TokioTaskSpawner));
    let request = crate::ProviderRequest {
        kind: crate::ProviderKind::Llm,
        channel_id: Some("channel".into()),
        thinking_enabled: false,
    };
    assert_eq!(
        provider.list_models(request.clone()).await.unwrap().models,
        vec!["z", "a"]
    );
    provider.validate(request.clone()).await.unwrap();
    let polisher = crate::SharedCloudTextPolisher::new(credentials);
    let mut context = crate::dictation_context::DictationContext::default();
    context.llm.provider_id = "channel".into();
    context.llm.provider_type = PROVIDER_ID.into();
    context.llm.model = Some("z".into());
    context.polish.style_system_prompt = "style-fixture".into();
    context.polish.hotwords = vec!["term-fixture".into()];
    let sink = Arc::new(TextSink::default());
    let output = polisher
        .polish(
            crate::SessionId::new(),
            Arc::new(context.clone()),
            "source text".into(),
            sink.clone(),
        )
        .await
        .unwrap();
    assert_eq!(output.text, "你好🙂");
    assert_eq!(*sink.0.lock().unwrap(), "你好🙂");
    context.polish.translation_active = true;
    context.polish.translation_target_language = "English".into();
    assert_eq!(
        polisher
            .polish(
                crate::SessionId::new(),
                Arc::new(context),
                "source text".into(),
                Arc::new(TextSink::default())
            )
            .await
            .unwrap()
            .text,
        "你好🙂"
    );
    let requests = f.state.lock().unwrap().requests.clone();
    assert!(requests
        .iter()
        .any(|body| body.to_string().contains("style-fixture")));
    for body in &requests {
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body.as_object().unwrap().len(), 4);
        assert_eq!(body["input"][0]["role"], "developer");
    }
    f.state.lock().unwrap().inference_error = true;
    let error = provider.validate(request).await.unwrap_err();
    assert_eq!(
        error.details.unwrap()["siwc"]["code"],
        "subscription_sharing_usage_limit_exceeded"
    );
    assert!(error.message.contains("ChatGPT plan"));
}
#[tokio::test]
async fn siwc_nonstreaming_callers_still_consume_responses_sse() {
    let f = Fixture::new().await;
    f.authorize().await;
    let config =
        crate::polish::OpenAICompatibleConfig::new(PROVIDER_ID, "ChatGPT Plan", API_BASE, "", "z");
    let provider = crate::polish::OpenAICompatibleLLMProvider::new(config)
        .with_siwc_access(f.service.access("channel").await.unwrap());
    assert_eq!(
        provider
            .translate_to(
                "text",
                "English",
                &[],
                Default::default(),
                Default::default(),
                None
            )
            .await
            .unwrap(),
        "你好🙂"
    );
    assert!(f
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .all(|body| body["stream"] == true));
}
#[test]
fn siwc_structured_diagnostics_never_copy_an_error_body_or_token_to_the_ui() {
    let body = json!({"error":{"code":"subscription_sharing_usage_limit_exceeded","param":"model","message":"access-secret refresh-secret user transcript"}});
    let error: BackendError = response_error(429, &body, Some("req_safe")).into_backend();
    let wire = serde_json::to_string(&error).unwrap();
    assert!(!wire.contains("access-secret"));
    assert!(!wire.contains("refresh-secret"));
    assert!(!wire.contains("user transcript"));
    assert_eq!(error.details.unwrap()["siwc"]["requestId"], "req_safe");
}

#[tokio::test]
async fn siwc_logout_during_rotation_never_claims_remote_revocation_was_confirmed() {
    let f = Fixture::new().await;
    f.authorize().await;
    f.expire().await;
    let gate = Arc::new(tokio::sync::Notify::new());
    f.state.lock().unwrap().refresh_gate = Some(gate.clone());
    let access = f.service.access("channel").await.unwrap();
    let task = tokio::spawn(async move { access.token().await });
    while f.state.lock().unwrap().refreshes == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        !f.service
            .disconnect("channel")
            .await
            .unwrap()
            .remote_revoked
    );
    gate.notify_one();
    assert!(task.await.unwrap().is_err());
    assert!(
        !f.service
            .disconnect("channel")
            .await
            .unwrap()
            .remote_revoked
    );
    assert_eq!(
        f.service
            .status("channel")
            .await
            .unwrap()
            .error
            .unwrap()
            .code,
        "remote_revocation_unconfirmed"
    );
}

#[tokio::test]
async fn siwc_cleanup_of_an_untouched_draft_does_not_create_a_secret_vault() {
    let vault = Arc::new(MemoryVault::default());
    let service = SiwcService::new(vault.clone());
    service.cancel_sign_in("draft").await.unwrap();
    service.detach("draft".into()).await.unwrap();
    assert!(vault.0.lock().unwrap().is_empty());
}
#[tokio::test]
async fn siwc_cancelling_an_old_attempt_preserves_its_successor() {
    let f = Fixture::new().await;
    let first = f
        .service
        .begin_sign_in("channel".into(), false, false)
        .await
        .unwrap();
    let second = f
        .service
        .begin_sign_in("channel".into(), false, false)
        .await
        .unwrap();
    f.service
        .cancel_attempt("channel", &first.attempt_id)
        .await
        .unwrap();
    assert!(f.service.authorizing("channel", &second.attempt_id));
    assert_eq!(
        f.service
            .status("channel")
            .await
            .unwrap()
            .attempt_id
            .as_deref(),
        Some(second.attempt_id.as_str())
    );
    f.service.cancel_sign_in("channel").await.unwrap();
}

#[tokio::test]
async fn siwc_refresh_discovery_failure_can_retry_after_network_recovers() {
    let f = Fixture::new().await;
    f.authorize().await;
    f.expire().await;
    let mut service = SiwcService::new(f.vault.clone());
    Arc::get_mut(&mut service).unwrap().endpoints = f.service.endpoints.clone();
    Arc::get_mut(&mut service).unwrap().endpoints.discovery =
        "http://127.0.0.1:1/unreachable".into();
    let access = service.access("channel").await.unwrap();
    assert_eq!(access.token().await.unwrap_err().code, "network_failed");
    assert_eq!(f.state.lock().unwrap().refreshes, 0);
    drop(access);
    Arc::get_mut(&mut service).unwrap().endpoints.discovery = f.service.endpoints.discovery.clone();
    let access = service.access("channel").await.unwrap();
    let token = tokio::time::timeout(Duration::from_secs(3), access.token())
        .await
        .expect("discovery recovery must not wait for an abandoned refresh")
        .expect("discovery failed before sending a rotating-token request");
    assert_eq!(token.expose_secret(), "access-1");
    assert_eq!(f.state.lock().unwrap().refreshes, 1);
}

#[tokio::test]
async fn siwc_interrupted_disconnect_preserves_remote_revocation_warning() {
    let f = Fixture::new().await;
    f.authorize().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut service = SiwcService::new(f.vault.clone());
    Arc::get_mut(&mut service).unwrap().endpoints = f.service.endpoints.clone();
    Arc::get_mut(&mut service).unwrap().endpoints.discovery =
        format!("http://{}/discovery", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { service.disconnect("channel").await });
    let (_blocked_request, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .unwrap()
        .unwrap();
    // Local sign-out is committed, but the remote revocation has not started.
    task.abort();
    let _ = task.await;
    assert_eq!(f.state.lock().unwrap().revocations, 0);
    let restarted = SiwcService::new(f.vault.clone());
    let status = restarted.status("channel").await.unwrap();
    assert_eq!(status.state, "reauthorization_required");
    assert_eq!(
        status.error.as_ref().map(|e| e.code.as_str()),
        Some("remote_revocation_unconfirmed")
    );
}
