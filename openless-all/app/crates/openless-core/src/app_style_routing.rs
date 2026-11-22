//! Device-local style assignments and the applications seen in successful dictation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::app_catalog::{AppCatalog, AppCatalogStore};

use serde::{Deserialize, Serialize};

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, persistence_error, read_or_default};

pub type AppCategory = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputApp {
    pub id: String,
    pub name: String,
}

impl InputApp {
    pub fn macos(bundle_id: &str, name: Option<&str>) -> Option<Self> {
        let bundle_id = bundle_id.trim();
        if bundle_id.is_empty() {
            return None;
        }
        Some(Self {
            id: format!("macos:{bundle_id}"),
            name: name
                .filter(|name| !name.is_empty())
                .unwrap_or(bundle_id)
                .to_owned(),
        })
    }

    pub fn macos_bundle_id(&self) -> Option<&str> {
        self.id.strip_prefix("macos:")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RoutingState {
    categories: BTreeMap<AppCategory, String>,
    apps: BTreeMap<String, AppEntry>,
    memberships: BTreeMap<String, Option<AppCategory>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppEntry {
    name: String,
    seen: bool,
    style_pack_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStyleRouting {
    pub catalog: AppCatalog,
    pub memberships: BTreeMap<String, Option<AppCategory>>,
    pub categories: BTreeMap<AppCategory, String>,
    pub apps: Vec<AppStyleEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStyleEntry {
    pub member_id: String,
    pub app: InputApp,
    pub category: Option<AppCategory>,
    pub seen: bool,
    pub style_pack_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "target",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AppStyleAssignment {
    Membership {
        member_id: String,
        category: Option<AppCategory>,
    },
    Category {
        category: AppCategory,
        style_pack_id: Option<String>,
    },
    App {
        app_id: String,
        style_pack_id: Option<String>,
    },
}

impl AppStyleAssignment {
    pub(crate) fn style_pack_id(&self) -> Option<&str> {
        match self {
            Self::Membership { .. } => None,
            Self::Category { style_pack_id, .. } | Self::App { style_pack_id, .. } => {
                style_pack_id.as_deref()
            }
        }
    }
}

pub(crate) struct AppStyleStore {
    pub(crate) catalog: Arc<AppCatalogStore>,
    path: PathBuf,
    state: Mutex<Result<RoutingState, BackendError>>,
}

impl AppStyleStore {
    pub(crate) fn open(data_dir: &Path, base_url: Option<reqwest::Url>) -> Self {
        let path = data_dir.join("app-style-routing.json");
        Self {
            catalog: Arc::new(AppCatalogStore::open(data_dir, base_url)),
            state: Mutex::new(read_or_default(&path)),
            path,
        }
    }

    pub(crate) fn in_memory() -> Self {
        Self {
            catalog: Arc::new(AppCatalogStore::in_memory()),
            path: PathBuf::new(),
            state: Mutex::new(Ok(RoutingState::default())),
        }
    }

    pub(crate) fn snapshot(&self) -> Result<AppStyleRouting, BackendError> {
        let state = self.state.lock().unwrap();
        state
            .as_ref()
            .map(|state| state.snapshot(&self.catalog.snapshot()))
            .map_err(Clone::clone)
    }

    pub(crate) fn selection(&self, app: &InputApp) -> (Vec<String>, Option<String>) {
        let state = self.state.lock().unwrap();
        let Ok(state) = state.as_ref() else {
            return (Vec::new(), None);
        };
        let catalog = self.catalog.snapshot();
        let category = state.category_for(app, &catalog);
        let ids = [
            state
                .apps
                .get(&app.id)
                .and_then(|entry| entry.style_pack_id.as_ref()),
            category
                .as_ref()
                .and_then(|category| state.categories.get(category)),
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        let category_guidance = category
            .and_then(|category| catalog.categories.iter().find(|item| item.id == category))
            .map(|category| category.prompt_guidance.as_str());
        let client_guidance = catalog
            .client_for(app)
            .map(|client| client.prompt_guidance.as_str());
        let guidance = [category_guidance, client_guidance]
            .into_iter()
            .flatten()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        (ids, (!guidance.is_empty()).then_some(guidance))
    }

    #[cfg(test)]
    fn candidates(&self, app: &InputApp) -> Vec<String> {
        self.selection(app).0
    }

    pub(crate) fn observe(&self, app: &InputApp) -> Result<(), BackendError> {
        self.update(|state| {
            if let Some(entry) = state.apps.get_mut(&app.id) {
                if entry.seen && entry.name == app.name {
                    return Ok(false);
                }
                entry.name.clone_from(&app.name);
                entry.seen = true;
            } else {
                state.apps.insert(
                    app.id.clone(),
                    AppEntry {
                        name: app.name.clone(),
                        seen: true,
                        style_pack_id: None,
                    },
                );
            }
            Ok(true)
        })
    }

    pub(crate) fn set(&self, assignment: AppStyleAssignment) -> Result<(), BackendError> {
        self.update(|state| {
            match assignment {
                AppStyleAssignment::Membership {
                    member_id,
                    category,
                } => {
                    let catalog = self.catalog.snapshot();
                    let known = state.apps.contains_key(&member_id)
                        || catalog
                            .clients
                            .iter()
                            .any(|client| format!("catalog:{}", client.id) == member_id);
                    if !known
                        || category
                            .as_ref()
                            .is_some_and(|id| !catalog.categories.iter().any(|item| &item.id == id))
                    {
                        return Err(BackendError::new(
                            BackendErrorCode::InvalidArgument,
                            "unknown application or category",
                        ));
                    }
                    // A newly recognized identity must replace its older app-specific membership.
                    if member_id.starts_with("catalog:") {
                        for (id, entry) in &state.apps {
                            let app = InputApp {
                                id: id.clone(),
                                name: entry.name.clone(),
                            };
                            if RoutingState::member_id(&app, &catalog) == member_id {
                                state.memberships.remove(id);
                            }
                        }
                    }
                    state.memberships.insert(member_id, category);
                }
                AppStyleAssignment::Category {
                    category,
                    style_pack_id: Some(id),
                } => {
                    state.categories.insert(category, id);
                }
                AppStyleAssignment::Category {
                    category,
                    style_pack_id: None,
                } => {
                    state.categories.remove(&category);
                }
                AppStyleAssignment::App {
                    app_id,
                    style_pack_id,
                } => {
                    let entry = state.apps.get_mut(&app_id).ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::InvalidArgument,
                            "application has not been observed",
                        )
                    })?;
                    entry.style_pack_id = style_pack_id;
                    if !entry.seen
                        && entry.style_pack_id.is_none()
                        && !state.memberships.contains_key(&app_id)
                    {
                        state.apps.remove(&app_id);
                    }
                }
            }
            Ok(true)
        })
    }

    pub(crate) fn clear_seen(&self) -> Result<(), BackendError> {
        self.update(|state| {
            state.apps.retain(|id, entry| {
                entry.seen = false;
                entry.style_pack_id.is_some() || state.memberships.contains_key(id)
            });
            Ok(true)
        })
    }

    fn update(
        &self,
        change: impl FnOnce(&mut RoutingState) -> Result<bool, BackendError>,
    ) -> Result<(), BackendError> {
        let mut state = self.state.lock().unwrap();
        let mut next = state.as_ref().map_err(Clone::clone)?.clone();
        if change(&mut next)? {
            if !self.path.as_os_str().is_empty() {
                let bytes = serde_json::to_vec(&next)
                    .map_err(|_| persistence_error("encode app style routing"))?;
                if let Err(error) = atomic_write(&self.path, &bytes) {
                    if error.code == BackendErrorCode::OutcomeUnknown {
                        *state = read_or_default(&self.path);
                    }
                    return Err(error);
                }
            }
            *state = Ok(next);
        }
        Ok(())
    }
}

impl RoutingState {
    fn member_id(app: &InputApp, catalog: &AppCatalog) -> String {
        catalog
            .client_for(app)
            .map_or_else(|| app.id.clone(), |client| format!("catalog:{}", client.id))
    }

    fn category_for(&self, app: &InputApp, catalog: &AppCatalog) -> Option<AppCategory> {
        let member = Self::member_id(app, catalog);
        self.memberships
            .get(&app.id)
            .or_else(|| self.memberships.get(&member))
            .cloned()
            .unwrap_or_else(|| {
                catalog
                    .client_for(app)
                    .map(|client| client.category.clone())
            })
    }

    fn snapshot(&self, catalog: &AppCatalog) -> AppStyleRouting {
        AppStyleRouting {
            catalog: catalog.clone(),
            memberships: self.memberships.clone(),
            categories: self.categories.clone(),
            apps: self
                .apps
                .iter()
                .map(|(id, entry)| {
                    let app = InputApp {
                        id: id.clone(),
                        name: entry.name.clone(),
                    };
                    AppStyleEntry {
                        category: self.category_for(&app, catalog),
                        member_id: Self::member_id(&app, catalog),
                        app,
                        seen: entry.seen,
                        style_pack_id: entry.style_pack_id.clone(),
                    }
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_moves_replace_the_category_and_preserve_specific_styles() {
        let dir =
            std::env::temp_dir().join(format!("openless-membership-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("input-app-catalog-cache.json"),
            include_str!("../tests/fixtures/app-catalog.json"),
        )
        .unwrap();
        let store = AppStyleStore::open(&dir, None);
        let editor = InputApp::macos("com.microsoft.VSCode", Some("VS Code")).unwrap();
        for (category, pack) in [("coding", "code-style"), ("office", "office-style")] {
            store
                .set(AppStyleAssignment::Category {
                    category: category.into(),
                    style_pack_id: Some(pack.into()),
                })
                .unwrap();
        }
        // An official app can be moved before its first dictation.
        store
            .set(AppStyleAssignment::Membership {
                member_id: "catalog:vscode".into(),
                category: Some("office".into()),
            })
            .unwrap();
        assert_eq!(store.candidates(&editor), ["office-style"]);
        store.observe(&editor).unwrap();
        store
            .set(AppStyleAssignment::App {
                app_id: editor.id.clone(),
                style_pack_id: Some("specific-style".into()),
            })
            .unwrap();
        assert_eq!(
            store.candidates(&editor),
            ["specific-style", "office-style"]
        );
        store
            .set(AppStyleAssignment::Membership {
                member_id: "catalog:vscode".into(),
                category: Some("coding".into()),
            })
            .unwrap();
        assert_eq!(store.candidates(&editor), ["specific-style", "code-style"]);
        store
            .set(AppStyleAssignment::Membership {
                member_id: "catalog:vscode".into(),
                category: None,
            })
            .unwrap();
        assert_eq!(store.candidates(&editor), ["specific-style"]);
        drop(store);
        let reopened = AppStyleStore::open(&dir, None);
        assert_eq!(reopened.snapshot().unwrap().apps[0].category, None);
        assert_eq!(reopened.candidates(&editor), ["specific-style"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shared_packs_keep_one_assignment_per_target() {
        let dir =
            std::env::temp_dir().join(format!("openless-app-styles-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("input-app-catalog-cache.json"),
            include_str!("../tests/fixtures/app-catalog.json"),
        )
        .unwrap();
        let store = AppStyleStore::open(&dir, None);
        let editor = InputApp::macos("com.microsoft.VSCode", Some("VS Code")).unwrap();
        let chat = InputApp::macos("com.tencent.xinWeChat", Some("WeChat")).unwrap();
        let override_with = |app: &InputApp, pack: Option<&str>| AppStyleAssignment::App {
            app_id: app.id.clone(),
            style_pack_id: pack.map(str::to_owned),
        };
        let assign_category = |category, pack: &str| AppStyleAssignment::Category {
            category,
            style_pack_id: Some(pack.into()),
        };
        assert!(store.set(override_with(&editor, Some("code"))).is_err());
        store.observe(&editor).unwrap();
        store.observe(&chat).unwrap();
        store
            .set(assign_category("coding".to_owned(), "shared"))
            .unwrap();
        store
            .set(assign_category("email".to_owned(), "shared"))
            .unwrap();
        store.set(override_with(&chat, Some("shared"))).unwrap();
        store.set(override_with(&editor, Some("shared"))).unwrap();
        let shared = store.snapshot().unwrap();
        assert_eq!(shared.categories[&"coding".to_owned()], "shared");
        assert_eq!(shared.categories[&"email".to_owned()], "shared");
        assert_eq!(shared.apps.len(), 2);
        assert!(shared
            .apps
            .iter()
            .all(|entry| entry.style_pack_id.as_deref() == Some("shared")));

        store
            .set(assign_category("email".to_owned(), "replacement"))
            .unwrap();
        store.set(override_with(&editor, Some("override"))).unwrap();
        assert_eq!(store.candidates(&editor), ["override", "shared"]);
        assert_eq!(store.candidates(&chat), ["shared"]);
        store.set(override_with(&editor, None)).unwrap();

        store.clear_seen().unwrap();
        drop(store);
        let reopened = AppStyleStore::open(&dir, None);
        let snapshot = reopened.snapshot().unwrap();
        assert_eq!(snapshot.categories.len(), 2);
        assert_eq!(snapshot.categories[&"coding".to_owned()], "shared");
        assert_eq!(snapshot.categories[&"email".to_owned()], "replacement");
        assert_eq!(snapshot.apps.len(), 1);
        assert_eq!(snapshot.apps[0].app, chat);
        assert!(!snapshot.apps[0].seen);
        assert_eq!(reopened.candidates(&editor), ["shared"]);
        assert_eq!(reopened.candidates(&chat), ["shared"]);
        reopened.set(override_with(&chat, None)).unwrap();
        assert!(reopened.snapshot().unwrap().apps.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
