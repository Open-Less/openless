use super::*;

#[tokio::test]
async fn waits_for_binary_setup_complete_before_starting_activity() {
    let (endpoint, server) = serve(|mut socket| async move {
        assert!(receive(&mut socket).await.get("setup").is_some());
        assert!(
            timeout(PENDING_WINDOW, socket.next()).await.is_err(),
            "audio or activity was sent before setupComplete"
        );
        send(&mut socket, json!({"setupComplete": {}}), true).await;
        assert_eq!(
            receive(&mut socket).await,
            json!({"realtimeInput": {"activityStart": {}}})
        );
        assert_closed(&mut socket).await;
    })
    .await;
    let asr = connect(&endpoint, Arc::new(TranscriptCapture::default())).await;
    asr.cancel();
    server.await.unwrap();
}

#[tokio::test]
async fn sends_raw_pcm_and_trailing_samples_before_activity_end() {
    let pcm = (0..6_502)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let expected = pcm.clone();
    let (endpoint, server) = serve(move |mut socket| async move {
        ready(&mut socket).await;
        assert_eq!(receive_audio_until_end(&mut socket).await, expected);
        send(
            &mut socket,
            json!({"serverContent": {
                "inputTranscription": {"text": "All audio arrived."},
                "generationComplete": true
            }}),
            true,
        )
        .await;
        assert_closed(&mut socket).await;
    })
    .await;
    let capture = Arc::new(TranscriptCapture::default());
    let asr = connect(&endpoint, capture.clone()).await;
    // Capture boundaries need not align to a sample or a network frame.
    asr.consume_pcm_chunk(&pcm[..101]);
    asr.consume_pcm_chunk(&pcm[101..4_105]);
    asr.consume_pcm_chunk(&pcm[4_105..]);
    let transcript = timeout(TEST_TIMEOUT, asr.finish()).await.unwrap().unwrap();
    assert_eq!(transcript.text, "All audio arrived.");
    assert_eq!(transcript.duration_ms, 203);
    capture.assert_snapshots(&["All audio arrived."]);
    server.await.unwrap();
}

#[tokio::test]
async fn replaces_interim_snapshots_and_waits_for_final_after_completion_marker() {
    let (ended, end_received) = oneshot::channel();
    let (allow_generation, generation_allowed) = oneshot::channel();
    let (generation_sent, generation_received) = oneshot::channel();
    let (allow_final, final_allowed) = oneshot::channel();
    let (endpoint, server) = serve(move |mut socket| async move {
        ready(&mut socket).await;
        for (content, binary) in [
            (
                json!({"interimInputTranscription": {"text": "Hell"}}),
                false,
            ),
            (
                json!({"interimInputTranscription": {"text": "Hello"}}),
                true,
            ),
            (json!({"inputTranscription": {"text": "Repeat."}}), false),
            (json!({"inputTranscription": {"text": "Repeat."}}), true),
            (
                json!({"generationComplete": true, "turnComplete": true}),
                true,
            ),
            (
                json!({"interimInputTranscription": {"text": "tail"}}),
                false,
            ),
        ] {
            send(&mut socket, json!({"serverContent": content}), binary).await;
        }
        assert_eq!(receive_audio_until_end(&mut socket).await, vec![1; 320]);
        ended.send(()).unwrap();
        generation_allowed.await.unwrap();
        send(
            &mut socket,
            json!({"serverContent": {
                "generationComplete": true, "turnComplete": true
            }}),
            true,
        )
        .await;
        generation_sent.send(()).unwrap();
        final_allowed.await.unwrap();
        send(
            &mut socket,
            json!({"serverContent": {
                "inputTranscription": {"text": "Done."}
            }}),
            false,
        )
        .await;
        assert_closed(&mut socket).await;
    })
    .await;
    let (capture, mut published) = notifying_capture();
    let asr = connect(&endpoint, capture.clone()).await;
    wait_for_snapshots(&mut published, 5).await;
    asr.consume_pcm_chunk(&[1; 320]);
    let mut finishing = tokio::spawn(async move { asr.finish().await });
    timeout(TEST_TIMEOUT, end_received).await.unwrap().unwrap();
    assert!(
        timeout(PENDING_WINDOW, &mut finishing).await.is_err(),
        "completion from before Stop must not finish the recording"
    );
    allow_generation.send(()).unwrap();
    timeout(TEST_TIMEOUT, generation_received)
        .await
        .unwrap()
        .unwrap();
    assert!(
        timeout(PENDING_WINDOW, &mut finishing).await.is_err(),
        "generationComplete must wait for authoritative input transcription"
    );
    allow_final.send(()).unwrap();
    let transcript = timeout(TEST_TIMEOUT, finishing)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(transcript.text, "Repeat. Repeat. Done.");
    capture.snapshots.assert_snapshots(&[
        "Hell",
        "Hello",
        "Repeat.",
        "Repeat. Repeat.",
        "Repeat. Repeat. tail",
        "Repeat. Repeat. Done.",
    ]);
    server.await.unwrap();
}

