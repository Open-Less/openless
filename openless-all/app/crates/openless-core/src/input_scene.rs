//! Shipped application recognition. Display names and matching words help describe
//! a verified application; they never authorize document access or input actions.

use crate::app_style_routing::InputApp;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplicationCategory {
    CodingAgent,
    CodeEditor,
    Browser,
    Chat,
    Email,
    Writing,
}

impl ApplicationCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::CodingAgent => "编程 Agent",
            Self::CodeEditor => "编程",
            Self::Browser => "浏览器",
            Self::Chat => "聊天",
            Self::Email => "邮件",
            Self::Writing => "写作与笔记",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputPolicy {
    SelectedStyle,
    CodingAgentInstruction,
}

impl OutputPolicy {
    /// A style constraint, never permission to execute the dictated task.
    pub fn guidance(self) -> Option<&'static str> {
        match self {
            Self::SelectedStyle => None,
            Self::CodingAgentInstruction => Some(
                "本次输入将交给编程 Agent。保留用户表达的目标、约束、文件名、技术名词和验收要求，\
                 整理为可直接交给 Agent 的任务描述；不代替 Agent 执行、回答或补充实现方案，\
                 不虚构需求。是否采用列表仍遵循用户所选风格。",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserFamily {
    Safari,
    Chromium,
    Ego,
    Dia,
    ChatGptEmbedded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplicationRule {
    pub id: &'static str,
    pub name: &'static str,
    pub category: ApplicationCategory,
    pub purpose: &'static str,
    pub output_policy: OutputPolicy,
    pub browser: Option<BrowserFamily>,
    pub macos_bundle_ids: &'static [&'static str],
    pub windows_executable_stems: &'static [&'static str],
    pub aliases: &'static [&'static str],
    pub matching_words: &'static [&'static str],
}

/// Trusted catalog facts only. No process path, page label, title or URL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationScene {
    pub name: &'static str,
    pub category: ApplicationCategory,
    pub purpose: &'static str,
    pub output_policy: OutputPolicy,
}

impl ApplicationRule {
    pub fn scene(&self) -> ApplicationScene {
        ApplicationScene {
            name: self.name,
            category: self.category,
            purpose: self.purpose,
            output_policy: self.output_policy,
        }
    }
}

macro_rules! rule {
    ($id:literal, $name:literal, $category:ident, $purpose:literal,
     $policy:ident, $browser:expr, [$($bundle:literal),*], [$($exe:literal),*],
     [$($alias:literal),*], [$($word:literal),*]) => {
        ApplicationRule {
            id: $id, name: $name, category: ApplicationCategory::$category,
            purpose: $purpose, output_policy: OutputPolicy::$policy,
            browser: $browser, macos_bundle_ids: &[$($bundle),*],
            windows_executable_stems: &[$($exe),*], aliases: &[$($alias),*],
            matching_words: &[$($word),*],
        }
    };
}

/// Exact OS identities only. In particular, "Ida", "Eagle", "light", window
/// titles and page text cannot turn an arbitrary process into a supported browser.
pub const APPLICATION_RULES: &[ApplicationRule] = &[
    rule!(
        "cursor",
        "Cursor",
        CodingAgent,
        "编辑代码并向编程 Agent 描述开发任务",
        CodingAgentInstruction,
        None,
        ["com.todesktop.230313mzl4w4u92"],
        ["cursor"],
        ["Cursor Editor"],
        ["代码", "Agent", "编程"]
    ),
    rule!(
        "orca",
        "Orca",
        CodingAgent,
        "向编程 Agent 描述、协作和验收开发任务",
        CodingAgentInstruction,
        None,
        ["com.stablyai.orca"],
        ["orca"],
        ["Orca IDE"],
        ["代码", "Agent", "工作区"]
    ),
    rule!(
        "vscode",
        "Visual Studio Code",
        CodeEditor,
        "编辑代码和技术文本",
        SelectedStyle,
        None,
        ["com.microsoft.VSCode"],
        ["code"],
        ["VS Code", "VSCode"],
        ["代码", "编辑器"]
    ),
    rule!(
        "safari",
        "Safari",
        Browser,
        "在浏览器网页或浏览器控件内输入",
        SelectedStyle,
        Some(BrowserFamily::Safari),
        ["com.apple.Safari"],
        [],
        [],
        ["网页", "浏览器"]
    ),
    rule!(
        "chrome",
        "Google Chrome",
        Browser,
        "在浏览器网页或浏览器控件内输入",
        SelectedStyle,
        Some(BrowserFamily::Chromium),
        [
            "com.google.Chrome",
            "com.google.Chrome.canary",
            "org.chromium.Chromium"
        ],
        ["chrome", "chromium"],
        ["Chrome", "Chromium"],
        ["网页", "浏览器"]
    ),
    rule!(
        "edge",
        "Microsoft Edge",
        Browser,
        "在浏览器网页或浏览器控件内输入",
        SelectedStyle,
        Some(BrowserFamily::Chromium),
        ["com.microsoft.edgemac"],
        ["msedge"],
        ["Edge"],
        ["网页", "浏览器"]
    ),
    rule!(
        "ego-lite",
        "ego lite",
        Browser,
        "在浏览器网页或浏览器控件内输入",
        SelectedStyle,
        Some(BrowserFamily::Ego),
        ["com.citrolabs.ego.lite"],
        [],
        ["ego", "ego-lite"],
        ["网页", "浏览器"]
    ),
    rule!(
        "dia",
        "Dia",
        Browser,
        "在浏览器网页或浏览器控件内输入",
        SelectedStyle,
        Some(BrowserFamily::Dia),
        ["company.thebrowser.dia"],
        [],
        ["Dia Browser"],
        ["网页", "浏览器"]
    ),
    // App recognition alone does not establish that an embedded browser is open.
    rule!(
        "chatgpt",
        "ChatGPT",
        Chat,
        "对话；仅在公开控件结构证实后提供内嵌网页场景",
        SelectedStyle,
        Some(BrowserFamily::ChatGptEmbedded),
        ["com.openai.chat"],
        [],
        ["Chat GPT"],
        ["对话"]
    ),
    rule!(
        "wechat",
        "WeChat",
        Chat,
        "聊天和消息输入",
        SelectedStyle,
        None,
        ["com.tencent.xinWeChat"],
        ["wechat", "weixin"],
        ["微信", "Weixin"],
        ["消息"]
    ),
    rule!(
        "mail",
        "Mail",
        Email,
        "撰写邮件",
        SelectedStyle,
        None,
        ["com.apple.mail"],
        [],
        ["Apple Mail", "邮件"],
        ["邮件"]
    ),
    rule!(
        "notes",
        "Notes",
        Writing,
        "记录笔记和撰写文本",
        SelectedStyle,
        None,
        ["com.apple.Notes"],
        [],
        ["Apple Notes", "备忘录"],
        ["笔记"]
    ),
];

