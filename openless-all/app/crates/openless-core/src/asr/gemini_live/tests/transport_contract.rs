use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};

/// Inspect upgrade headers using the handshake callback's fixed response signature.
struct CheckUpgradeRequest;

impl Callback for CheckUpgradeRequest {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let headers = request.headers();
        assert_eq!(headers.get_all("x-goog-api-key").iter().count(), 1);
        assert_eq!(headers["x-goog-api-key"], API_KEY);
        assert_eq!(headers["x-gateway-route"], "transcription");
        assert_eq!(headers.get_all("upgrade").iter().count(), 1);
        assert_eq!(headers["upgrade"], "websocket");
        assert_eq!(request.uri().query(), Some("project=demo"));
        assert_eq!(
            request.uri().path(),
            "/gateway/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent"
        );
        Ok(response)
    }
}

#[test]
fn normalizes_existing_api_roots_and_explicit_live_endpoints() {
    const PATH: &str =
        "/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
    let expected = format!("https://generativelanguage.googleapis.com{PATH}");
    for base in [
        "",
        "https://generativelanguage.googleapis.com",
        "https://generativelanguage.googleapis.com/v1beta/",
        "https://generativelanguage.googleapis.com/v1alpha/interactions/",
        "https://generativelanguage.googleapis.com/v1beta/interactions/?key=discard&access_token=discard#fragment",
    ] {
        assert_eq!(transport::endpoint(base).unwrap().as_str(), expected);
    }
    assert_eq!(
        transport::endpoint("https://gateway.example/gemini/v1beta?project=demo&key=discard")
            .unwrap()
            .as_str(),
        format!("https://gateway.example/gemini{PATH}?project=demo")
    );
    assert_eq!(
        transport::endpoint(&format!(
            "wss://gateway.example/custom{PATH}?project=demo#fragment"
        ))
        .unwrap()
        .as_str(),
        format!("https://gateway.example/custom{PATH}?project=demo")
    );
    assert_eq!(
        transport::endpoint("ws://127.0.0.1:9000/v1beta")
            .unwrap()
            .as_str(),
        format!("http://127.0.0.1:9000{PATH}")
    );
}

#[test]
fn rejects_unsupported_schemes_and_embedded_credentials_without_echoing_them() {
    for endpoint in [
        "not-a-url test-gemini-secret",
        "ftp://example.com/path?key=test-gemini-secret",
        "https://test-gemini-secret:password@example.com/v1beta",
    ] {
        let error = transport::endpoint(endpoint).unwrap_err().to_string();
        assert!(!error.contains(API_KEY));
        assert!(!error.contains("password"));
    }
}

#[tokio::test]
async fn upgrade_uses_one_api_key_header_and_preserves_gateway_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/gateway/v1beta?key=discard&access_token=discard&project=demo",
        listener.local_addr().unwrap()
    );
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_hdr_async(stream, CheckUpgradeRequest)
            .await
            .unwrap();
        timeout(TEST_TIMEOUT, assert_closed(&mut socket))
            .await
            .unwrap();
    });
    let mut config = config(&endpoint);
    config.api_key = format!(" {API_KEY} ");
    config.extra_headers = HashMap::from([
        ("x-gateway-route".into(), "transcription".into()),
        ("X-Goog-Api-Key".into(), "must-not-override".into()),
        ("Upgrade".into(), "must-not-override".into()),
    ]);
    let mut socket = timeout(TEST_TIMEOUT, transport::connect(&config))
        .await
        .unwrap()
        .unwrap();
    socket.close(None).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn redirect_is_not_followed_or_allowed_to_expose_credentials() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1beta", origin.local_addr().unwrap());
    let location = format!("http://{}/?key={API_KEY}", target.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = origin.accept().await.unwrap();
        let request = read_headers(&mut stream).await;
        assert!(request
            .to_ascii_lowercase()
            .contains(&format!("x-goog-api-key: {API_KEY}")));
        stream
            .write_all(format!(
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ).as_bytes())
            .await
            .unwrap();
    });
    let error = timeout(TEST_TIMEOUT, transport::connect(&config(&endpoint)))
        .await
        .unwrap()
        .expect_err("credential-bearing upgrade followed a redirect")
        .to_string();
    assert!(error.contains("307"));
    assert!(!error.contains(API_KEY));
    assert!(timeout(PENDING_WINDOW, target.accept()).await.is_err());
    server.await.unwrap();
}

/// Inspect the upgrade request without depending on an HTTP server framework.
async fn read_headers(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1_024];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = timeout(TEST_TIMEOUT, stream.read(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        assert!(count > 0, "client closed before the HTTP upgrade");
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).unwrap()
}
