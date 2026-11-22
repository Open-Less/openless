//! Device-local style assignments and the applications seen in successful dictation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, persistence_error, read_or_default};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppCategory {
    Coding,
    Chat,
    Email,
}

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

    pub fn category(&self) -> Option<AppCategory> {
        use AppCategory::{Chat, Coding, Email};
        if let Some(bundle) = self.macos_bundle_id() {
            return match bundle.to_ascii_lowercase().as_str() {
                "com.microsoft.vscode"
                | "com.microsoft.vscodeinsiders"
                | "com.apple.dt.xcode"
                | "com.todesktop.230313mzl4w4u92"
                | "dev.zed.zed"
                | "com.sublimetext.4"
                | "com.jetbrains.intellij"
                | "com.jetbrains.intellij.ce"
                | "com.jetbrains.pycharm"
                | "com.jetbrains.pycharm.ce"
                | "com.jetbrains.webstorm"
                | "com.google.android.studio"
                | "com.apple.terminal"
                | "com.googlecode.iterm2"
                | "com.mitchellh.ghostty"
                | "dev.warp.warp-stable" => Some(Coding),
                "com.tencent.xinwechat"
                | "com.tencent.qq"
                | "com.tencent.weworkmac"
                | "com.apple.ichat"
                | "com.tinyspeck.slackmacgap"
                | "com.hnc.discord"
                | "com.microsoft.teams2"
                | "com.microsoft.teams"
                | "ru.keepcoder.telegram"
                | "org.telegram.desktop"
                | "com.whatsapp.whatsapp"
                | "com.electron.lark"
                | "com.bytedance.macos.feishu"
                | "com.laiwang.dingtalk"
                | "us.zoom.xos" => Some(Chat),
                "com.apple.mail"
                | "com.microsoft.outlook"
                | "org.mozilla.thunderbird"
                | "com.readdle.smartemail-macos"
                | "com.mimestream.mimestream" => Some(Email),
                _ => None,
            };
        }
        if self.id.starts_with("windows:") {
            // The host supplies the executable stem, never a window title.
            return match self.name.to_ascii_lowercase().as_str() {
                "code" | "code - insiders" | "cursor" | "zed" | "devenv" | "idea64"
                | "pycharm64" | "webstorm64" | "studio64" | "sublime_text" | "windowsterminal" => {
                    Some(Coding)
                }
                "wechat" | "weixin" | "qq" | "wxwork" | "slack" | "discord" | "ms-teams"
                | "teams" | "telegram" | "whatsapp" | "lark" | "feishu" | "dingtalk" | "zoom" => {
                    Some(Chat)
                }
                "outlook" | "olk" | "thunderbird" => Some(Email),
                _ => None,
            };
        }
        None
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RoutingState {
    categories: BTreeMap<AppCategory, String>,
    apps: BTreeMap<String, AppEntry>,
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
    pub categories: BTreeMap<AppCategory, String>,
    pub apps: Vec<AppStyleEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStyleEntry {
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
            Self::Category { style_pack_id, .. } | Self::App { style_pack_id, .. } => {
                style_pack_id.as_deref()
            }
        }
    }
}

pub(crate) struct AppStyleStore {
    path: PathBuf,
    state: Mutex<Result<RoutingState, BackendError>>,
}

