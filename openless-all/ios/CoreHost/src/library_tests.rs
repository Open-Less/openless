use super::*;
use super::control_tests::{control, open_at, result};
use super::tests::{next, reply, send};

fn entry(id: &str, text: &str) -> openless_core::types::DictationSession {
    serde_json::from_value(json!({"id":id,"createdAt":"2099-01-01T00:00:00Z", "rawTranscript":text,
        "finalText":text, "mode":"raw", "insertStatus":"notRequested", "durationMs":1000})).unwrap()
}
#[test]
fn normal_library_has_bounded_previews_full_detail_and_independent_activity() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(dir.path(), false, Arc::new(credentials::MemoryVault::default()));
    let owner = host(handle).unwrap(); let id = Uuid::new_v4().to_string(); let text = "你好👩🏽‍💻".repeat(200);
    owner.backend.append_history(entry(&id, &text), 0, Some(200)).unwrap();
    owner.backend.record_activity("2099-01-01", 42, 2000).unwrap();
    let library = result(control(handle, context, json!({"action":"library"})));
    assert_eq!(library["canPolish"], false);
    assert_eq!(library["history"][0]["characterCount"], text.chars().count());
    assert!(library["history"][0]["rawTranscript"].as_str().unwrap().chars().count() <= 160);
    assert_eq!(library["history"][0]["isPreview"], true);
    let detail = result(control(handle, context, json!({"action":"historyDetail","id":id})));
    assert_eq!(detail["rawTranscript"], text);
    result(control(handle, context, json!({"action":"clearLibraryHistory"})));
    let library = result(control(handle, context, json!({"action":"library"})));
    assert_eq!(library["history"], json!([]));
    assert_eq!(library["activity"][0]["chars"], 42);
    assert!(library.get("channels").is_none());
    ol_ios_destroy(handle);
}
#[test]
fn vocabulary_crud_is_public_persisted_idempotent_and_validated() {
    let dir = tempfile::tempdir().unwrap(); let vault = Arc::new(credentials::MemoryVault::default());
    let (handle, context) = open_at(dir.path(), false, vault.clone());
    result(control(handle, context, json!({"action":"addVocabulary","phrase":" OpenLess ","note":"产品名称"})));
    result(control(handle, context, json!({"action":"addVocabulary","phrase":"OpenLess"})));
    let words = result(control(handle, context, json!({"action":"vocabulary","query":"openless"})));
    assert_eq!(words["total"], 1);
    let id = words["entries"][0]["id"].as_str().unwrap();
    result(control(handle, context, json!({"action":"setVocabularyEnabled","id":id,"enabled":false})));
    result(control(handle, context, json!({"action":"updateVocabulary","id":id,"phrase":"OpenLess iOS"})));
    assert_eq!(control(handle, context, json!({"action":"addVocabulary","phrase":"first\nsecond"}))["ok"], false);
    ol_ios_destroy(handle);
    let (handle, context) = open_at(dir.path(), false, vault);
    let words = result(control(handle, context, json!({"action":"vocabulary"})));
    assert_eq!(words["entries"][0]["enabled"], false);
    assert_eq!(words["entries"][0]["phrase"], "OpenLess iOS");
    result(control(handle, context, json!({"action":"removeVocabulary","id":id})));
    assert_eq!(result(control(handle, context, json!({"action":"vocabulary"})))["total"], 0);
    ol_ios_destroy(handle);
}
#[test]
fn raw_repolish_uses_core_auxiliary_without_overwriting_history() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(dir.path(), true, Arc::new(credentials::MemoryVault::default()));
    let owner = host(handle).unwrap(); let id = Uuid::new_v4().to_string();
    let mut record = entry(&id, "保留原文"); record.final_text = "旧结果".into();
    owner.backend.append_history(record, 0, Some(200)).unwrap();
    assert_eq!(control(handle, context, json!({"action":"repolishHistory","id":id,"stylePackId":"builtin.light"}))["ok"], false);
    let preview = result(control(handle, context, json!({"action":"repolishHistory","id":id,"stylePackId":"builtin.raw"})));
    assert_eq!(preview["text"], "保留原文");
    assert_eq!(owner.history_entry(&id).unwrap().final_text, "旧结果");
    ol_ios_destroy(handle);
}
#[test]
fn failed_transcription_retains_actual_audio_and_delete_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(dir.path(), false, Arc::new(credentials::MemoryVault::default()));
    let client = Uuid::new_v4();
    assert_eq!(send(handle, json!({"operation":"start","clientSessionID":client})), 0);
    let id = loop {
        let event = next(context);
        match event["kind"].as_str() {
            Some("nativeRequest") => reply(handle, &event, Value::Null),
            Some("started") => break event["sessionID"].as_str().unwrap().to_string(),
            Some("failed") => panic!("{event}"), _ => {}
        }
    };
    let c_id = CString::new(id.clone()).unwrap(); let pcm = vec![0u8; 3200];
    assert_eq!(unsafe { ol_ios_feed_pcm(handle, c_id.as_ptr(), pcm.as_ptr(), pcm.len()) }, 0);
    assert_eq!(send(handle, json!({"operation":"stop","clientSessionID":client})), 0);
    loop {
        let event = next(context);
        match event["kind"].as_str() {
            Some("nativeRequest") => {
                if event["operation"] == "speechFinish" {
                    let data = serde_json::to_vec(&json!({"requestID":event["requestID"],"ok":false,"error":"fixture transcription failed"})).unwrap();
                    assert_eq!(unsafe { ol_ios_reply(handle, data.as_ptr(), data.len()) }, 0);
                } else { reply(handle, &event, Value::Null); }
            },
            Some("failed") => break,
            Some("completed") => panic!("empty ASR must fail"), _ => {}
        }
    }
    for _ in 0..100 { if host(handle).unwrap().admission.lock().unwrap().is_none() { break; } std::thread::sleep(Duration::from_millis(1)); }
    let detail = result(control(handle, context, json!({"action":"historyDetail","id":id})));
    assert_eq!(detail["errorCode"], "transcribeFailed");
    assert_eq!(detail["hasAudioRecording"], true);
    let asset = result(control(handle, context, json!({"action":"audioAsset","id":id})));
    let path = PathBuf::from(asset["path"].as_str().unwrap());
    assert_eq!(std::fs::read(&path).unwrap().len(), 44 + pcm.len());
    result(control(handle, context, json!({"action":"deleteHistory","id":id})));
    assert!(!path.exists());
    assert_eq!(control(handle, context, json!({"action":"audioAsset","id":"../outside"}))["ok"], false);
    ol_ios_destroy(handle);
}

