use base64::Engine;
use openless_core::{builtin_style_pack_for_mode, PolishMode, StylePackStore};
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("openless-style-icon-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> StylePackStore {
        StylePackStore::at_data_dir(&self.0).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO3c3b0AAAAASUVORK5CYII=").unwrap()
}

#[test]
fn uploaded_icon_survives_restart_export_and_clear() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let saved = store.update_icon("builtin.light", Some(&png())).unwrap();
    let path = PathBuf::from(saved.icon_path.unwrap());
    assert!(path.is_file());
    let expected = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png())
    );
    let reopened = fixture.store();
    assert_eq!(
        reopened.icon_data_url("builtin.light").unwrap(),
        Some(expected.clone())
    );
    let exported = reopened.export_zip_bytes("builtin.light").unwrap();
    let imported = reopened.import_from_zip_bytes(&exported).unwrap();
    assert_eq!(
        reopened.icon_data_url(&imported.id).unwrap(),
        Some(expected)
    );
    reopened.update_icon("builtin.light", None).unwrap();
    assert!(!path.exists());
    assert_eq!(
        fixture.store().icon_data_url("builtin.light").unwrap(),
        None
    );
    assert!(reopened.icon_data_url(&imported.id).unwrap().is_some());
}

#[test]
fn invalid_icon_preserves_the_previous_image() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let saved = store.update_icon("builtin.light", Some(&png())).unwrap();
    for invalid in [b"<svg onload='alert(1)'/>".to_vec(), vec![0; 65_537]] {
        assert!(store.update_icon("builtin.light", Some(&invalid)).is_err());
        assert_eq!(
            store.get("builtin.light").unwrap().icon_path,
            saved.icon_path
        );
        assert!(PathBuf::from(saved.icon_path.as_ref().unwrap()).is_file());
    }
}

#[test]
fn icon_reader_cannot_read_paths_outside_the_asset_directory() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let outside = fixture.0.join("outside.png");
    std::fs::write(&outside, png()).unwrap();
    let mut template = builtin_style_pack_for_mode(PolishMode::Light);
    template.icon_path = Some(outside.to_string_lossy().into_owned());
    let created = store.create(template).unwrap();
    assert!(store.icon_data_url(&created.id).is_err());
    store.update_icon(&created.id, None).unwrap();
    assert!(outside.is_file());
}

#[test]
fn failed_metadata_write_preserves_old_image_and_memory() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let saved = store.update_icon("builtin.light", Some(&png())).unwrap();
    let path = fixture.0.join("style-packs.json");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store.update_icon("builtin.light", Some(&png())).is_err());
    assert_eq!(
        store.get("builtin.light").unwrap().icon_path,
        saved.icon_path
    );
    assert!(PathBuf::from(saved.icon_path.unwrap()).is_file());
}
