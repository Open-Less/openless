//! DashScope Omni Realtime messages and deterministic endpoint normalization.
//!
//! This is the Omni protocol, not Qwen ASR, LiveTranslate or OpenAI Realtime GA.
//! Wire fields follow the official Omni client-events reference and DashScope
//! SDK's Omni serializer: `modalities` and top-level `turn_detection` remain
//! unchanged for both current Omni 3.5/3.8 and the legacy Omni 3 profile.
//! https://help.aliyun.com/zh/model-studio/client-events
//! https://help.aliyun.com/zh/model-studio/omni-realtime-websocket-access

use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use super::transport::credential_or_model_query;

use crate::polish::LLMError;

const DEFAULT_BASE: &str = "https://dashscope.aliyuncs.com";
const REALTIME_PATH: &str = "/api-ws/v1/realtime";

/// Configure text-only output and manual turn boundaries with 16 kHz PCM16 input.
/// The explicit legacy profile selects its flat format; never infer it from a
/// model name, or inherit OpenAI's 24 kHz format or reasoning settings.
pub(super) fn setup(instructions: &str, legacy: bool) -> Message {
    let mut session = json!({
        "modalities": ["text"],
        "instructions": instructions,
        "turn_detection": null,
        "input_audio_transcription": null,
    });
    if legacy {
        // Omni 3's documented flat `pcm` format is mono, signed 16-bit, 16 kHz.
        session["input_audio_format"] = json!("pcm");
    } else {
        // These common fields work with Omni 3.5 and 3.8. Do not add 3.8-only
        // multichannel fields to the shared current profile.
        session["audio"] = json!({"input": {"format": {
            "type": "pcm", "sample_rate": 16000
        }}});
    }
    Message::text(json!({"type": "session.update", "session": session}).to_string())
}

/// A patch replaces instructions without resetting the negotiated audio format.
pub(super) fn instructions(value: &str) -> Message {
    Message::text(json!({"type": "session.update", "session": {"instructions": value}}).to_string())
}