#[tokio::test]
async fn accepts_activity_end_for_the_500ms_silence_verification_probe() {
    let (endpoint, server) = serve(|mut socket| async move {
        ready(&mut socket).await;
        assert_eq!(receive_audio_until_end(&mut socket).await, vec![0; 16_000]);
        send(
            &mut socket,
            json!({"voiceActivity": {"type": "ACTIVITY_END"}}),
            true,
        )
        .await;
        assert_closed(&mut socket).await;
    })
    .await;
    let capture = Arc::new(TranscriptCapture::default());
    let asr = connect(&endpoint, capture.clone()).await;
    asr.consume_pcm_chunk(&[0; 16_000]);
    let transcript = timeout(TEST_TIMEOUT, asr.finish()).await.unwrap().unwrap();
    assert_eq!(transcript.text, "");
    assert_eq!(transcript.duration_ms, 500);
    capture.assert_snapshots(&[]);
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_and_session_drop_release_the_socket() {
    for cancel in [true, false] {
        let (endpoint, server) = serve(|mut socket| async move {
            ready(&mut socket).await;
            send(
                &mut socket,
                json!({"serverContent": {
                    "interimInputTranscription": {"text": "Unconfirmed"}
                }}),
                true,
            )
            .await;
            assert_closed(&mut socket).await;
        })
        .await;
        let (capture, mut published) = notifying_capture();
        let asr = connect(&endpoint, capture.clone()).await;
        wait_for_snapshots(&mut published, 1).await;
        if cancel {
            asr.cancel();
            asr.consume_pcm_chunk(&[1; 320]);
            assert!(timeout(TEST_TIMEOUT, asr.finish()).await.unwrap().is_err());
        } else {
            drop(asr);
        }
        server.await.unwrap();
        capture.snapshots.assert_snapshots(&["Unconfirmed"]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_an_idle_writer_prevents_later_preview_delivery() {
    let (cancelled, cancellation_received) = oneshot::channel();
    let (endpoint, server) = serve(move |mut socket| async move {
        ready(&mut socket).await;
        send(
            &mut socket,
            json!({"serverContent": {"interimInputTranscription": {"text": "Before cancellation"}}}),
            true,
        )
        .await;
        cancellation_received.await.unwrap();
        // The peer may have already closed; either outcome must suppress this preview.
        let _ = socket.send(Message::text(json!({"serverContent": {
            "interimInputTranscription": {"text": "After cancellation"}
        }}).to_string())).await;
        assert_closed(&mut socket).await;
    })
    .await;
    let (capture, mut published) = notifying_capture();
    let asr = connect(&endpoint, capture.clone()).await;
    wait_for_snapshots(&mut published, 1).await;
    asr.cancel();
    cancelled.send(()).unwrap();
    assert!(timeout(TEST_TIMEOUT, asr.finish()).await.unwrap().is_err());
    server.await.unwrap();
    capture.snapshots.assert_snapshots(&["Before cancellation"]);
}

#[tokio::test]
async fn dropping_finish_closes_the_socket_even_when_the_session_is_retained() {
    let (ended, end_received) = oneshot::channel();
    let (endpoint, server) = serve(move |mut socket| async move {
        ready(&mut socket).await;
        send(
            &mut socket,
            json!({"serverContent": {
                "interimInputTranscription": {"text": "Unconfirmed"}
            }}),
            false,
        )
        .await;
        receive_audio_until_end(&mut socket).await;
        ended.send(()).unwrap();
        assert_closed(&mut socket).await;
    })
    .await;
    let asr = connect(&endpoint, Arc::new(TranscriptCapture::default())).await;
    asr.consume_pcm_chunk(&[1; 320]);
    let retained = asr.clone();
    let finishing = tokio::spawn(async move { asr.finish().await });
    timeout(TEST_TIMEOUT, end_received).await.unwrap().unwrap();
    finishing.abort();
    assert!(finishing.await.unwrap_err().is_cancelled());
    server.await.unwrap();
    assert!(retained.finish().await.is_err());
}

#[tokio::test]
async fn dropping_pending_setup_releases_the_socket() {
    let (setup, setup_received) = oneshot::channel();
    let (endpoint, server) = serve(move |mut socket| async move {
        assert!(receive(&mut socket).await.get("setup").is_some());
        setup.send(()).unwrap();
        assert_closed(&mut socket).await;
    })
    .await;
    let connecting =
        tokio::spawn(
            async move { connect(&endpoint, Arc::new(TranscriptCapture::default())).await },
        );
    timeout(TEST_TIMEOUT, setup_received)
        .await
        .unwrap()
        .unwrap();
    connecting.abort();
    assert!(connecting.await.err().unwrap().is_cancelled());
    server.await.unwrap();
}

#[tokio::test]
async fn server_error_or_disconnect_never_returns_a_partial_transcript_as_success() {
    for server_error in [true, false] {
        let (endpoint, server) = serve(move |mut socket| async move {
            ready(&mut socket).await;
            send(
                &mut socket,
                json!({"serverContent": {
                    "interimInputTranscription": {"text": "Only a preview"}
                }}),
                true,
            )
            .await;
            receive_audio_until_end(&mut socket).await;
            if server_error {
                send(
                    &mut socket,
                    json!({"error": {
                        "code": 429, "message": format!("invalid credential {API_KEY}")
                    }}),
                    true,
                )
                .await;
            } else {
                socket.send(Message::Close(None)).await.unwrap();
            }
            assert_closed(&mut socket).await;
        })
        .await;
        let capture = Arc::new(TranscriptCapture::default());
        let asr = connect(&endpoint, capture.clone()).await;
        asr.consume_pcm_chunk(&[1; 320]);
        let error = timeout(TEST_TIMEOUT, asr.finish())
            .await
            .unwrap()
            .unwrap_err()
            .to_string();
        assert!(!error.contains(API_KEY));
        if server_error {
            assert!(error.contains("429"));
        }
        capture.assert_snapshots(&["Only a preview"]);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn final_received_before_stop_does_not_require_a_duplicate_after_stop() {
    let (endpoint, server) = serve(|mut socket| async move {
        ready(&mut socket).await;
        let audio = receive(&mut socket).await;
        assert!(audio["realtimeInput"]["audio"].is_object());
        send(
            &mut socket,
            json!({"serverContent": {"inputTranscription": {"text": "Already final."}}}),
            true,
        )
        .await;
        assert!(receive_audio_until_end(&mut socket).await.is_empty());
        send(
            &mut socket,
            json!({"serverContent": {"generationComplete": true, "turnComplete": true}}),
            true,
        )
        .await;
        assert_closed(&mut socket).await;
    })
    .await;
    let (capture, mut published) = notifying_capture();
    let asr = connect(&endpoint, capture.clone()).await;
    asr.consume_pcm_chunk(&[1; 3_200]);
    wait_for_snapshots(&mut published, 1).await;
    let transcript = timeout(TEST_TIMEOUT, asr.finish()).await.unwrap().unwrap();
    assert_eq!(transcript.text, "Already final.");
    assert_eq!(transcript.duration_ms, 100);
    capture.snapshots.assert_snapshots(&["Already final."]);
    server.await.unwrap();
}
