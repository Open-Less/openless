use super::*;
use crate::control_tests::{control, open_at, result};
use crate::credentials::{IosCredentialStore, MemoryVault};
use crate::tests::{audio, event};
use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::symlink;

fn run(handle: u64, context: u64, action: Value) -> Value {
    result(control(handle, context, action))
}
fn archive(owner: &Host, name: &str) {
    fs::write(
        owner.style_staging_directory().unwrap().join(name),
        owner
            .backend
            .export_style_pack_bytes("builtin.light")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn style_library_normal_mode_is_read_only_detail_with_raw_default_and_no_editor() {
    let dir = tempfile::tempdir().unwrap();
    let (h, c) = open_at(dir.path(), false, Arc::new(MemoryVault::default()));
    let library = run(h, c, json!({"action":"styleLibrary"}));
    assert_eq!(library["activeStylePackId"], "builtin.raw");
    assert_eq!(library["archive"]["format"], "zip");
    assert_eq!(library["archive"]["maxCompressedBytes"], 524288);
    assert_eq!(library["editorSupported"], false);
    assert_eq!(library["updateSupported"], false);
    assert!(library["packs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|pack| pack.get("prompt").is_none()));
    let detail = run(h, c, json!({"action":"styleDetail","id":"builtin.light"}));
    assert!(!detail["pack"]["prompt"].as_str().unwrap().is_empty());
    assert_eq!(detail["runtime"]["packId"], "builtin.light");
    let denied = control(h, c, json!({"action":"activateStyle","id":"builtin.light"}));
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["code"], "PermissionDenied");
    assert_eq!(
        control(h, c, json!({"action":"removeStyle","id":"builtin.raw"}))["ok"],
        false
    );
    assert_eq!(
        control(
            h,
            c,
            json!({"action":"setStyleEnabled","id":"builtin.raw","enabled":false})
        )["code"],
        "PermissionDenied"
    );
    assert_eq!(control(h, c, json!({"action":"createStyle"}))["ok"], false);
    assert_eq!(
        control(
            h,
            c,
            json!({"action":"validate","kind":"llm","channelID":"none"})
        )["message"],
        "developer mode required"
    );
    assert_eq!(
        run(h, c, json!({"action":"capabilitySnapshot"}))["llm"]["enabled"],
        false
    );
    ol_ios_destroy(h);
}

#[test]
fn style_library_normal_reopen_reenables_raw_without_resetting_style_content() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(MemoryVault::default());
    let (h, c) = open_at(dir.path(), true, vault.clone());
    let before = host(h)
        .unwrap()
        .backend
        .get_style_pack("builtin.raw")
        .unwrap();
    run(
        h,
        c,
        json!({"action":"setStyleEnabled","id":"builtin.raw","enabled":false}),
    );
    run(h, c, json!({"action":"activateStyle","id":"builtin.light"}));
    ol_ios_destroy(h);
    let (h, c) = open_at(dir.path(), false, vault);
    let raw = host(h)
        .unwrap()
        .backend
        .get_style_pack("builtin.raw")
        .unwrap();
    assert!(raw.enabled);
    assert_eq!(raw.prompt, before.prompt);
    assert_eq!(raw.examples, before.examples);
    assert_eq!(raw.selection_prompt, before.selection_prompt);
    let snapshot = run(h, c, json!({"action":"capabilitySnapshot"}));
    assert_eq!(snapshot["selectedStyle"]["id"], "builtin.raw");
    assert_eq!(snapshot["asr"]["providerType"], "ios-sensevoice");
    assert_eq!(snapshot["llm"]["enabled"], false);
    assert_eq!(snapshot["llm"]["canAttemptPolish"], false);
    ol_ios_destroy(h);
}

#[test]
fn style_library_import_enable_activate_remove_and_reopen_use_core_store() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(MemoryVault::default());
    let (h, c) = open_at(dir.path(), true, vault.clone());
    archive(&host(h).unwrap(), "local.zip");
    let first = run(
        h,
        c,
        json!({"action":"importStyleArchive","stagedFileName":"local.zip"}),
    );
    let id = first["pack"]["id"].as_str().unwrap().to_string();
    let second = run(
        h,
        c,
        json!({"action":"importStyleArchive","stagedFileName":"local.zip"}),
    );
    assert_ne!(first["pack"]["id"], second["pack"]["id"]);
    assert_eq!(first["pack"]["kind"], "imported");
    run(
        h,
        c,
        json!({"action":"setStyleEnabled","id":id,"enabled":false}),
    );
    // Core activation re-enables the pack; the Host doesn't invent this rule.
    run(h, c, json!({"action":"activateStyle","id":id}));
    assert_eq!(
        run(h, c, json!({"action":"styleDetail","id":id}))["pack"]["enabled"],
        true
    );
    ol_ios_destroy(h);
    let (h, c) = open_at(dir.path(), true, vault);
    assert_eq!(
        run(h, c, json!({"action":"styleLibrary"}))["activeStylePackId"],
        id
    );
    run(h, c, json!({"action":"removeStyle","id":id}));
    let fallback = run(h, c, json!({"action":"styleLibrary"}))["activeStylePackId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(fallback, id);
    assert!(
        host(h)
            .unwrap()
            .backend
            .get_style_pack(&fallback)
            .unwrap()
            .enabled
    );
    assert_eq!(
        control(h, c, json!({"action":"styleDetail","id":id}))["ok"],
        false
    );
    ol_ios_destroy(h);
}

#[test]
fn style_library_staging_rejects_traversal_links_oversize_and_invalid_zip_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let (h, c) = open_at(dir.path(), false, Arc::new(MemoryVault::default()));
    let owner = host(h).unwrap();
    let staging = &owner.style_staging_directory().unwrap();
    archive(&owner, "valid.zip");
    let original = owner.style_library().unwrap()["packs"].clone();
    fs::write(dir.path().join("outside.zip"), b"do not read").unwrap();
    symlink(dir.path().join("outside.zip"), staging.join("link.zip")).unwrap();
    fs::hard_link(dir.path().join("outside.zip"), staging.join("hard.zip")).unwrap();
    fs::write(staging.join("invalid.zip"), b"not a ZIP").unwrap();
    let mut traversal = owner
        .backend
        .export_style_pack_bytes("builtin.light")
        .unwrap();
    let positions: Vec<_> = traversal
        .windows(b"manifest.json".len())
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == b"manifest.json").then_some(index))
        .collect();
    assert!(positions.len() >= 2); // ZIP local and physical central headers.
    for index in positions {
        traversal[index..index + 13].copy_from_slice(b"../escape.txt");
    }
    fs::write(staging.join("traversal.zip"), traversal).unwrap();
    File::create(staging.join("large.zip"))
        .unwrap()
        .set_len(524289)
        .unwrap();
    for name in [
        "../outside.zip",
        "/tmp/outside.zip",
        "file:///tmp/outside.zip",
        "link.zip",
        "hard.zip",
        "invalid.zip",
        "traversal.zip",
        "large.zip",
        "missing.zip",
        ".hidden.zip",
    ] {
        assert_eq!(
            control(
                h,
                c,
                json!({"action":"importStyleArchive","stagedFileName":name})
            )["ok"],
            false,
            "{name}"
        );
        assert_eq!(owner.style_library().unwrap()["packs"], original);
    }
    // Valid import is public but never opts normal mode into an LLM.
    run(
        h,
        c,
        json!({"action":"importStyleArchive","stagedFileName":"valid.zip"}),
    );
    assert_eq!(
        owner.backend.get_preferences().active_style_pack_id,
        "builtin.raw"
    );
    ol_ios_destroy(h);
}

