use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use std::sync::Mutex;
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use super::{migrate_style_packs_from_preferences, sync_style_pack_preferences, StylePackStore};
use crate::shared_types::UserPreferences;
use crate::style_pack_archive::{
    MAX_ENTRY_UNCOMPRESSED_BYTES, MAX_EXAMPLES_BYTES, MAX_ICON_BYTES, MAX_MANIFEST_BYTES,
    MAX_PROMPT_BYTES, MAX_TOTAL_UNCOMPRESSED_BYTES, STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES,
};
use crate::style_packs::{
    builtin_style_packs, CustomStylePrompts, StylePack, StylePackExample, StyleSystemPrompts,
};
use crate::types::PolishMode;

const VALID_PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "openless-style-pack-{label}-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&path).expect("create test dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_store(root: &Path, packs: Vec<StylePack>) -> StylePackStore {
    let asset_root = root.join("assets");
    fs::create_dir_all(&asset_root).expect("create asset root");
    StylePackStore {
        path: Some(root.join("style-packs.json")),
        asset_root: Some(asset_root),
        state: Mutex::new(packs),
    }
}

fn manifest(prompt_file: &str, examples_file: &str, icon_file: Option<&str>) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 1,
        "id": "test-pack",
        "name": "Test Pack",
        "description": "security fixture",
        "author": "OpenLess",
        "version": "1.0.0",
        "baseMode": "light",
        "tags": ["test"],
        "promptFile": prompt_file,
        "examplesFile": examples_file,
        "iconFile": icon_file,
    }))
    .expect("encode manifest")
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, contents) in entries {
        writer.start_file(*name, options).expect("start zip entry");
        writer.write_all(contents).expect("write zip entry");
    }
    writer.finish().expect("finish zip");
}

fn duplicate_central_directory_entry(path: &Path, entry_name: &str, extra_copies: usize) {
    const CENTRAL_HEADER: &[u8; 4] = b"PK\x01\x02";
    const EOCD: &[u8; 4] = b"PK\x05\x06";
    const CENTRAL_HEADER_LEN: usize = 46;

    let mut bytes = fs::read(path).expect("read zip fixture");
    let eocd_offset = bytes
        .windows(EOCD.len())
        .rposition(|window| window == EOCD)
        .expect("find zip EOCD");
    let original_count = u16::from_le_bytes(
        bytes[eocd_offset + 10..eocd_offset + 12]
            .try_into()
            .expect("EOCD entry count"),
    );
    let original_central_size = u32::from_le_bytes(
        bytes[eocd_offset + 12..eocd_offset + 16]
            .try_into()
            .expect("EOCD central size"),
    );
    let mut offset = u32::from_le_bytes(
        bytes[eocd_offset + 16..eocd_offset + 20]
            .try_into()
            .expect("EOCD central offset"),
    ) as usize;

    let mut selected_record = None;
    for _ in 0..original_count {
        assert_eq!(
            bytes.get(offset..offset + CENTRAL_HEADER.len()),
            Some(CENTRAL_HEADER.as_slice()),
            "central directory signature"
        );
        let name_len = u16::from_le_bytes(
            bytes[offset + 28..offset + 30]
                .try_into()
                .expect("central name length"),
        ) as usize;
        let extra_len = u16::from_le_bytes(
            bytes[offset + 30..offset + 32]
                .try_into()
                .expect("central extra length"),
        ) as usize;
        let comment_len = u16::from_le_bytes(
            bytes[offset + 32..offset + 34]
                .try_into()
                .expect("central comment length"),
        ) as usize;
        let end = offset + CENTRAL_HEADER_LEN + name_len + extra_len + comment_len;
        if &bytes[offset + CENTRAL_HEADER_LEN..offset + CENTRAL_HEADER_LEN + name_len]
            == entry_name.as_bytes()
        {
            selected_record = Some(bytes[offset..end].to_vec());
        }
        offset = end;
    }

    let record = selected_record.expect("selected central directory entry");
    let added = record.repeat(extra_copies);
    bytes.splice(eocd_offset..eocd_offset, added.iter().copied());
    let new_eocd_offset = eocd_offset + added.len();
    let new_count = original_count
        .checked_add(u16::try_from(extra_copies).expect("fixture copy count fits u16"))
        .expect("fixture entry count fits u16");
    bytes[new_eocd_offset + 8..new_eocd_offset + 10].copy_from_slice(&new_count.to_le_bytes());
    bytes[new_eocd_offset + 10..new_eocd_offset + 12].copy_from_slice(&new_count.to_le_bytes());
    let new_central_size = original_central_size
        .checked_add(u32::try_from(added.len()).expect("fixture size fits u32"))
        .expect("fixture central size fits u32");
    bytes[new_eocd_offset + 12..new_eocd_offset + 16]
        .copy_from_slice(&new_central_size.to_le_bytes());
    fs::write(path, bytes).expect("write duplicate central directory fixture");
}

