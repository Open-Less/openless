#[path = "../../src/style_zip_transfer.rs"]
mod style_zip_transfer;

use std::cell::RefCell;
use openless_core::StylePackStore;

#[test]
fn content_document_receives_actual_core_zip_and_can_be_reimported() {
    let store = StylePackStore::in_memory();
    let count_before = store.list().unwrap().len();
    assert!(store.import_from_zip_bytes(&[]).is_err());
    assert_eq!(store.list().unwrap().len(), count_before);
    let document = RefCell::new(Vec::new());
    style_zip_transfer::export_zip(
        "content://test-provider/opaque-document",
        || store.export_zip_bytes("builtin.light").map_err(|error| error.to_string()),
        |_, bytes| { document.replace(bytes.to_vec()); Ok(()) },
        || -> Result<(), String> { panic!("content URI must never go through filesystem export") },
    ).unwrap();
    let bytes = document.borrow();
    assert!(!bytes.is_empty());
    let imported = store.import_from_zip_bytes(&bytes).unwrap();
    assert_eq!(imported.prompt, store.get("builtin.light").unwrap().prompt);
}

#[test]
fn ordinary_paths_keep_file_export_and_resolver_failures_do_not_report_success() {
    let result = style_zip_transfer::export_zip(
        "fixture.zip", || -> Result<Vec<u8>, &str> { panic!("file branch must not encode twice") },
        |_, _| -> Result<(), &str> { panic!("file branch must not call resolver") }, || Ok(()),
    );
    assert_eq!(result, Ok(()));
    let result = style_zip_transfer::export_zip(
        "content://test-provider/opaque-document", || Ok(vec![1, 2, 3]),
        |_, _| Err("write_failed"), || -> Result<(), &str> { panic!("no path fallback on write error") },
    );
    assert_eq!(result, Err("write_failed"));
    let result = style_zip_transfer::export_zip(
        "content://test-provider/opaque-document", || Err("encode_failed"),
        |_, _| -> Result<(), &str> { panic!("encoding failed; do not touch destination") },
        || -> Result<(), &str> { panic!("no fallback") },
    );
    assert_eq!(result, Err("encode_failed"));
}
