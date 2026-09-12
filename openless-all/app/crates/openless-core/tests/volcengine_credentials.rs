use openless_core::credentials::{CredentialKey, CredentialNamespace};
use openless_core::credentials_legacy::decode_legacy_credentials;

#[test]
fn reloading_volcengine_channels_preserves_service_and_separate_credentials() {
    let saved = r#"{"version":2,"providers":{"asr":{
        "plan-channel":{"providerType":"volcengine","volcengineService":"agent_plan","volcengineApiKey":"plan-key"},
        "legacy-channel":{"providerType":"volcengine","authMode":"app_id_token","appKey":"app","accessKey":"token"}
    }}}"#;
    let loaded = decode_legacy_credentials(saved).unwrap();
    let read = |channel: &str, account: &str| {
        let key =
            CredentialKey::new(CredentialNamespace::Asr, Some(channel.into()), account).unwrap();
        loaded
            .secrets
            .iter()
            .find(|(stored, _)| *stored == key)
            .map(|(_, value)| value.expose_secret())
    };
    assert_eq!(
        read("plan-channel", "volcengine.service"),
        Some("agent_plan")
    );
    assert_eq!(read("plan-channel", "volcengine.api_key"), Some("plan-key"));
    assert_eq!(read("legacy-channel", "volcengine.service"), None);
    assert_eq!(
        read("legacy-channel", "volcengine.access_key"),
        Some("token")
    );
}

#[tokio::test]
async fn volcengine_sessions_send_service_specific_handshakes() {
    use futures_util::{future::BoxFuture, StreamExt};
    use openless_core::asr::volcengine::{
        VolcengineAuthMode, VolcengineConnector, VolcengineCredentials, VolcengineService,
        VolcengineStreamingASR, VolcengineWebSocket,
    };
    use std::{net::SocketAddr, sync::Arc, time::Duration};
    use tokio_tungstenite::tungstenite::{
        handshake::client::{Request, Response},
        Error,
    };
    use tokio_tungstenite::{accept_hdr_async, client_async, MaybeTlsStream};
    struct LocalConnector(SocketAddr);
    impl VolcengineConnector for LocalConnector {
        fn connect(
            &self,
            request: Request,
        ) -> BoxFuture<'static, Result<(VolcengineWebSocket, Response), Error>> {
            let address = self.0;
            Box::pin(async move {
                assert_eq!(request.uri().scheme_str(), Some("wss"));
                let stream = tokio::net::TcpStream::connect(address).await?;
                client_async(request, MaybeTlsStream::Plain(stream)).await
            })
        }
    }
    for (service, mode, path, app_headers) in [
        (
            VolcengineService::Standard,
            VolcengineAuthMode::AppIdToken,
            "/api/v3/sauc/bigmodel_async",
            true,
        ),
        (
            VolcengineService::Standard,
            VolcengineAuthMode::ApiKey,
            "/api/v3/sauc/bigmodel_async",
            false,
        ),
        (
            VolcengineService::AgentPlan,
            VolcengineAuthMode::AppIdToken,
            "/api/v3/plan/sauc/bigmodel_async",
            false,
        ),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut tx = Some(tx);
            let mut socket=accept_hdr_async(stream, move |request: &Request, response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                tx.take().unwrap().send((request.uri().path().to_string(), request.headers().clone())).unwrap();
                Ok(response)
            }).await.unwrap();
            let _ = socket.next().await;
        });
        let provider = Arc::new(
            VolcengineStreamingASR::new(
                VolcengineCredentials {
                    service,
                    auth_mode: mode,
                    app_id: "fixture-app".into(),
                    access_token: "fixture-secret".into(),
                    resource_id: "volc.seedasr.sauc.duration".into(),
                },
                vec![],
            )
            .with_connector(Arc::new(LocalConnector(address))),
        );
        tokio::time::timeout(Duration::from_secs(3), provider.open_session())
            .await
            .unwrap()
            .unwrap();
        let (actual_path, headers) = rx.await.unwrap();
        assert_eq!(actual_path, path);
        assert_eq!(headers["host"], "openspeech.bytedance.com");
        assert_eq!(headers["X-Api-Resource-Id"], "volc.seedasr.sauc.duration");
        if app_headers {
            assert_eq!(headers["X-Api-App-Key"], "fixture-app");
            assert_eq!(headers["X-Api-Access-Key"], "fixture-secret");
            assert!(!headers.contains_key("X-Api-Key"));
        } else {
            assert_eq!(headers["X-Api-Key"], "fixture-secret");
            assert!(!headers.contains_key("X-Api-App-Key"));
            assert!(!headers.contains_key("X-Api-Access-Key"));
        }
        assert!(uuid::Uuid::parse_str(headers["X-Api-Connect-Id"].to_str().unwrap()).is_ok());
        assert!(uuid::Uuid::parse_str(headers["X-Api-Request-Id"].to_str().unwrap()).is_ok());
        provider.cancel();
        server.await.unwrap();
    }
}