impl AppStyleStore {
    pub(crate) fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("app-style-routing.json");
        Self {
            state: Mutex::new(read_or_default(&path)),
            path,
        }
    }

    pub(crate) fn in_memory() -> Self {
        Self {
            path: PathBuf::new(),
            state: Mutex::new(Ok(RoutingState::default())),
        }
    }

    pub(crate) fn snapshot(&self) -> Result<AppStyleRouting, BackendError> {
        let state = self.state.lock().unwrap();
        state
            .as_ref()
            .map(RoutingState::snapshot)
            .map_err(Clone::clone)
    }

    pub(crate) fn candidates(&self, app: &InputApp) -> Vec<String> {
        let state = self.state.lock().unwrap();
        let Ok(state) = state.as_ref() else {
            return Vec::new();
        };
        [
            state
                .apps
                .get(&app.id)
                .and_then(|entry| entry.style_pack_id.as_ref()),
            app.category()
                .and_then(|category| state.categories.get(&category)),
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect()
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
                AppStyleAssignment::Category {
                    category,
                    style_pack_id: Some(id),
                } => {
                    if state
                        .categories
                        .iter()
                        .any(|(owner, assigned)| *owner != category && *assigned == id)
                    {
                        return Err(assignment_conflict());
                    }
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
                    if let Some(id) = &style_pack_id {
                        if state.apps.iter().any(|(owner, entry)| {
                            *owner != app_id && entry.style_pack_id.as_ref() == Some(id)
                        }) {
                            return Err(assignment_conflict());
                        }
                    }
                    let entry = state.apps.get_mut(&app_id).ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::InvalidArgument,
                            "application has not been observed",
                        )
                    })?;
                    entry.style_pack_id = style_pack_id;
                    if !entry.seen && entry.style_pack_id.is_none() {
                        state.apps.remove(&app_id);
                    }
                }
            }
            Ok(true)
        })
    }

    pub(crate) fn clear_seen(&self) -> Result<(), BackendError> {
        self.update(|state| {
            state.apps.retain(|_, entry| {
                entry.seen = false;
                entry.style_pack_id.is_some()
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
    fn snapshot(&self) -> AppStyleRouting {
        AppStyleRouting {
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
                        category: app.category(),
                        app,
                        seen: entry.seen,
                        style_pack_id: entry.style_pack_id.clone(),
                    }
                })
                .collect(),
        }
    }
}

fn assignment_conflict() -> BackendError {
    BackendError::new(
        BackendErrorCode::InvalidArgument,
        "style pack is already assigned",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_platform_identity_instead_of_a_macos_display_name() {
        assert_eq!(
            InputApp::macos("com.tencent.xinWeChat", Some("微信"))
                .unwrap()
                .category(),
            Some(AppCategory::Chat),
        );
        assert_eq!(
            InputApp::macos("com.example.unknown", Some("Mail"))
                .unwrap()
                .category(),
            None,
        );
        let windows = InputApp {
            id: "windows:mail-product".into(),
            name: "OUTLOOK".into(),
        };
        assert_eq!(windows.category(), Some(AppCategory::Email));
        assert!(InputApp::macos("", Some("Mail")).is_none());
    }

    #[test]
    fn assignments_are_unique_within_each_level_and_persist_independently_of_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStyleStore::open(dir.path());
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
            .set(assign_category(AppCategory::Coding, "shared"))
            .unwrap();
        assert!(store
            .set(assign_category(AppCategory::Email, "shared"))
            .is_err());
        store.set(override_with(&chat, Some("shared"))).unwrap();
        assert!(store.set(override_with(&editor, Some("shared"))).is_err());
        store.set(override_with(&editor, Some("override"))).unwrap();
        assert_eq!(store.candidates(&editor), ["override", "shared"]);
        store
            .set(override_with(&editor, Some("replacement")))
            .unwrap();
        store.set(override_with(&chat, Some("override"))).unwrap();
        store.set(override_with(&editor, None)).unwrap();

        store.clear_seen().unwrap();
        drop(store);
        let reopened = AppStyleStore::open(dir.path());
        let snapshot = reopened.snapshot().unwrap();
        assert_eq!(snapshot.categories.len(), 1);
        assert_eq!(snapshot.apps.len(), 1);
        assert_eq!(snapshot.apps[0].app, chat);
        assert!(!snapshot.apps[0].seen);
        assert_eq!(reopened.candidates(&editor), ["shared"]);
        assert_eq!(reopened.candidates(&chat), ["override"]);
        reopened.set(override_with(&chat, None)).unwrap();
        assert!(reopened.snapshot().unwrap().apps.is_empty());
    }
}
