//! Application recognition data fetched from the public catalog API.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::app_style_routing::InputApp;
use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, read_or_default};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppCatalog {
    pub schema_version: u32,
    pub reviewed_at: String,
    pub categories: Vec<CatalogCategory>,
    pub clients: Vec<CatalogClient>,
    #[serde(flatten)]
    pub details: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub prompt_guidance: String,
    #[serde(flatten)]
    pub details: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogClient {
    pub id: String,
    pub name: String,
    pub role: String,
    pub category: String,
    pub platforms: Vec<String>,
    pub identifiers: ClientIdentifiers,
    #[serde(default)]
    pub canonical_client_id: Option<String>,
    #[serde(default)]
    pub prompt_guidance: String,
    #[serde(flatten)]
    pub details: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClientIdentifiers {
    pub macos_bundle_ids: Vec<String>,
    pub macos_bundle_prefixes: Vec<String>,
    pub windows_executable_stems: Vec<String>,
    pub installed_web_app_names: Vec<String>,
}

impl CatalogClient {
    fn matches(&self, app: &InputApp) -> bool {
        let ids = &self.identifiers;
        if let Some(bundle) = app.macos_bundle_id() {
            return ids
                .macos_bundle_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case(bundle))
                || (ids.macos_bundle_prefixes.iter().any(|prefix| {
                    bundle
                        .to_ascii_lowercase()
                        .starts_with(&prefix.to_ascii_lowercase())
                }) && ids
                    .installed_web_app_names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&app.name)));
        }
        app.id.starts_with("windows:")
            && ids
                .windows_executable_stems
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&app.name))
    }
}

impl AppCatalog {
    pub fn client_for(&self, app: &InputApp) -> Option<&CatalogClient> {
        let matched = self.clients.iter().find(|client| client.matches(app))?;
        matched
            .canonical_client_id
            .as_ref()
            .and_then(|id| self.clients.iter().find(|client| &client.id == id))
            .or(Some(matched))
    }

    fn validate(&self) -> Result<(), BackendError> {
        if self.schema_version != 1 || self.categories.is_empty() {
            return Err(catalog_error("unsupported application catalog"));
        }
        if self.clients.iter().any(|client| {
            !self
                .categories
                .iter()
                .any(|category| category.id == client.category)
                || client
                    .identifiers
                    .macos_bundle_prefixes
                    .iter()
                    .any(String::is_empty)
        }) {
            return Err(catalog_error("invalid application catalog references"));
        }
        Ok(())
    }
}

pub(crate) struct AppCatalogStore {
    path: Option<PathBuf>,
    base_url: Option<reqwest::Url>,
    current: RwLock<AppCatalog>,
    refresh_gate: tokio::sync::Mutex<()>,
}

impl AppCatalogStore {
    pub(crate) fn open(data_dir: &Path, base_url: Option<reqwest::Url>) -> Self {
        let path = data_dir.join("input-app-catalog-cache.json");
        let catalog = read_or_default::<AppCatalog>(&path)
            .ok()
            .filter(|catalog| catalog.validate().is_ok())
            .unwrap_or_default();
        Self {
            path: Some(path),
            base_url,
            current: RwLock::new(catalog),
            refresh_gate: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) fn in_memory() -> Self {
        Self {
            path: None,
            base_url: None,
            current: RwLock::new(AppCatalog::default()),
            refresh_gate: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) fn snapshot(&self) -> AppCatalog {
        self.current.read().unwrap().clone()
    }

    pub(crate) async fn refresh(&self) -> Result<AppCatalog, BackendError> {
        let _refresh = self.refresh_gate.lock().await;
        let base = self
            .base_url
            .as_ref()
            .ok_or_else(|| catalog_error("application catalog API unavailable"))?;
        let url = base
            .join("v1/app-catalog")
            .map_err(|_| catalog_error("invalid application catalog URL"))?;
        let response = crate::net::credential_http_for_url(base.as_str())
            .get(url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|_| catalog_error("application catalog download failed"))?
            .error_for_status()
            .map_err(|_| catalog_error("application catalog API unavailable"))?;
        let catalog: AppCatalog = response
            .json()
            .await
            .map_err(|_| catalog_error("invalid application catalog response"))?;
        catalog.validate()?;
        if let Some(path) = &self.path {
            let bytes = serde_json::to_vec(&catalog)
                .map_err(|_| catalog_error("encode application catalog"))?;
            atomic_write(path, &bytes)?;
        }
        *self.current.write().unwrap() = catalog.clone();
        Ok(catalog)
    }
}

fn catalog_error(message: &str) -> BackendError {
    BackendError::new(BackendErrorCode::Provider, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn canonical_identity_uses_one_client_even_when_an_alias_matches_first() {
        let mut catalog: AppCatalog =
            serde_json::from_str(include_str!("../tests/fixtures/app-catalog.json")).unwrap();
        let mut alias = catalog.clients[0].clone();
        alias.id = "old-editor-name".into();
        alias.canonical_client_id = Some("vscode".into());
        catalog.clients.insert(0, alias);
        let app = InputApp::macos("com.microsoft.VSCode", Some("Renamed application")).unwrap();
        assert_eq!(catalog.client_for(&app).unwrap().id, "vscode");
    }

    #[tokio::test]
    async fn api_catalog_updates_recognition_and_keeps_the_last_good_cache_offline() {
        let directory =
            std::env::temp_dir().join(format!("openless-catalog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let original = include_str!("../tests/fixtures/app-catalog.json");
        std::fs::write(directory.join("input-app-catalog-cache.json"), original).unwrap();
        let mut changed: serde_json::Value = serde_json::from_str(original).unwrap();
        changed["clients"][0]["category"] = serde_json::json!("office");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base =
            reqwest::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let count = socket.read(&mut request).await.unwrap();
            assert!(std::str::from_utf8(&request[..count])
                .unwrap()
                .starts_with("GET /v1/app-catalog HTTP/1.1"));
            let body = changed.to_string();
            let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(reply.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        let store = AppCatalogStore::open(&directory, Some(base));
        let app = InputApp::macos("com.microsoft.VSCode", Some("Any display name")).unwrap();
        assert_eq!(
            store.snapshot().client_for(&app).unwrap().category,
            "coding"
        );
        store.refresh().await.unwrap();
        server.await.unwrap();
        assert_eq!(
            store.snapshot().client_for(&app).unwrap().category,
            "office"
        );
        assert!(store.refresh().await.is_err());
        let reopened = AppCatalogStore::open(&directory, None);
        assert_eq!(
            reopened.snapshot().client_for(&app).unwrap().category,
            "office"
        );
        assert!(reopened
            .snapshot()
            .client_for(
                &InputApp::macos("org.example.unknown", Some("Visual Studio Code")).unwrap()
            )
            .is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