#[test]
fn history_search_matches_full_text_and_asr_without_exporting_full_records() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(dir.path(), false, Arc::new(credentials::MemoryVault::default()));
    let owner = host(handle).unwrap();
    let mut record = entry("search-full", &("前".repeat(250) + "Needle"));
    record.asr_transcript = Some("独立识别原文".into());
    owner.backend.append_history(record, 0, Some(200)).unwrap();
    for query in [" needle ", "独立识别", ""] {
        let page = result(control(handle, context, json!({"action":"searchHistory", "query":query})));
        assert_eq!(page["entries"].as_array().unwrap().len(), 1);
        assert_eq!(page["entries"][0]["id"], "search-full");
        assert!(page["entries"][0]["rawTranscript"].as_str().unwrap().chars().count() <= 160);
    }
    let page = result(control(handle, context, json!({"action":"searchHistory", "query":"missing"})));
    assert_eq!(page["entries"], json!([]));
    ol_ios_destroy(handle);
}

#[test]
fn ordinary_history_never_retains_more_than_two_hundred_entries() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(dir.path(), false, Arc::new(credentials::MemoryVault::default()));
    let owner = host(handle).unwrap();
    for index in 0..205 {
        owner.backend.append_history(entry(&format!("cap-{index}"), "retention fixture"), 0, Some(1000)).unwrap();
    }
    assert_eq!(owner.backend.list_history().unwrap().len(), 200);
    let page = result(control(handle, context, json!({"action":"searchHistory", "query":"fixture"})));
    assert_eq!(page["entries"].as_array().unwrap().len(), 200);
    ol_ios_destroy(handle);
}