#[test]
fn style_library_translation_uses_shared_catalog_legacy_target_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(MemoryVault::default());
    let (h, c) = open_at(dir.path(), false, vault.clone());
    let initial = run(h, c, json!({"action":"translationConfiguration"}));
    assert_eq!(initial["catalog"], catalog());
    assert_eq!(initial["targetLanguage"], "");
    assert!(initial["maxTargets"].as_u64().unwrap() > 3);
    assert_eq!(initial["multipleTargetsSupported"], true);
    assert_eq!(initial["targetOrderingSupported"], true);
    assert!(initial["targetLanguages"].is_array());
    let changed = run(
        h,
        c,
        json!({"action":"setTranslationTarget","targetLanguage":" English "}),
    );
    assert_eq!(changed["targetLanguage"], "English");
    assert_eq!(changed["targetLanguages"], json!(["English"]));
    assert_eq!(changed["workingLanguages"], initial["workingLanguages"]);
    let added = run(h, c, json!({"action":"addTranslationTarget","target":"ja"}));
    assert_eq!(added["targetLanguages"].as_array().unwrap().len(), 2);
    let mut reversed = added["targetLanguages"].as_array().unwrap().clone();
    reversed.reverse();
    let ordered = run(h, c, json!({"action":"reorderTranslationTargets","targets":reversed}));
    assert_eq!(ordered["targetLanguages"], json!(reversed));
    assert_eq!(ordered["targetLanguage"], "English");
    assert_eq!(control(h,c,json!({"action":"reorderTranslationTargets","targets":["English","English"]}))["ok"], false);
    let reduced = run(h, c, json!({"action":"deleteTranslationTarget","target":"ja"}));
    assert_eq!(reduced["targetLanguages"], json!(["English"]));
    assert_eq!(
        control(
            h,
            c,
            json!({"action":"setTranslationTarget","targetLanguage":"en-US"})
        )["ok"],
        false
    );
    ol_ios_destroy(h);
    let (h, c) = open_at(dir.path(), false, vault);
    assert_eq!(
        run(h, c, json!({"action":"translationConfiguration"}))["targetLanguage"],
        "English"
    );
    assert_eq!(
        run(h, c, json!({"action":"capabilitySnapshot"}))["llm"]["canAttemptTranslation"],
        false
    );
    assert_eq!(
        run(
            h,
            c,
            json!({"action":"setTranslationTarget","targetLanguage":""})
        )["effective"],
        false
    );
    ol_ios_destroy(h);
}

