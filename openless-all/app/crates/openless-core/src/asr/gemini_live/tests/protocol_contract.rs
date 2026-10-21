use super::*;

#[test]
fn setup_selects_each_transcription_mode_and_structured_hints() {
    for (mode, expected) in [
        (TranscriptionMode::Verbatim, "VERBATIM"),
        (TranscriptionMode::Smart, "SMART"),
    ] {
        let mut config = config("https://generativelanguage.googleapis.com/v1beta");
        config.mode = mode;
        config.languages = vec!["zh-CN".into(), "yue".into(), "en".into(), " ".into()];
        config.hotwords = vec![" Kubernetes ".into(), "".into(), "Kubernetes".into()];
        let Message::Text(message) = protocol::setup(&config) else {
            panic!("setup must be a JSON message")
        };
        let message: Value = serde_json::from_str(&message).unwrap();
        let setup = &message["setup"];
        assert_eq!(setup["model"], format!("models/{MODEL}"));
        assert_eq!(
            setup["generationConfig"]["responseModalities"],
            json!(["TEXT"])
        );
        assert_eq!(
            setup["realtimeInputConfig"]["automaticActivityDetection"]["disabled"],
            true
        );
        assert_eq!(setup["inputAudioTranscription"]["mode"], expected);
        assert_eq!(
            setup["inputAudioTranscription"]["languageCodes"],
            json!(["cmn-Hans-CN", "yue-Hant-HK", "en"])
        );
        assert_eq!(
            setup["inputAudioTranscription"]["customVocabulary"],
            json!(["Kubernetes"])
        );
    }
}

#[test]
fn errors_keep_server_code_without_echoing_credentials() {
    let body = json!({"error": {
        "code": 429,
        "message": format!("rejected key={API_KEY} at https://gateway.invalid/?access_token=secret")
    }});
    for message in [
        Message::text(body.to_string()),
        Message::binary(serde_json::to_vec(&body).unwrap()),
    ] {
        let error = protocol::decode(message).unwrap_err().to_string();
        assert!(error.contains("429"));
        assert!(!error.contains(API_KEY));
        assert!(!error.contains("access_token"));
        assert!(!error.contains("gateway.invalid"));
    }
    let malformed = protocol::decode(Message::binary(API_KEY.as_bytes().to_vec()))
        .unwrap_err()
        .to_string();
    assert!(!malformed.contains(API_KEY));
    assert!(protocol::decode(Message::Close(None)).is_err());
    assert!(protocol::decode(Message::text(json!({"goAway": {}}).to_string())).is_err());
}

#[test]
fn turn_complete_alone_never_promotes_interim_text() {
    let mut transcript = protocol::Transcript::default();
    transcript.update(
        &json!({"serverContent": {
            "inputTranscription": {"text": "Confirmed."}, "generationComplete": true
        }}),
        false,
    );
    transcript.begin_finish();
    transcript.update(
        &json!({"serverContent": {
            "interimInputTranscription": {"text": "Unconfirmed"}, "turnComplete": true
        }}),
        true,
    );
    assert!(!transcript.can_finish(false));
    assert_eq!(transcript.final_text(), "Confirmed.");
    transcript.update(&json!({"voiceActivity": {"type": "ACTIVITY_END"}}), true);
    assert!(!transcript.can_finish(false));
    transcript.update(
        &json!({"serverContent": {"inputTranscription": {"text": "Confirmed."}}}),
        true,
    );
    assert!(transcript.can_finish(false));
    assert_eq!(transcript.final_text(), "Confirmed. Confirmed.");
}
