use super::*;
use std::collections::VecDeque;

static EVENTS: OnceLock<Mutex<HashMap<u64, VecDeque<Value>>>> = OnceLock::new();
fn events() -> &'static Mutex<HashMap<u64, VecDeque<Value>>> {
    EVENTS.get_or_init(Default::default)
}
pub(super) extern "C" fn event(context: u64, data: *const u8, len: usize) {
    let value: Value =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(data, len) }).unwrap();
    events()
        .lock()
        .unwrap()
        .entry(context)
        .or_default()
        .push_back(value);
}
pub(super) extern "C" fn audio(
    _context: u64,
    _session: *const c_char,
    _data: *const u8,
    _len: usize,
) {
}
pub(super) fn send(handle: u64, command: Value) -> i32 {
    let data = serde_json::to_vec(&command).unwrap();
    unsafe { ol_ios_command(handle, data.as_ptr(), data.len()) }
}
pub(super) fn next(context: u64) -> Value {
    for _ in 0..400 {
        if let Some(value) = events()
            .lock()
            .unwrap()
            .entry(context)
            .or_default()
            .pop_front()
        {
            return value;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for callback")
}
pub(super) fn reply(handle: u64, request: &Value, value: Value) {
    let data =
        serde_json::to_vec(&json!({"requestID":request["requestID"],"ok":true,"value":value}))
            .unwrap();
    assert_eq!(
        unsafe { ol_ios_reply(handle, data.as_ptr(), data.len()) },
        0
    );
}
fn opened() -> (u64, u64, tempfile::TempDir) {
    static CONTEXT: AtomicU64 = AtomicU64::new(1);
    let context = CONTEXT.fetch_add(1, Ordering::Relaxed);
    let directory = tempfile::tempdir().unwrap();
    let handle = create_with_store(
        Config {
            data_dir: directory.path().to_path_buf(),
            developer_mode: false,
        },
        Callbacks {
            event,
            audio,
            context,
        },
        Arc::new(
            credentials::IosCredentialStore::open(Arc::new(credentials::MemoryVault::default()))
                .unwrap(),
        ),
    )
    .unwrap();
    assert_ne!(handle, 0);
    (handle, context, directory)
}
#[test]
fn native_adapters_run_the_real_core_pipeline_and_history() {
    let (handle, context, directory) = opened();
    let client = Uuid::new_v4();
    assert_eq!(
        send(
            handle,
            json!({"operation":"start","clientSessionID":client,"insert":true})
        ),
        0
    );
    let core_session = loop {
        let value = next(context);
        match value["kind"].as_str() {
            Some("nativeRequest") => reply(handle, &value, Value::Null),
            Some("started") => break value["sessionID"].as_str().unwrap().to_string(),
            Some("failed") | Some("commandError") => panic!("{value}"),
            _ => {}
        }
    };
    let session = CString::new(core_session).unwrap();
    let pcm = vec![16u8; 32_000];
    assert_eq!(
        unsafe { ol_ios_feed_pcm(handle, session.as_ptr(), pcm.as_ptr(), pcm.len()) },
        0
    );
    assert_eq!(
        send(handle, json!({"operation":"stop","clientSessionID":client})),
        0
    );
    loop {
        let value = next(context);
        match value["kind"].as_str() {
            Some("nativeRequest") => {
                let result = match value["operation"].as_str() {
                    Some("speechFinish") => json!({"text":"你好 OpenLess","durationMs":1000}),
                    Some("insert") => {
                        assert_eq!(value["payload"]["text"], "你好 OpenLess");
                        json!({"status":"submitted"})
                    }
                    _ => Value::Null,
                };
                reply(handle, &value, result);
            }
            Some("completed") => break,
            Some("failed") | Some("commandError") => panic!("{value}"),
            _ => {}
        }
    }
    assert_eq!(send(handle, json!({"operation":"history"})), 0);
    let history = next(context);
    assert_eq!(history["kind"], "history");
    assert_eq!(history["entries"][0]["finalText"], "你好 OpenLess");
    assert_eq!(history["entries"][0]["hasAudioRecording"], false);
    assert_eq!(std::fs::read_dir(directory.path().join("recordings")).unwrap().count(), 0);
    ol_ios_destroy(handle);
    assert_eq!(send(handle, json!({"operation":"history"})), -1);
    ol_ios_destroy(handle);
}
#[test]
fn invalid_ffi_and_oversize_payloads_fail_without_panicking() {
    assert_eq!(unsafe { ol_ios_command(999, std::ptr::null(), 0) }, -1);
    assert_eq!(
        unsafe { ol_ios_create(std::ptr::null(), 0, Some(event), Some(audio), 0) },
        0
    );
    let bytes = [b' '; 4];
    assert_eq!(
        unsafe { ol_ios_command(999, bytes.as_ptr(), MAX_JSON + 1) },
        -1
    );
}
#[test]
fn stale_client_cannot_cancel_another_session() {
    let (handle, context, _directory) = opened();
    let client = Uuid::new_v4();
    assert_eq!(
        send(
            handle,
            json!({"operation":"start","clientSessionID":client,"insert":false})
        ),
        0
    );
    let first = next(context);
    assert_eq!(first["kind"], "nativeRequest");
    assert_eq!(
        send(
            handle,
            json!({"operation":"cancel","clientSessionID":Uuid::new_v4()})
        ),
        -2
    );
    assert_eq!(next(context)["kind"], "commandError");
    assert_eq!(
        send(
            handle,
            json!({"operation":"cancel","clientSessionID":client})
        ),
        0
    );
    // The native startup must not be completed after its caller cancelled.
    let data = serde_json::to_vec(&json!({"requestID":first["requestID"],"ok":true,"value":null}))
        .unwrap();
    assert_eq!(
        unsafe { ol_ios_reply(handle, data.as_ptr(), data.len()) },
        1
    );
    ol_ios_destroy(handle);
}
