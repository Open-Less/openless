// Built against the exact source revision returned by production /revision.
mod common;
use common::*;
use openless_cloud_sync::{
    auth::{Account, IdentityVerifier, VerifyFuture},
    config::Config,
    server::{router, AppState},
    store::Store,
};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::Arc};

struct FixtureGithub;
impl IdentityVerifier for FixtureGithub {
    fn verify<'a>(&'a self, _: &'a str) -> VerifyFuture<'a> {
        Box::pin(async {
            Ok(Account {
                github_id: "11".into(),
                login: "fixture".into(),
            })
        })
    }
}

fn persistent_fixture(dir: &std::path::Path) -> Fixture {
    let database = dir.join("service.db");
    let store = Store::open(&database).unwrap();
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database,
        github_client_id: "test-app".into(),
        github_client_secret: "fixture-secret-not-a-real-credential".to_string().into(),
        allowed_origins: HashSet::new(),
        allowed_github_ids: HashSet::from(["11".into()]),
        public_access: false,
        require_https_proxy: false,
    };
    Fixture {
        app: router(Arc::new(AppState::new(
            config,
            store.clone(),
            Arc::new(FixtureGithub),
        ))),
        store,
        dir: tempfile::tempdir().unwrap(),
    }
}

#[tokio::test]
async fn v1_release_server_exchange() {
    let dir = std::path::PathBuf::from(std::env::var("OPENLESS_V1_COMPAT_DIR").unwrap());
    // Separate process phases reopen the same SQLite database, not a fabricated
    // metadata head or a replay into an empty replacement service.
    let fixture = persistent_fixture(&dir);
    let token = fixture.login("github-alice").await;
    let initial = fixture.meta(&token).await;
    let first: Value =
        serde_json::from_slice(&std::fs::read(dir.join("new-upload.json")).unwrap()).unwrap();
    if initial.json["state"] == "empty" {
        let created = fixture.put(&token, initial.tag(), &first).await;
        assert_eq!(created.status, 200);
    } else {
        assert_eq!(initial.json["revision"], "1");
    }
    let mut meta = fixture.meta(&token).await;
    let old_path = dir.join("old-upload.json");
    if old_path.exists() {
        let old: Value = serde_json::from_slice(&std::fs::read(old_path).unwrap()).unwrap();
        let saved = fixture.put(&token, meta.tag(), &old).await;
        assert_eq!(saved.status, 200);
        // Replay and stale CAS cannot replace the winning full snapshot.
        assert_eq!(fixture.put(&token, meta.tag(), &old).await.status, 200);
        let mut stale = old.clone();
        stale["operationId"] = json!(uuid::Uuid::new_v4().to_string());
        let rejected = fixture.put(&token, meta.tag(), &stale).await;
        assert_eq!(rejected.status, 412);
        meta = fixture.meta(&token).await;
    }
    let downloaded = fixture
        .request(
            "GET",
            "/v1/me/vault/snapshot",
            Some(&token),
            &[("if-match", meta.tag())],
            vec![],
        )
        .await;
    assert_eq!(downloaded.status, 200);
    let expected = if dir.join("old-upload.json").exists() {
        std::fs::read(dir.join("old-upload.json")).unwrap()
    } else {
        std::fs::read(dir.join("new-upload.json")).unwrap()
    };
    assert_eq!(
        downloaded.json,
        serde_json::from_slice::<Value>(&expected).unwrap()
    );
    std::fs::write(dir.join("download.json"), downloaded.body).unwrap();
    if !dir.join("old-upload.json").exists() {
        return;
    }
    let deletion = json!({"protocolVersion":1,"baseRevision":meta.json["revision"],"operationId":uuid::Uuid::new_v4().to_string(),"expectedVaultId":first["vaultId"]});
    let deleted = fixture
        .request(
            "DELETE",
            "/v1/me/vault",
            Some(&token),
            &[
                ("if-match", meta.tag()),
                ("content-type", "application/json"),
                ("idempotency-key", deletion["operationId"].as_str().unwrap()),
            ],
            serde_json::to_vec(&deletion).unwrap(),
        )
        .await;
    assert_eq!(deleted.status, 200);
    let tombstone = fixture.meta(&token).await;
    assert_eq!(tombstone.json["state"], "deleted");
    let gone = fixture
        .request(
            "GET",
            "/v1/me/vault/snapshot",
            Some(&token),
            &[("if-match", tombstone.tag())],
            vec![],
        )
        .await;
    assert_eq!(gone.status, 404);
    assert_eq!(gone.code(), "vault_deleted");
}
