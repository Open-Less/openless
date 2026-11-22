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
    Writing,
    Office,
    Browser,
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
        use AppCategory::{Browser, Chat, Coding, Email, Office, Writing};
        if let Some(bundle) = self.macos_bundle_id() {
            let bundle = bundle.to_ascii_lowercase();
            // Installed web apps have their own stable bundle identity. Browser
            // tabs retain the browser's category, regardless of their title.
            let installed_web_app = [
                "com.google.chrome.app.",
                "com.microsoft.edgemac.app.",
                "com.apple.safari.webapp.",
            ]
            .iter()
            .any(|prefix| bundle.starts_with(prefix));
            if installed_web_app
                && matches!(
                    self.name.to_ascii_lowercase().as_str(),
                    "gemini" | "google gemini" | "chatgpt" | "claude" | "google ai studio"
                )
            {
                return Some(Coding);
            }
            return match bundle.as_str() {
                // Agent clients, IDEs, terminals, and development tools.
                "com.openai.codex"
                | "com.openai.chat"
                | "com.anthropic.claudefordesktop"
                | "com.stablyai.orca"
                | "com.google.antigravity"
                | "ai.opencode.desktop"
                | "com.t3tools.t3code"
                | "dev.kiro.desktop"
                | "com.trae.app"
                | "com.conductor.app"
                | "com.superset.desktop"
                | "com.exafunction.windsurf"
                | "com.kangfenmao.cherrystudio"
                | "com.docker.docker"
                | "com.electron.dockerdesktop"
                | "com.microsoft.vscode"
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
                "com.apple.notes" | "md.obsidian" | "notion.id" | "abnerworks.typora"
                | "net.shinyfrog.bear" | "com.ulyssesapp.mac" => Some(Writing),
                "com.microsoft.word"
                | "com.microsoft.excel"
                | "com.microsoft.powerpoint"
                | "com.apple.iwork.pages"
                | "com.apple.iwork.numbers"
                | "com.apple.iwork.keynote" => Some(Office),
                "com.apple.safari"
                | "com.google.chrome"
                | "com.google.chrome.canary"
                | "org.mozilla.firefox"
                | "com.microsoft.edgemac"
                | "com.brave.browser"
                | "company.thebrowser.browser"
                | "com.vivaldi.vivaldi" => Some(Browser),
                _ => None,
            };
        }
        if self.id.starts_with("windows:") {
            // The host supplies the executable stem, never a window title.
            return match self.name.to_ascii_lowercase().as_str() {
                "chatgpt" | "codex" | "claude" | "orca" | "antigravity" | "opencode"
                | "t3 code" | "kiro" | "trae" | "windsurf" | "conductor" | "superset"
                | "cherry studio" | "docker desktop" | "code" | "code - insiders" | "cursor"
                | "zed" | "devenv" | "idea64" | "pycharm64" | "webstorm64" | "studio64"
                | "sublime_text" | "windowsterminal" => Some(Coding),
                "wechat" | "weixin" | "qq" | "wxwork" | "slack" | "discord" | "ms-teams"
                | "teams" | "telegram" | "whatsapp" | "lark" | "feishu" | "dingtalk" | "zoom" => {
                    Some(Chat)
                }
                "outlook" | "olk" | "thunderbird" => Some(Email),
                "obsidian" | "notion" | "typora" | "notepad" | "notepad++" => Some(Writing),
                "winword" | "excel" | "powerpnt" | "wps" | "et" | "wpp" => Some(Office),
                "chrome" | "msedge" | "firefox" | "brave" | "arc" | "vivaldi" => Some(Browser),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_catalog_classifies_agents_and_common_input_apps() {
        for bundle in [
            "com.openai.codex",
            "com.openai.chat",
            "com.stablyai.orca",
            "com.google.antigravity",
            "com.anthropic.claudefordesktop",
            "ai.opencode.desktop",
            "com.t3tools.t3code",
            "dev.kiro.desktop",
            "com.trae.app",
            "com.conductor.app",
            "com.superset.desktop",
            "com.docker.docker",
            "com.microsoft.VSCode",
        ] {
            assert_eq!(
                InputApp::macos(bundle, None).unwrap().category(),
                Some(AppCategory::Coding),
                "{bundle}"
            );
        }
        for (bundle, category) in [
            ("com.apple.Notes", AppCategory::Writing),
            ("com.tencent.xinWeChat", AppCategory::Chat),
            ("com.apple.mail", AppCategory::Email),
            ("com.microsoft.Word", AppCategory::Office),
            ("com.google.Chrome", AppCategory::Browser),
        ] {
            assert_eq!(
                InputApp::macos(bundle, None).unwrap().category(),
                Some(category)
            );
        }
        assert_eq!(
            InputApp::macos("com.google.Chrome.app.test-gemini", Some("Gemini"))
                .unwrap()
                .category(),
            Some(AppCategory::Coding)
        );
        assert_eq!(
            InputApp::macos("com.google.Chrome", Some("Gemini"))
                .unwrap()
                .category(),
            Some(AppCategory::Browser)
        );
        // A display name cannot turn an unknown app or browser tab into an agent.
        assert_eq!(
            InputApp::macos("org.example.unknown", Some("ChatGPT"))
                .unwrap()
                .category(),
            None
        );
        assert_eq!(
            InputApp::macos("com.macpaw.site.Gemini2", Some("Gemini"))
                .unwrap()
                .category(),
            None
        );
    }

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
    fn shared_packs_keep_one_assignment_per_target() {
        let dir =
            std::env::temp_dir().join(format!("openless-app-styles-{}", uuid::Uuid::new_v4()));
        let store = AppStyleStore::open(&dir);
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
        store
            .set(assign_category(AppCategory::Email, "shared"))
            .unwrap();
        store.set(override_with(&chat, Some("shared"))).unwrap();
        store.set(override_with(&editor, Some("shared"))).unwrap();
        let shared = store.snapshot().unwrap();
        assert_eq!(shared.categories[&AppCategory::Coding], "shared");
        assert_eq!(shared.categories[&AppCategory::Email], "shared");
        assert_eq!(shared.apps.len(), 2);
        assert!(shared
            .apps
            .iter()
            .all(|entry| entry.style_pack_id.as_deref() == Some("shared")));

        store
            .set(assign_category(AppCategory::Email, "replacement"))
            .unwrap();
        store.set(override_with(&editor, Some("override"))).unwrap();
        assert_eq!(store.candidates(&editor), ["override", "shared"]);
        assert_eq!(store.candidates(&chat), ["shared"]);
        store.set(override_with(&editor, None)).unwrap();

        store.clear_seen().unwrap();
        drop(store);
        let reopened = AppStyleStore::open(&dir);
        let snapshot = reopened.snapshot().unwrap();
        assert_eq!(snapshot.categories.len(), 2);
        assert_eq!(snapshot.categories[&AppCategory::Coding], "shared");
        assert_eq!(snapshot.categories[&AppCategory::Email], "replacement");
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