fn valid_archive(path: &Path, icon: Option<&[u8]>) {
    let manifest = manifest(
        "prompt.md",
        "examples.json",
        icon.map(|_| "assets/icon.png"),
    );
    let examples = serde_json::to_vec(&vec![StylePackExample {
        title: Some("Greeting".into()),
        input: "hello".into(),
        output: "Hello!".into(),
    }])
    .expect("encode examples");
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("manifest.json", &manifest),
        ("prompt.md", b"Write clearly and concisely."),
        ("examples.json", &examples),
    ];
    if let Some(icon) = icon {
        entries.push(("assets/icon.png", icon));
    }
    write_zip(path, &entries);
}

fn assert_import_error_contains(store: &StylePackStore, zip_path: &Path, expected: &str) {
    let error = store
        .import_from_zip(zip_path)
        .expect_err("archive must be rejected");
    let message = format!("{error:#}");
    assert!(
        message.contains(expected),
        "expected error containing {expected:?}, got {message:?}"
    );
    assert!(
        store.state.lock().unwrap().is_empty(),
        "rejected import changed state"
    );
}

#[test]
fn import_rejects_duplicate_physical_manifest_entry() {
    let root = TestDir::new("duplicate-manifest");
    let zip_path = root.path().join("duplicate-manifest.zip");
    valid_archive(&zip_path, None);
    duplicate_central_directory_entry(&zip_path, "manifest.json", 1);
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "duplicate physical");
}

#[test]
fn import_rejects_duplicate_physical_manifest_selected_prompt_entry() {
    let root = TestDir::new("duplicate-prompt");
    let zip_path = root.path().join("duplicate-prompt.zip");
    valid_archive(&zip_path, None);
    duplicate_central_directory_entry(&zip_path, "prompt.md", 1);
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "duplicate physical");
}

#[test]
fn import_rejects_duplicate_physical_manifest_selected_icon_entry() {
    let root = TestDir::new("duplicate-icon");
    let zip_path = root.path().join("duplicate-icon.zip");
    valid_archive(&zip_path, Some(VALID_PNG_1X1));
    duplicate_central_directory_entry(&zip_path, "assets/icon.png", 1);
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "duplicate physical");
}

#[test]
fn import_rejects_more_than_sixteen_physical_entries_even_when_names_repeat() {
    let root = TestDir::new("physical-entry-count");
    let zip_path = root.path().join("physical-entry-count.zip");
    valid_archive(&zip_path, None);
    duplicate_central_directory_entry(&zip_path, "prompt.md", 14);
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "physical entries");
}

#[test]
fn import_rejects_compressed_archive_over_limit_before_zip_parsing() {
    let root = TestDir::new("compressed-limit");
    let zip_path = root.path().join("oversized.zip");
    fs::write(
        &zip_path,
        vec![0u8; STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES + 1],
    )
    .expect("write oversized archive");
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "compressed size");
}

#[test]
fn import_rejects_oversized_manifest_independently() {
    let root = TestDir::new("manifest-limit");
    let zip_path = root.path().join("manifest.zip");
    let oversized_manifest = vec![b' '; MAX_MANIFEST_BYTES + 1];
    write_zip(&zip_path, &[("manifest.json", &oversized_manifest)]);
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "manifest.json declared size");
}

