use super::*;

const REPLAY_TIMEOUT: Duration = Duration::from_secs(15);

#[tokio::test(flavor = "current_thread")]
async fn buffered_30_second_recording_can_be_fed_in_one_synchronous_call() {
    assert_buffered_recording(32_000 * 30 + 102, 30_003).await;
}

#[tokio::test(flavor = "current_thread")]
async fn recording_at_the_limit_reserves_room_for_its_tail_and_finish_marker() {
    assert_buffered_recording(32_000 * 600 - 2, 599_999).await;
}

/// On one executor thread, the network worker cannot drain the queue during consume.
async fn assert_buffered_recording(byte_count: usize, expected_duration_ms: u64) {
    let pcm = (0..byte_count)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let expected = pcm.clone();
    let (endpoint, server) = serve_with_timeout(REPLAY_TIMEOUT, move |mut socket| async move {
        ready(&mut socket).await;
        let received = receive_audio_until_end(&mut socket).await;
        assert_eq!(received.len(), expected.len());
        assert!(
            received == expected,
            "buffered PCM was changed or reordered"
        );
        send(
            &mut socket,
            json!({"serverContent": {
                "inputTranscription": {"text": "Buffered recording completed."},
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
    asr.consume_pcm_chunk(&pcm);
    let transcript = timeout(REPLAY_TIMEOUT, asr.finish())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(transcript.text, "Buffered recording completed.");
    assert_eq!(transcript.duration_ms, expected_duration_ms);
    capture.assert_snapshots(&["Buffered recording completed."]);
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn recording_longer_than_ten_minutes_fails_without_sending_audio() {
    let (endpoint, server) = serve(|mut socket| async move {
        ready(&mut socket).await;
        assert_closed(&mut socket).await;
    })
    .await;
    let capture = Arc::new(TranscriptCapture::default());
    let asr = connect(&endpoint, capture.clone()).await;
    asr.consume_pcm_chunk(&vec![1; 32_000 * 600 + 2]);
    let error = timeout(TEST_TIMEOUT, asr.finish())
        .await
        .unwrap()
        .unwrap_err()
        .to_string();
    assert!(error.contains("10 minutes"));
    capture.assert_snapshots(&[]);
    server.await.unwrap();
}