#[test]
fn style_library_credentials_never_imply_enabled_verified_or_entitled_and_no_secrets_escape() {
    let dir = tempfile::tempdir().unwrap();
    let (h, c) = open_at(dir.path(), true, Arc::new(MemoryVault::default()));
    run(h, c, json!({"action":"activateStyle","id":"builtin.light"}));
    let missing = run(h, c, json!({"action":"capabilitySnapshot"}));
    assert_eq!(missing["llm"]["availability"], "missingConfiguration");
    assert_eq!(missing["llm"]["canAttemptPolish"], false);
    run(h, c, json!({"action":"activateStyle","id":"builtin.raw"}));
    let saved = run(
        h,
        c,
        json!({"action":"saveChannel","kind":"llm","providerType":"custom","name":"Private","endpoint":"https://invalid.example/v1?secret=private-endpoint","model":"fixture-model","apiKey":"never-return-secret"}),
    );
    run(
        h,
        c,
        json!({"action":"setActive","kind":"llm","channelID":saved["channelID"]}),
    );
    let snapshot = run(h, c, json!({"action":"capabilitySnapshot"}));
    assert_eq!(snapshot["llm"]["enabled"], false);
    assert_eq!(snapshot["llm"]["configured"], true);
    assert_eq!(snapshot["llm"]["canAttemptPolish"], false);
    run(h, c, json!({"action":"activateStyle","id":"builtin.light"}));
    let snapshot = run(h, c, json!({"action":"capabilitySnapshot"}));
    assert_eq!(snapshot["llm"]["enabled"], true);
    assert_eq!(snapshot["llm"]["canAttemptPolish"], true);
    assert_eq!(snapshot["llm"]["verification"], "notRecorded");
    assert_eq!(snapshot["llm"]["entitlement"], "unknown");
    assert_eq!(snapshot["llm"]["modelAvailability"], "unknown");
    assert_eq!(snapshot["llm"]["canAttemptHelpWrite"], true);
    assert_eq!(snapshot["llm"]["model"], "fixture-model");
    assert!(!snapshot.to_string().contains("never-return-secret"));
    assert!(!snapshot.to_string().contains("private-endpoint"));
    assert!(!snapshot.to_string().contains("invalid.example"));
    ol_ios_destroy(h);
}

#[test]
fn style_library_active_session_gate_allows_reads_but_rejects_mutations_and_network() {
    let dir = tempfile::tempdir().unwrap();
    let (h, c) = open_at(dir.path(), false, Arc::new(MemoryVault::default()));
    let owner = host(h).unwrap();
    *owner.admission.lock().unwrap() = Some(Admission {
        client: Uuid::new_v4(),
        finishing: false,
        cancelled: false,
        execution: Arc::new(voice_intent::Execution::default()),
    });
    for action in [
        json!({"action":"styleLibrary"}),
        json!({"action":"styleDetail","id":"builtin.raw"}),
        json!({"action":"capabilitySnapshot"}),
        json!({"action":"translationConfiguration"}),
    ] {
        run(h, c, action);
    }
    for action in [
        json!({"action":"activateStyle","id":"builtin.raw"}),
        json!({"action":"setStyleEnabled","id":"builtin.light","enabled":false}),
        json!({"action":"removeStyle","id":"unused"}),
        json!({"action":"importStyleArchive","stagedFileName":"unused.zip"}),
        json!({"action":"setTranslationTarget","targetLanguage":"English"}),
        json!({"action":"marketplaceList"}),
        json!({"action":"marketplaceDetail","id":"unused"}),
        json!({"action":"marketplaceInstall","id":"unused"}),
    ] {
        assert_eq!(
            control(h, c, action)["message"],
            "recording or processing is active"
        );
    }
    *owner.admission.lock().unwrap() = None;
    ol_ios_destroy(h);
}

