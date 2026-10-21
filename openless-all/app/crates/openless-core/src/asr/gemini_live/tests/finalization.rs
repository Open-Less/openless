use super::super::FINISH_TIMEOUT;
use super::*;

#[tokio::test]
async fn completion_before_multiple_finals_restarts_the_quiet_window() {
    for marker in [
        json!({"serverContent": {"generationComplete": true, "turnComplete": true}}),
        json!({"voiceActivity": {"type": "ACTIVITY_END"}}),
    ] {
        let (release, released) = oneshot::channel();
        let (endpoint, server) = serve(move |mut socket| async move {
            ready(&mut socket).await;
            receive_audio_until_end(&mut socket).await;
            send(&mut socket, marker, true).await;
            send(
                &mut socket,
                json!({"serverContent": {
                    "inputTranscription": {"text": "First."}
                }}),
                true,
            )
            .await;
            released.await.unwrap();
            send(
                &mut socket,
                json!({"serverContent": {
                    "inputTranscription": {"text": "Second."}
                }}),
                false,
            )
            .await;
            assert_closed(&mut socket).await;
        })
        .await;
        let (capture, mut published) = notifying_capture();
        let asr = connect(&endpoint, capture.clone()).await;
        asr.consume_pcm_chunk(&[1; 320]);
        let mut finishing = tokio::spawn(async move { asr.finish().await });
        wait_for_snapshots(&mut published, 1).await;
        assert!(timeout(Duration::from_millis(250), &mut finishing)
            .await
            .is_err());
        release.send(()).unwrap();
        wait_for_snapshots(&mut published, 1).await;
        // This crosses the first segment's deadline, but not the second segment's deadline.
        assert!(timeout(Duration::from_millis(300), &mut finishing)
            .await
            .is_err());
        let transcript = timeout(TEST_TIMEOUT, finishing)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(transcript.text, "First. Second.");
        capture
            .snapshots
            .assert_snapshots(&["First.", "First. Second."]);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn new_interim_during_drain_waits_for_its_final_revision() {
    let (release_interim, interim_allowed) = oneshot::channel();
    let (release_final, final_allowed) = oneshot::channel();
    let (endpoint, server) = serve(move |mut socket| async move {
        ready(&mut socket).await;
        receive_audio_until_end(&mut socket).await;
        send(
            &mut socket,
            json!({"serverContent": {
                "inputTranscription": {"text": "First."}, "generationComplete": true
            }}),
            true,
        )
        .await;
        interim_allowed.await.unwrap();
        send(
            &mut socket,
            json!({"serverContent": {
                "interimInputTranscription": {"text": "Unconfirmed"}
            }}),
            true,
        )
        .await;
        final_allowed.await.unwrap();
        send(
            &mut socket,
            json!({"serverContent": {
                "inputTranscription": {"text": "Confirmed."}
            }}),
            true,
        )
        .await;
        assert_closed(&mut socket).await;
    })
    .await;
    let (capture, mut published) = notifying_capture();
    let asr = connect(&endpoint, capture.clone()).await;
    asr.consume_pcm_chunk(&[1; 320]);
    let mut finishing = tokio::spawn(async move { asr.finish().await });
    wait_for_snapshots(&mut published, 1).await;
    release_interim.send(()).unwrap();
    wait_for_snapshots(&mut published, 1).await;
    assert!(timeout(Duration::from_millis(600), &mut finishing)
        .await
        .is_err());
    release_final.send(()).unwrap();
    let transcript = timeout(TEST_TIMEOUT, finishing)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(transcript.text, "First. Confirmed.");
    capture
        .snapshots
        .assert_snapshots(&["First.", "First. Unconfirmed", "First. Confirmed."]);
    server.await.unwrap();
}

#[tokio::test]
async fn metadata_and_repeated_completion_markers_do_not_extend_the_quiet_window() {
    let (endpoint, server) = serve(|mut socket| async move {
        ready(&mut socket).await;
        receive_audio_until_end(&mut socket).await;
        send(
            &mut socket,
            json!({"serverContent": {
                "inputTranscription": {"text": "Complete."}, "generationComplete": true
            }}),
            true,
        )
        .await;
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                message = socket.next() => {
                    assert!(matches!(message, None | Some(Ok(Message::Close(_))) | Some(Err(_))));
                    break;
                }
                _ = interval.tick() => {
                    send(&mut socket, json!({"serverContent": {"generationComplete": true},
                        "usageMetadata": {"totalTokenCount": 1}}), true).await;
                }
            }
        }
    })
    .await;
    let asr = connect(&endpoint, Arc::new(TranscriptCapture::default())).await;
    asr.consume_pcm_chunk(&[1; 320]);
    let transcript = timeout(Duration::from_millis(1_200), asr.finish())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(transcript.text, "Complete.");
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_during_drain_does_not_return_the_confirmed_text() {
    let (endpoint, server) = serve(|mut socket| async move {
        ready(&mut socket).await;
        receive_audio_until_end(&mut socket).await;
        send(
            &mut socket,
            json!({"serverContent": {
                "inputTranscription": {"text": "Do not submit."}, "generationComplete": true
            }}),
            true,
        )
        .await;
        assert_closed(&mut socket).await;
    })
    .await;
    let (capture, mut published) = notifying_capture();
    let asr = connect(&endpoint, capture).await;
    asr.consume_pcm_chunk(&[1; 320]);
    let retained = asr.clone();
    let finishing = tokio::spawn(async move { asr.finish().await });
    wait_for_snapshots(&mut published, 1).await;
    retained.cancel();
    let error = timeout(TEST_TIMEOUT, finishing)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    server.await.unwrap();
}

#[tokio::test]
async fn normal_close_completes_drain_but_provider_errors_still_fail() {
    use tokio_tungstenite::tungstenite::{
        protocol::frame::coding::CloseCode, protocol::CloseFrame,
    };
    for (termination, succeeds) in [
        (Message::Close(None), true),
        (
            Message::Close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "failure".into(),
            })),
            false,
        ),
        (
            Message::text(json!({"error": {"code": 429}}).to_string()),
            false,
        ),
    ] {
        let (endpoint, server) = serve(move |mut socket| async move {
            ready(&mut socket).await;
            receive_audio_until_end(&mut socket).await;
            send(
                &mut socket,
                json!({"serverContent": {
                    "inputTranscription": {"text": "Complete."}, "generationComplete": true
                }}),
                true,
            )
            .await;
            socket.send(termination).await.unwrap();
            assert_closed(&mut socket).await;
        })
        .await;
        let asr = connect(&endpoint, Arc::new(TranscriptCapture::default())).await;
        asr.consume_pcm_chunk(&[1; 320]);
        let result = timeout(TEST_TIMEOUT, asr.finish()).await.unwrap();
        assert_eq!(result.is_ok(), succeeds);
        if succeeds {
            assert_eq!(result.unwrap().text, "Complete.");
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn new_transcripts_cannot_extend_the_total_finish_timeout() {
    let (endpoint, server) = serve_with_timeout(FINISH_TIMEOUT + TEST_TIMEOUT, |mut socket| async move {
        ready(&mut socket).await;
        receive_audio_until_end(&mut socket).await;
        send(&mut socket, json!({"serverContent": {"generationComplete": true}}), true).await;
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                message = socket.next() => {
                    assert!(matches!(message, None | Some(Ok(Message::Close(_))) | Some(Err(_))));
                    break;
                }
                _ = interval.tick() => {
                    send(&mut socket, json!({"serverContent": {
                        "inputTranscription": {"text": "Repeated."}
                    }}), true).await;
                }
            }
        }
    }).await;
    let asr = connect(&endpoint, Arc::new(TranscriptCapture::default())).await;
    asr.consume_pcm_chunk(&[1; 320]);
    let error = timeout(FINISH_TIMEOUT + Duration::from_secs(2), asr.finish())
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("timed out"));
    server.await.unwrap();
}