#[test]
fn import_rejects_bomb_shaped_prompt_before_unbounded_allocation() {
    let root = TestDir::new("prompt-bomb");
    let zip_path = root.path().join("bomb.zip");
    let manifest = manifest("prompt.md", "examples.json", None);
    let prompt = vec![b'a'; MAX_PROMPT_BYTES + 1];
    write_zip(
        &zip_path,
        &[
            ("manifest.json", &manifest),
            ("prompt.md", &prompt),
            ("examples.json", b"[]"),
        ],
    );
    assert!(
        fs::metadata(&zip_path).expect("zip metadata").len()
            < STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES as u64,
        "fixture must be highly compressed"
    );
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "prompt.md declared size");
}

#[test]
fn import_rejects_oversized_examples_independently() {
    let root = TestDir::new("examples-limit");
    let zip_path = root.path().join("examples.zip");
    let manifest = manifest("prompt.md", "examples.json", None);
    let examples = vec![b' '; MAX_EXAMPLES_BYTES + 1];
    write_zip(
        &zip_path,
        &[
            ("manifest.json", &manifest),
            ("prompt.md", b"prompt"),
            ("examples.json", &examples),
        ],
    );
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "examples.json declared size");
}

#[test]
fn import_rejects_oversized_icon_without_leaving_assets() {
    let root = TestDir::new("icon-limit");
    let zip_path = root.path().join("icon.zip");
    let manifest = manifest("prompt.md", "examples.json", Some("assets/icon.png"));
    let mut icon = VALID_PNG_1X1.to_vec();
    icon.resize(MAX_ICON_BYTES + 1, 0);
    write_zip(
        &zip_path,
        &[
            ("manifest.json", &manifest),
            ("prompt.md", b"prompt"),
            ("examples.json", b"[]"),
            ("assets/icon.png", &icon),
        ],
    );
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "assets/icon.png declared size");
    assert!(
        !root.path().join("assets/test-pack").exists(),
        "rejected icon left a partial asset directory"
    );
}

#[test]
fn import_rejects_single_entry_over_generic_limit() {
    let root = TestDir::new("entry-limit");
    let zip_path = root.path().join("entry.zip");
    let manifest = manifest("prompt.md", "examples.json", None);
    let oversized_extra = vec![b'x'; MAX_ENTRY_UNCOMPRESSED_BYTES + 1];
    write_zip(
        &zip_path,
        &[
            ("manifest.json", &manifest),
            ("prompt.md", b"prompt"),
            ("examples.json", b"[]"),
            ("extra.txt", &oversized_extra),
        ],
    );
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "extra.txt declared size");
}

#[test]
fn import_rejects_total_declared_uncompressed_size_over_limit() {
    let root = TestDir::new("total-limit");
    let zip_path = root.path().join("total.zip");
    let manifest = manifest("prompt.md", "examples.json", None);
    let large = vec![b'x'; 60 * 1024];
    write_zip(
        &zip_path,
        &[
            ("manifest.json", &manifest),
            ("prompt.md", &large),
            ("examples.json", &large),
            ("extra-1.txt", &large),
            ("extra-2.txt", &large),
            ("extra-3.txt", &large),
        ],
    );
    assert!(
        5 * large.len() > MAX_TOTAL_UNCOMPRESSED_BYTES,
        "fixture must cross total uncompressed limit"
    );
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "total declared uncompressed size");
}

#[test]
fn import_rejects_unsafe_manifest_selected_path() {
    let root = TestDir::new("unsafe-path");
    let zip_path = root.path().join("unsafe.zip");
    let manifest = manifest("../prompt.md", "examples.json", None);
    write_zip(
        &zip_path,
        &[
            ("manifest.json", &manifest),
            ("../prompt.md", b"escaped prompt"),
            ("examples.json", b"[]"),
        ],
    );
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "unsafe");
}

#[test]
fn import_rejects_malformed_backslash_entry_name() {
    let root = TestDir::new("backslash-name");
    let zip_path = root.path().join("backslash.zip");
    let manifest = manifest("folder\\prompt.md", "examples.json", None);
    write_zip(
        &zip_path,
        &[
            ("manifest.json", &manifest),
            ("folder\\prompt.md", b"prompt"),
            ("examples.json", b"[]"),
        ],
    );
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "unsafe");
}