// A bounded loopback HTTP fixture exercises the real Core marketplace client;
// no external service, OAuth, permissions or microphone is used.
fn server(
    responses: Vec<(&'static str, Vec<u8>)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let thread = std::thread::spawn(move || {
        let mut requests = vec![];
        for (status, bytes) in responses {
            let start = std::time::Instant::now();
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(10),
                    "fixture request timed out"
                );
                std::thread::sleep(Duration::from_millis(1));
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                assert_eq!(stream.read(&mut byte).unwrap(), 1);
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            requests.push(String::from_utf8(request).unwrap());
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            stream.write_all(&bytes).unwrap();
        }
        requests
    });
    (url, thread)
}
fn marketplace_host(dir: &Path, url: &str) -> (u64, u64) {
    static CONTEXT: AtomicU64 = AtomicU64::new(900_000);
    let context = CONTEXT.fetch_add(1, Ordering::Relaxed);
    let h = create_with_marketplace(
        Config {
            data_dir: dir.into(),
            developer_mode: false,
        },
        Callbacks {
            event,
            audio,
            context,
        },
        Arc::new(IosCredentialStore::open(Arc::new(MemoryVault::default())).unwrap()),
        openless_core::MarketplaceConfig::new(url).unwrap(),
    )
    .unwrap();
    (h, context)
}

#[test]
fn style_library_marketplace_uses_real_anonymous_core_list_detail_download_and_store() {
    let source = openless_core::StylePackStore::in_memory();
    let bytes = source.export_zip_bytes("builtin.light").unwrap();
    let summary = json!({"id":"01234567-89ab-cdef-0123-456789abcdef","slug":"01234567-89ab-cdef-0123-456789abcdef","name":"Fixture","description":"fixture","authorLogin":"fixture-author","version":"1.0","baseMode":"light","tags":[],"likeCount":0,"downloadCount":0,"publishedAt":"","updatedAt":""});
    let mut detail = summary.clone();
    detail["prompt"] = json!("prompt");
    detail["state"] = json!("published");
    let (url, server) = server(vec![
        ("200 OK", serde_json::to_vec(&vec![summary]).unwrap()),
        ("200 OK", serde_json::to_vec(&detail).unwrap()),
        ("200 OK", serde_json::to_vec(&detail).unwrap()),
        ("200 OK", bytes),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let (h, c) = marketplace_host(dir.path(), &url);
    let list = run(
        h,
        c,
        json!({"action":"marketplaceList","query":"hello world","sort":"popular","limit":2}),
    );
    assert_eq!(
        list["items"][0]["id"],
        "01234567-89ab-cdef-0123-456789abcdef"
    );
    assert_eq!(
        run(
            h,
            c,
            json!({"action":"marketplaceDetail","id":"01234567-89ab-cdef-0123-456789abcdef"})
        )["detail"]["state"],
        "published"
    );
    let installed = run(
        h,
        c,
        json!({"action":"marketplaceInstall","id":"01234567-89ab-cdef-0123-456789abcdef"}),
    );
    assert_eq!(
        installed["pack"]["originPackId"],
        "01234567-89ab-cdef-0123-456789abcdef"
    );
    assert_eq!(installed["pack"]["originAuthorLogin"], "fixture-author");
    let id = installed["pack"]["id"].as_str().unwrap();
    assert_eq!(
        run(h, c, json!({"action":"styleDetail","id":id}))["pack"]["id"],
        id
    );
    assert_eq!(
        run(h, c, json!({"action":"styleLibrary"}))["activeStylePackId"],
        "builtin.raw"
    );
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /packs?q=hello+world&sort=popular&limit=2 HTTP/1.1"));
    assert!(requests[1].starts_with("GET /packs/01234567-89ab-cdef-0123-456789abcdef HTTP/1.1"));
    assert!(requests[3]
        .starts_with("GET /packs/01234567-89ab-cdef-0123-456789abcdef/download HTTP/1.1"));
    assert!(requests
        .iter()
        .all(|request| !request.to_lowercase().contains("authorization:")));
    ol_ios_destroy(h);
}

#[test]
fn style_library_marketplace_error_is_coded_and_does_not_reflect_server_secrets() {
    let (url, server) = server(vec![(
        "401 Unauthorized",
        b"private-server-secret".to_vec(),
    )]);
    let dir = tempfile::tempdir().unwrap();
    let (h, c) = marketplace_host(dir.path(), &url);
    let failure = control(h, c, json!({"action":"marketplaceList"}));
    assert_eq!(failure["ok"], false);
    assert_eq!(failure["code"], "Provider");
    assert!(!failure.to_string().contains("private-server-secret"));
    server.join().unwrap();
    assert_eq!(
        control(
            h,
            c,
            json!({"action":"marketplaceInstall","id":"../unsafe"})
        )["code"],
        "InvalidArgument"
    );
    ol_ios_destroy(h);
}