/// Output modality is already negotiated by setup. Do not send GA-only fields.
pub(super) fn response() -> Message {
    Message::text(r#"{"type":"response.create"}"#)
}

/// Build the HTTP(S) URL used by reqwest's WebSocket upgrade.
///
/// Input forms are deliberately distinct:
/// - HTTP(S) is an API root. A nonempty custom prefix is retained and `/realtime`
///   is appended, unless the path already ends in `/realtime`.
/// - WS(S) with a non-root path is an exact WebSocket endpoint. Its path,
///   including a trailing slash, is retained without guessing a provider route.
/// - A bare host uses DashScope's `/api-ws/v1/realtime` route.
/// - On official DashScope / workspace hosts only, HTTP(S) `/v1` or
///   `/compatible-mode/v1` is recognized as a text-API root and mapped to the
///   Omni route. A third-party gateway with these prefixes is never rewritten.
///
/// Omni 3.8 requires the user's workspace-specific host, e.g.
/// `{workspace}.cn-beijing.maas.aliyuncs.com`; this function cannot discover a
/// workspace from an API key. Users with nonstandard gateway routes should use
/// a complete WS(S) URL. URL customization does not translate gateway protocols.
/// Routing queries survive, but URL credentials, fragments and stale model
/// parameters do not. Authentication belongs in the handshake header only.
pub(super) fn endpoint(base: &str, model: &str) -> Result<url::Url, LLMError> {
    let invalid = || LLMError::ParseError("Invalid Qwen Omni Realtime endpoint".into());
    let raw = if base.trim().is_empty() {
        DEFAULT_BASE
    } else {
        base.trim()
    };
    // `Url` can normalize away an empty userinfo (`https://@host`). Reject that
    // spelling too, rather than accepting any credential-bearing authority.
    if raw
        .split_once("://")
        .map(|(_, rest)| {
            rest.split(['/', '?', '#', '\\'])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
        .unwrap_or(false)
    {
        return Err(invalid());
    }
    let mut url = url::Url::parse(raw).map_err(|_| invalid())?;
    let websocket = matches!(url.scheme(), "ws" | "wss");
    let scheme = match url.scheme() {
        "https" | "wss" => "https",
        "http" | "ws" => "http",
        _ => return Err(invalid()),
    };
    url.set_scheme(scheme).map_err(|_| invalid())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid());
    }
    url.set_fragment(None);
    crate::endpoint_security::validate_http_endpoint(url.as_str()).map_err(|_| invalid())?;

    let path = url.path().trim_end_matches('/');
    if path.is_empty() {
        url.set_path(REALTIME_PATH);
    } else if !websocket {
        let official = url.host_str().is_some_and(official_host);
        if official && matches!(path, "/v1" | "/compatible-mode/v1") {
            url.set_path(REALTIME_PATH);
        } else if !path.ends_with("/realtime") {
            url.set_path(&format!("{path}/realtime"));
        }
    }

    let model = model.trim();
    if model.is_empty() {
        return Err(LLMError::ParseError(
            "Qwen Omni Realtime requires a model".into(),
        ));
    }
    let query = url
        .query_pairs()
        .filter(|(name, _)| !credential_or_model_query(name))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    url.query_pairs_mut()
        .extend_pairs(query)
        .append_pair("model", model);
    Ok(url)
}

fn official_host(host: &str) -> bool {
    matches!(
        host,
        "dashscope.aliyuncs.com" | "dashscope-intl.aliyuncs.com"
    ) || host.ends_with(".maas.aliyuncs.com")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn value(message: Message) -> Value {
        match message {
            Message::Text(text) => serde_json::from_str(&text).unwrap(),
            other => panic!("expected JSON text, got {other:?}"),
        }
    }

    #[test]
    fn current_omni_setup_is_text_only_manual_and_16khz_without_ga_fields() {
        assert_eq!(
            value(setup("Synthetic dictation instructions", false)),
            json!({"type": "session.update", "session": {
                "modalities": ["text"],
                "instructions": "Synthetic dictation instructions",
                "turn_detection": null,
                "input_audio_transcription": null,
                "audio": {"input": {"format": {"type": "pcm", "sample_rate": 16000}}}
            }})
        );
    }

    #[test]
    fn legacy_omni_uses_flat_pcm_format_without_current_audio_fields() {
        assert_eq!(
            value(setup("Legacy synthetic instructions", true)),
            json!({"type": "session.update", "session": {
                "modalities": ["text"],
                "instructions": "Legacy synthetic instructions",
                "turn_detection": null,
                "input_audio_transcription": null,
                "input_audio_format": "pcm"
            }})
        );
    }

    #[test]
    fn instruction_patch_and_response_do_not_reset_or_invent_session_fields() {
        assert_eq!(
            value(instructions("New instructions")),
            json!({"type": "session.update", "session": {"instructions": "New instructions"}})
        );
        assert_eq!(value(response()), json!({"type": "response.create"}));
    }

    #[test]
    fn host_roots_and_official_http_text_roots_use_the_qwen_route() {
        for source in [
            "",
            "https://dashscope.aliyuncs.com/",
            "wss://dashscope.aliyuncs.com",
            "https://dashscope.aliyuncs.com/v1/",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
        ] {
            assert_eq!(
                endpoint(source, "qwen3.5-omni-flash-realtime")
                    .unwrap()
                    .as_str(),
                "https://dashscope.aliyuncs.com/api-ws/v1/realtime?model=qwen3.5-omni-flash-realtime"
            );
        }
        for host in [
            "dashscope-intl.aliyuncs.com",
            "synthetic.cn-beijing.maas.aliyuncs.com",
            "synthetic.ap-southeast-1.maas.aliyuncs.com",
        ] {
            let source = format!("https://{host}/compatible-mode/v1");
            let url = endpoint(&source, "qwen3.8-omni-flash-realtime").unwrap();
            assert_eq!(url.host_str(), Some(host));
            assert_eq!(url.path(), REALTIME_PATH);
        }
    }

    #[test]
    fn custom_api_roots_keep_their_gateway_prefix() {
        for (source, path) in [
            ("https://example.com/gateway/v1", "/gateway/v1/realtime"),
            (
                "https://example.com/compatible-mode/v1/",
                "/compatible-mode/v1/realtime",
            ),
            (
                "https://dashscope.aliyuncs.com.example.com/v1",
                "/v1/realtime",
            ),
            (
                "https://example.com/gateway%20one/api-ws/v1",
                "/gateway%20one/api-ws/v1/realtime",
            ),
            (
                "https://example.com/gateway/api-ws/v1/realtime",
                "/gateway/api-ws/v1/realtime",
            ),
        ] {
            assert_eq!(endpoint(source, "synthetic-model").unwrap().path(), path);
        }
    }

    #[test]
    fn websocket_paths_are_complete_endpoints_including_trailing_slashes() {
        for (source, expected) in [
            (
                "wss://example.com/gateway/socket/",
                "https://example.com/gateway/socket/?model=synthetic-model",
            ),
            (
                "ws://127.0.0.1:19001/proxy/audio",
                "http://127.0.0.1:19001/proxy/audio?model=synthetic-model",
            ),
            (
                "wss://dashscope.aliyuncs.com/v1",
                "https://dashscope.aliyuncs.com/v1?model=synthetic-model",
            ),
        ] {
            assert_eq!(
                endpoint(source, "synthetic-model").unwrap().as_str(),
                expected
            );
        }
        assert_eq!(
            endpoint("ws://localhost:19001", "synthetic-model")
                .unwrap()
                .as_str(),
            "http://localhost:19001/api-ws/v1/realtime?model=synthetic-model"
        );
    }

    #[test]
    fn routing_queries_survive_but_credentials_fragments_and_duplicate_models_do_not() {
        let url = endpoint(
            "wss://example.com/gateway/socket?tenant=a%2Fb&model=old&MODEL=older&API%5FKEY=synthetic-secret&accessToken=synthetic-secret&Authorization=synthetic-secret&X-Api-Key=synthetic-secret&client_secret=synthetic-secret&token=synthetic-secret&signature=synthetic-secret#synthetic-secret",
            "  synthetic model&api_key=not-a-query  ",
        )
        .unwrap();
        assert_eq!(url.fragment(), None);
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![
                ("tenant".into(), "a/b".into()),
                ("model".into(), "synthetic model&api_key=not-a-query".into()),
            ]
        );
        assert!(!url.as_str().contains("synthetic-secret"));
        assert!(url.as_str().contains("%26api_key%3Dnot-a-query"));
    }

    #[test]
    fn invalid_inputs_reject_userinfo_without_echoing_the_endpoint_or_secret() {
        for source in [
            "https://user:synthetic-secret@example.com/v1",
            "wss://user@example.com/socket",
            "https://@example.com",
            "https://:synthetic-secret@example.com",
            "file:///tmp/socket",
            "ftp://example.com/socket",
            "not an endpoint synthetic-secret",
        ] {
            let error = endpoint(source, "synthetic-model").unwrap_err();
            assert!(!error.to_string().contains("synthetic-secret"));
            assert!(!error.to_string().contains("example.com"));
        }
        assert!(endpoint("https://example.com", " \t").is_err());
    }
}