#[test]
fn import_rejects_icon_extension_content_mismatch() {
    let root = TestDir::new("icon-mismatch");
    let zip_path = root.path().join("mismatch.zip");
    let manifest = manifest("prompt.md", "examples.json", Some("assets/icon.png"));
    write_zip(
        &zip_path,
        &[
            ("manifest.json", &manifest),
            ("prompt.md", b"prompt"),
            ("examples.json", b"[]"),
            ("assets/icon.png", b"not a png"),
        ],
    );
    let store = test_store(root.path(), Vec::new());

    assert_import_error_contains(&store, &zip_path, "PNG");
    assert!(!root.path().join("assets/test-pack").exists());
}

#[test]
fn import_rolls_back_icon_and_state_when_store_persistence_fails() {
    let root = TestDir::new("rollback");
    let zip_path = root.path().join("valid.zip");
    valid_archive(&zip_path, Some(VALID_PNG_1X1));
    let mut store = test_store(root.path(), Vec::new());
    let invalid_store_path = root.path().join("store-target-is-a-directory");
    fs::create_dir_all(&invalid_store_path).expect("create invalid store target");
    store.path = Some(invalid_store_path);

    let error = store
        .import_from_zip(&zip_path)
        .expect_err("persistence failure must abort import");
    assert_eq!(error.code, crate::BackendErrorCode::Persistence);
    assert!(
        store.state.lock().unwrap().is_empty(),
        "failed import changed state"
    );
    assert!(
        !root.path().join("assets/test-pack").exists(),
        "failed import left a partial asset directory"
    );
}

#[test]
fn style_pack_archive_round_trip_preserves_valid_pack_and_png_icon() {
    let root = TestDir::new("roundtrip");
    let icon_path = root.path().join("source-icon.png");
    fs::write(&icon_path, VALID_PNG_1X1).expect("write source icon");
    let mut pack = builtin_style_packs()
        .into_iter()
        .find(|pack| pack.id == "builtin.light")
        .expect("builtin pack");
    pack.id = "roundtrip-pack".into();
    pack.name = "Roundtrip Pack".into();
    pack.prompt = "Roundtrip prompt".into();
    pack.examples = vec![StylePackExample {
        title: Some("Example".into()),
        input: "input".into(),
        output: "output".into(),
    }];
    pack.icon_path = Some(icon_path.to_string_lossy().into_owned());
    let source = test_store(&root.path().join("source"), vec![pack.clone()]);
    let zip_path = root.path().join("roundtrip.zip");
    fs::write(&zip_path, b"stale pre-created archive contents").expect("pre-create zip target");
    source
        .export_to_zip(&pack.id, &zip_path)
        .expect("export valid pack");

    let destination = test_store(&root.path().join("destination"), Vec::new());
    let imported = destination
        .import_from_zip(&zip_path)
        .expect("import valid pack");

    assert_eq!(imported.id, pack.id);
    assert_eq!(imported.name, pack.name);
    assert_eq!(imported.prompt, pack.prompt);
    assert_eq!(imported.examples, pack.examples);
    let imported_icon = imported.icon_path.expect("imported icon path");
    assert_eq!(
        fs::read(imported_icon).expect("read imported icon"),
        VALID_PNG_1X1
    );
}

#[test]
fn style_pack_archive_bytes_can_be_imported_from_a_document_provider() {
    let root = TestDir::new("bytes-import");
    let zip_path = root.path().join("document-provider.zip");
    valid_archive(&zip_path, None);
    let bytes = fs::read(&zip_path).expect("read archive bytes");
    let destination = test_store(&root.path().join("destination"), Vec::new());

    let imported = destination
        .import_from_zip_bytes(&bytes)
        .expect("import valid archive bytes");

    assert_eq!(imported.id, "test-pack");
    assert_eq!(imported.name, "Test Pack");
    assert_eq!(imported.prompt, "Write clearly and concisely.");
    assert_eq!(imported.examples.len(), 1);
}

#[test]
fn migration_fills_empty_selection_prompts_with_style_defaults() {
    let mut packs = builtin_style_packs();
    for pack in &mut packs {
        pack.selection_prompt.clear();
    }

    assert!(migrate_style_packs_from_preferences(
        &mut packs,
        &UserPreferences::default()
    ));
    let prompts: Vec<_> = packs
        .iter()
        .map(|pack| pack.selection_prompt.as_str())
        .collect();
    assert_eq!(prompts.len(), 4);
    assert_eq!(
        prompts
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );

    let prompt_for = |mode| {
        packs
            .iter()
            .find(|pack| pack.base_mode == mode)
            .expect("built-in pack")
            .selection_prompt
            .as_str()
    };
    assert!(prompt_for(PolishMode::Light).contains("轻度文本润色助手"));
    assert!(prompt_for(PolishMode::Structured).contains("AI Prompt 整理助手"));
    assert!(prompt_for(PolishMode::Formal).contains("职场与专业沟通文本编辑助手"));
}

