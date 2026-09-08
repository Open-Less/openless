#![cfg(target_os = "linux")]

use openless_core::{CredentialKey, CredentialNamespace, CredentialStore, SecretValue};
use openless_linux_egui::LinuxCredentialStore;

/// This contract is intentionally ignored by the normal test suite because it
/// requires a live Secret Service session.  Run it explicitly inside
/// `dbus-run-session` after starting gnome-keyring.
#[tokio::test]
#[ignore = "requires a live Linux Secret Service session"]
async fn secret_service_round_trip_preserves_secret_boundary() {
    assert_eq!(
        std::env::var("OPENLESS_RUN_SECRET_SERVICE_CONTRACT")
            .ok()
            .as_deref(),
        Some("1"),
        "set OPENLESS_RUN_SECRET_SERVICE_CONTRACT=1 when running this native contract"
    );

    let root = std::env::temp_dir().join(format!(
        "openless-linux-secret-service-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let store = LinuxCredentialStore::open(&root).expect("open Linux credential store");
    let key = CredentialKey::new(
        CredentialNamespace::Application,
        Some("secret-service-contract".to_string()),
        "round-trip",
    )
    .unwrap();
    let secret = "openless-secret-service-contract";

    store
        .write(key.clone(), SecretValue::new(secret))
        .await
        .expect("write credential through Secret Service");
    let read = store
        .read(key.clone())
        .await
        .expect("read credential through Secret Service")
        .expect("credential should exist after write");
    assert_eq!(read.expose_secret(), secret);

    let metadata = std::fs::read_to_string(root.join("credential-metadata.json"))
        .expect("read non-secret credential metadata");
    assert!(!metadata.contains(secret));
    assert!(!metadata.contains("SecretValue"));

    store
        .remove(key.clone())
        .await
        .expect("remove credential through Secret Service");
    assert!(store
        .read(key)
        .await
        .expect("read removed credential")
        .is_none());

    let _ = std::fs::remove_dir_all(root);
}