/// Keep the worker unpolled until both the inbound frame and quiet timer are ready.
async fn assert_pending_frame_wins_quiet_deadline(server_error: bool) {
    struct QueuedSpawner(mpsc::UnboundedSender<futures_util::future::BoxFuture<'static, ()>>);
    impl crate::TaskSpawner for QueuedSpawner {
        fn spawn(&self, task: futures_util::future::BoxFuture<'static, ()>) {
            self.0.send(task).unwrap();
        }
    }
    let (release, released) = oneshot::channel();
    let (sent, received) = oneshot::channel();
    let (endpoint, server) = serve(move |mut socket| async move {
        ready(&mut socket).await;
        receive_audio_until_end(&mut socket).await;
        send(
            &mut socket,
            json!({"serverContent": {
                "inputTranscription": {"text": "First."}, "generationComplete": true
            }}),
            true,
        )
        .await;
        released.await.unwrap();
        let late = if server_error {
            json!({"error": {"code": 429}})
        } else {
            json!({"serverContent": {"inputTranscription": {"text": "Second."}}})
        };
        send(&mut socket, late, true).await;
        sent.send(()).unwrap();
        assert_closed(&mut socket).await;
    })
    .await;
    let (tasks, mut queued) = mpsc::unbounded_channel();
    let (capture, mut published) = notifying_capture();
    let asr = GeminiLiveASR::connect(
        config(&endpoint),
        capture.clone(),
        Arc::new(QueuedSpawner(tasks)),
    )
    .await
    .unwrap();
    asr.consume_pcm_chunk(&[1; 320]);
    let mut worker = queued.recv().await.unwrap();
    let mut finishing = Box::pin(asr.finish());
    timeout(TEST_TIMEOUT, async {
        tokio::select! {
            biased;
            result = &mut finishing => panic!("finish returned before drain: {result:?}"),
            _ = &mut worker => panic!("worker returned before drain"),
            event = published.recv() => event.unwrap(),
        }
    })
    .await
    .unwrap();
    release.send(()).unwrap();
    timeout(TEST_TIMEOUT, received).await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let (_, result) = timeout(TEST_TIMEOUT, async {
        tokio::join!(&mut worker, &mut finishing)
    })
    .await
    .unwrap();
    server.await.unwrap();
    if server_error {
        assert!(result.unwrap_err().to_string().contains("429"));
    } else {
        assert_eq!(result.unwrap().text, "First. Second.");
        capture
            .snapshots
            .assert_snapshots(&["First.", "First. Second."]);
    }
}

#[tokio::test]
async fn pending_transcript_is_processed_before_an_expired_quiet_timer() {
    assert_pending_frame_wins_quiet_deadline(false).await;
}

#[tokio::test]
async fn pending_server_error_is_processed_before_an_expired_quiet_timer() {
    assert_pending_frame_wins_quiet_deadline(true).await;
}