#[test]
fn sync_style_pack_preferences_uses_builtin_store_prompts_as_source_of_truth() {
    let mut prefs = UserPreferences {
        style_system_prompts: StyleSystemPrompts {
            raw: "stale raw".into(),
            light: "stale light".into(),
            structured: "stale structured".into(),
            formal: "stale formal".into(),
        },
        custom_style_prompts: CustomStylePrompts {
            raw: String::new(),
            light: "legacy extra instruction".into(),
            structured: String::new(),
            formal: String::new(),
        },
        ..Default::default()
    };
    let mut packs = builtin_style_packs();
    let light = packs
        .iter_mut()
        .find(|pack| pack.id == "builtin.light")
        .expect("builtin light pack");
    light.prompt = "fresh light prompt from store".into();

    assert!(sync_style_pack_preferences(&mut prefs, &packs));
    assert_eq!(prefs.style_system_prompts.raw, packs[0].prompt);
    assert_eq!(
        prefs.style_system_prompts.light,
        "fresh light prompt from store"
    );
    assert_eq!(prefs.style_system_prompts.structured, packs[2].prompt);
    assert_eq!(prefs.style_system_prompts.formal, packs[3].prompt);
    assert_eq!(prefs.custom_style_prompts, CustomStylePrompts::default());
}

#[test]
fn pack_version_newer_compares_numeric_segments() {
    assert!(super::version_newer("3.0.0", "2.0.0"));
    assert!(!super::version_newer("2.0.0", "3.0.0"));
    assert!(!super::version_newer("3.0.0", "3.0.0"));
    assert!(super::version_newer("3.1.0", "3.0.9"));
    assert!(super::version_newer("10.0.0", "9.9.9"));
    // pre-release 视为与正式版同级，不判为更新
    assert!(!super::version_newer("3.0.0-beta.1", "3.0.0"));
    assert!(!super::version_newer("3.0.0", "3.0.0-beta.1"));
    assert!(super::version_newer("3.0.1-beta", "3.0.0"));
    // 全非数字 → 不判定更新
    assert!(!super::version_newer("abc", "def"));
}

#[test]
fn reconcile_builtin_packs_upgrades_prompt_only_and_preserves_user_fields() {
    let mut packs = builtin_style_packs();
    let local = packs
        .iter_mut()
        .find(|p| p.id == "builtin.structured")
        .expect("builtin structured pack");
    local.version = "2.0.0".into();
    local.prompt = "用户自定义的旧 prompt".into();
    local.name = "我的清晰结构".into();
    local.enabled = false; // 用户手动禁用

    assert!(super::reconcile_builtin_packs(&mut packs));

    let upgraded = packs
        .iter()
        .find(|p| p.id == "builtin.structured")
        .expect("builtin structured pack");
    assert_eq!(upgraded.version, "3.0.0", "版本应推进到官方 3.0.0");
    assert!(
        upgraded.prompt.contains("# 场景优先级"),
        "prompt 应推进为 v3.0 Beta"
    );
    assert_eq!(upgraded.name, "我的清晰结构", "用户改名必须保留");
    assert!(!upgraded.enabled, "用户 enabled 状态必须保留");
}

#[test]
fn reconcile_builtin_packs_skips_equal_version_and_adds_missing() {
    // 等版本（builtin 3.0.0 vs local 3.0.0）→ 不推进、不落盘
    let mut packs = builtin_style_packs();
    assert!(!super::reconcile_builtin_packs(&mut packs));

    // 本地缺失内置包 → 补入全部 4 个
    let mut empty: Vec<StylePack> = Vec::new();
    assert!(super::reconcile_builtin_packs(&mut empty));
    assert_eq!(empty.len(), 4);
    assert!(empty
        .iter()
        .all(|p| p.kind == crate::style_packs::StylePackKind::Builtin));
}
