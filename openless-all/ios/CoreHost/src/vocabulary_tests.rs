use super::*;
use super::control_tests::{control, open_at, result};
use openless_core::shared_types::LEARNED_VOCAB_NOTE;

#[test]
fn source_and_search_are_filtered_before_total_and_pagination() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, context) = open_at(dir.path(), false, Arc::new(credentials::MemoryVault::default()));
    let owner = host(handle).unwrap();
    // More than one page; source interleaving must not produce sparse pages.
    for index in 0..105 {
        owner.backend.add_vocabulary_if_absent(format!("Example learned {index}"), Some(LEARNED_VOCAB_NOTE.into())).unwrap();
        owner.backend.add_vocabulary_if_absent(format!("Example manual {index}"), None).unwrap();
    }
    let learned = result(control(handle, context, json!({"action":"vocabulary", "source":"learned", "query":" EXAMPLE "})));
    assert_eq!(learned["total"], 105);
    assert_eq!(learned["entries"].as_array().unwrap().len(), 100);
    assert!(learned["entries"].as_array().unwrap().iter().all(|entry| entry["source"] == "learned"));
    let tail = result(control(handle, context, json!({"action":"vocabulary", "source":"manual", "offset":100})));
    assert_eq!(tail["total"], 105);
    assert_eq!(tail["entries"].as_array().unwrap().len(), 5);
    assert!(tail["entries"].as_array().unwrap().iter().all(|entry| entry["source"] == "manual"));
    let none = result(control(handle, context, json!({"action":"vocabulary", "source":"learned", "query":"manual"})));
    assert_eq!(none["total"], 0);
    assert_eq!(result(control(handle, context, json!({"action":"vocabulary"})))["total"], 210);
    assert_eq!(result(control(handle, context, json!({"action":"vocabulary", "offset":usize::MAX})))["entries"], json!([]));
    assert_eq!(control(handle, context, json!({"action":"vocabulary", "source":"future-unknown-filter"}))["ok"], false);
    ol_ios_destroy(handle);
}