/// A renamed application still matches by its exact bundle/executable identity.
/// An unknown process named "Cursor", "Orca" or "Chrome" stays ordinary.
pub fn recognize(app: &InputApp) -> Option<&'static ApplicationRule> {
    if let Some(bundle) = app.macos_bundle_id() {
        return APPLICATION_RULES.iter().find(|rule| {
            rule.macos_bundle_ids
                .iter()
                .any(|known| known.eq_ignore_ascii_case(bundle))
        });
    }
    // The Windows host hashes the process path into id and supplies its actual
    // executable stem as name (activity_app_icons::app_for_pid). This is an
    // exact stem match, never a window title or substring heuristic.
    let _path_key = app.id.strip_prefix("windows:")?;
    let stem = &app.name;
    APPLICATION_RULES.iter().find(|rule| {
        rule.windows_executable_stems
            .iter()
            .any(|known| known.eq_ignore_ascii_case(stem))
    })
}

/// Catalog/discovery lookup only; deliberately separate from OS recognition.
pub fn named_rule(name_or_alias: &str) -> Option<&'static ApplicationRule> {
    APPLICATION_RULES.iter().find(|rule| {
        rule.name.eq_ignore_ascii_case(name_or_alias.trim())
            || rule
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name_or_alias.trim()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_use_exact_identity_even_when_the_display_name_changes() {
        for bundle in ["com.todesktop.230313mzl4w4u92", "com.stablyai.orca"] {
            let app = InputApp::macos(bundle, Some("renamed")).unwrap();
            let rule = recognize(&app).unwrap();
            assert_eq!(rule.category, ApplicationCategory::CodingAgent);
            assert!(rule
                .output_policy
                .guidance()
                .unwrap()
                .contains("不虚构需求"));
        }
    }

    #[test]
    fn names_matching_words_and_pending_aliases_never_grant_recognition() {
        for name in [
            "Cursor",
            "Orca",
            "Chrome",
            "Ask anything",
            "Ida",
            "Eagle",
            "light",
            "Agent",
        ] {
            assert!(recognize(&InputApp::macos("example.unknown", Some(name)).unwrap()).is_none());
        }
        assert_eq!(named_rule(" VS Code ").unwrap().id, "vscode");
        assert!(named_rule("Ida").is_none());
        assert!(named_rule("ChatGPT Atlas").is_none());
    }

    #[test]
    fn windows_identity_comes_from_the_process_stem_and_other_platforms_stay_ordinary() {
        assert_eq!(
            recognize(&InputApp {
                id: "windows:process-path-key".into(),
                name: "Cursor".into()
            })
            .unwrap()
            .id,
            "cursor"
        );
        assert!(recognize(&InputApp {
            id: "linux:cursor".into(),
            name: "Cursor".into()
        })
        .is_none());
        assert!(recognize(&InputApp {
            id: "windows:process-path-key".into(),
            name: "Cursor Notes".into()
        })
        .is_none());
    }
}
