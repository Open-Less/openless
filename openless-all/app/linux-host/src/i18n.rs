//! Rust 原生侧的本地化。
//!
//! 界面文案全部在 React（`src/i18n/*.ts`），Rust 不再复制一份 UI 目录：
//! 之前那个 13k 行的生成表只为已删除的自绘界面存在，同步脚本
//! （`scripts/sync-host-i18n.mjs`）也随之删除。
//!
//! 这里只保留**原生侧真的会显示**的字符串：托盘菜单，以及 core 以错误码
//! 形式返回、需要在界面里按语言呈现的几条提示。语言集合与前端一致
//! （`system` + 8 个具体 locale），`zh-CN` 仍是源文。

use std::fmt::Display;

/// The five concrete languages the Linux UI UI supports.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Lang {
    ZhCn,
    ZhTw,
    En,
    Ja,
    Ko,
    Es,
    Fr,
    De,
}

/// The persisted UI-locale preference. `System` means "follow the host OS
/// locale"; `Lang(lang)` is an explicit user choice, matching the Tauri
/// `setLocalePreference` model where only an explicit tag is stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocalePref {
    System,
    Lang(Lang),
}

pub const LANGS: [Lang; 8] = [
    Lang::ZhCn,
    Lang::ZhTw,
    Lang::En,
    Lang::Ja,
    Lang::Ko,
    Lang::Es,
    Lang::Fr,
    Lang::De,
];

/// JSON/wire tags, matching the Tauri `SUPPORTED_LOCALES`.
pub const FOLLOW_SYSTEM: &str = "system";

impl Lang {
    /// Canonical BCP-47-ish tag for a concrete language.
    pub fn tag(self) -> &'static str {
        match self {
            Lang::ZhCn => "zh-CN",
            Lang::ZhTw => "zh-TW",
            Lang::En => "en",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::De => "de",
        }
    }

    /// Parse a BCP-47 tag / locale identifier into a supported language.
    /// Handles region and script suffixes (`zh-Hant-TW`, `zh_TW`, `ja_JP`…).
    pub fn parse(tag: &str) -> Option<Lang> {
        let normalized = tag.replace('-', "_").to_ascii_lowercase();
        if normalized.starts_with("zh") {
            // Traditional markers win regardless of where they appear.
            if normalized.contains("hant")
                || normalized.contains("_tw")
                || normalized.contains("_hk")
                || normalized.contains("_mo")
            {
                return Some(Lang::ZhTw);
            }
            return Some(Lang::ZhCn);
        }
        if normalized.starts_with("ja") {
            return Some(Lang::Ja);
        }
        if normalized.starts_with("ko") {
            return Some(Lang::Ko);
        }
        if normalized.starts_with("en") {
            return Some(Lang::En);
        }
        if normalized.starts_with("es") {
            return Some(Lang::Es);
        }
        if normalized.starts_with("fr") {
            return Some(Lang::Fr);
        }
        if normalized.starts_with("de") {
            return Some(Lang::De);
        }
        None
    }
}

impl LocalePref {
    pub fn from_tag(tag: &str) -> LocalePref {
        if tag.eq_ignore_ascii_case(FOLLOW_SYSTEM) {
            LocalePref::System
        } else if let Some(lang) = Lang::parse(tag) {
            LocalePref::Lang(lang)
        } else {
            LocalePref::System
        }
    }

    pub fn to_tag(self) -> String {
        match self {
            LocalePref::System => FOLLOW_SYSTEM.to_string(),
            LocalePref::Lang(lang) => lang.tag().to_string(),
        }
    }

    /// Resolve this preference against the running host into a concrete lang.
    /// `System` falls through to `resolve_system_lang()`, mirroring Tauri's
    /// `detectSystemLocale()`.
    pub fn resolve(self) -> Lang {
        match self {
            LocalePref::System => resolve_system_lang(),
            LocalePref::Lang(lang) => lang,
        }
    }
}

/// Resolve the host OS locale into a supported language without touching any
/// UI. Uses the same precedence as typical Linux tooling: `LC_ALL`, then
/// `LC_MESSAGES`, then `LANG`; an unparseable or unset value falls back to `en`
/// rather than guessing.
pub fn resolve_system_lang() -> Lang {
    for variable in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(variable) {
            if let Some(lang) = Lang::parse(&value) {
                return lang;
            }
        }
    }
    Lang::En
}

/// One catalog row: a stable key plus the text in all five concrete locales.
/// Index `0` is the `zh-CN` source of truth.
pub struct Msg {
    pub key: &'static str,
    pub text: [&'static str; 8],
}

/// Order helper so callers can write rows positionally and stay readable.
/// `[zh, zh_tw, en, ja, ko, es, fr, de]` is the single canonical order.
///
/// 只有 zh/zh-TW/en/ja/ko 是逐条人工翻译的；es/fr/de 未翻译的条目**回落到英文**
/// （英文本身是完整翻译，比留空导致回落到中文更合理）。
#[allow(dead_code)]
const fn row(
    zh: &'static str,
    zh_tw: &'static str,
    en: &'static str,
    ja: &'static str,
    ko: &'static str,
) -> [&'static str; 8] {
    [zh, zh_tw, en, ja, ko, en, en, en]
}

/// 需要西/法/德真实译文时用它（顺序同上，末尾三个为 es/fr/de）。
#[allow(dead_code, clippy::too_many_arguments)]
const fn row8(
    zh: &'static str,
    zh_tw: &'static str,
    en: &'static str,
    ja: &'static str,
    ko: &'static str,
    es: &'static str,
    fr: &'static str,
    de: &'static str,
) -> [&'static str; 8] {
    [zh, zh_tw, en, ja, ko, es, fr, de]
}

// Global catalog of every UI string the Linux Linux host renders.
//
// Convention:
//   * `zh` (zh-CN) is the source of truth and is never empty.
//   * A `[&str;5]` value that is empty for a non-zh locale means "fall back to
//     zh-CN for this key" (`tr` handles that); the completeness test asserts
//     that the zh-CN column is fully populated and that every key actually
//     referenced resolves.

pub const CATALOG: &[Msg] = &[
    Msg {
        key: "tray.show",
        text: [
            "显示 OpenLess",
            "顯示 OpenLess",
            "Show OpenLess",
            "OpenLess を表示",
            "OpenLess 표시",
            "Mostrar OpenLess",
            "Afficher OpenLess",
            "OpenLess anzeigen"
        ],
    },
    Msg {
        key: "tray.previous_style",
        text: [
            "切换到上一风格",
            "切換到上一風格",
            "Switch to previous style",
            "前のスタイルに切り替え",
            "이전 스타일로 전환",
            "Cambiar al estilo anterior",
            "Passer au style précédent",
            "Zum vorherigen Stil wechseln"
        ],
    },
    Msg {
        key: "tray.quit",
        text: [
            "退出",
            "結束",
            "Quit",
            "終了",
            "종료",
            "Salir",
            "Quitter",
            "Beenden"
        ],
    },
    Msg {
        key: "settings.system_default",
        text: [
            "系统默认",
            "系統預設",
            "System default",
            "システム既定",
            "시스템 기본",
            "Predeterminado del sistema",
            "Par défaut du système",
            "Systemstandard"
        ],
    },
    Msg {
        key: "settings.microphone",
        text: [
            "麦克风",
            "麥克風",
            "Microphone",
            "マイク",
            "마이크",
            "Micrófono",
            "Microphone",
            "Mikrofon"
        ],
    },
    Msg {
        key: "linuxAccess.keyringLocked",
        text: [
            "钥匙串已锁定：请在系统弹窗里解锁后重试。已保存的密钥没有被删除。",
            "鑰匙圈已鎖定：請在系統彈窗裡解鎖後重試。已儲存的密鑰沒有被刪除。",
            "The keyring is locked: unlock it in the system dialog and retry. Saved keys were not deleted.",
            "キーチェーンがロックされています。システムのダイアログでロックを解除して再試行してください。保存済みのキーは削除されていません。",
            "키링이 잠겨 있습니다. 시스템 대화상자에서 잠금을 해제한 뒤 다시 시도하세요. 저장된 키는 삭제되지 않았습니다.",
            "El llavero está bloqueado: desbloquéalo en el diálogo del sistema y vuelve a intentarlo. Las claves guardadas no se han eliminado.",
            "Le trousseau est verrouillé : déverrouillez-le dans la boîte de dialogue du système puis réessayez. Les clés enregistrées n'ont pas été supprimées.",
            "Der Schlüsselbund ist gesperrt: entsperren Sie ihn im Systemdialog und versuchen Sie es erneut. Gespeicherte Schlüssel wurden nicht gelöscht.",
        ],
    },
    Msg {
        key: "linuxAccess.keyringUnlockCancelled",
        text: [
            "钥匙串仍处于锁定状态（已取消或超时）。请解锁后重试，已保存的密钥没有丢失。",
            "鑰匙圈仍處於鎖定狀態（已取消或逾時）。請解鎖後重試，已儲存的密鑰沒有遺失。",
            "The keyring is still locked (cancelled or timed out). Unlock it and retry; saved keys were not lost.",
            "キーチェーンはロックされたままです（キャンセルまたはタイムアウト）。ロックを解除して再試行してください。保存済みのキーは失われていません。",
            "키링이 여전히 잠겨 있습니다(취소 또는 시간 초과). 잠금을 해제한 뒤 다시 시도하세요. 저장된 키는 사라지지 않았습니다.",
            "El llavero sigue bloqueado (cancelado o tiempo de espera agotado). Desbloquéalo y vuelve a intentarlo; las claves guardadas no se han perdido.",
            "Le trousseau est toujours verrouillé (annulé ou délai dépassé). Déverrouillez-le puis réessayez ; les clés enregistrées n'ont pas été perdues.",
            "Der Schlüsselbund ist weiterhin gesperrt (abgebrochen oder Zeitüberschreitung). Entsperren Sie ihn und versuchen Sie es erneut; gespeicherte Schlüssel gingen nicht verloren.",
        ],
    },
    Msg {
        key: "settings.providers.llmThinkingRequired",
        text: [
            "该模型不接受关闭思考。请打开模型旁的「思考」开关后重新验证。",
            "此模型不接受關閉思考。請開啟模型旁的「思考」開關後重新驗證。",
            "This model does not accept thinking turned off. Turn on Thinking next to the model, then verify again.",
            "このモデルは思考をオフにできません。モデル欄の横の「思考」をオンにしてから、もう一度検証してください。",
            "이 모델은 사고를 끌 수 없습니다. 모델 옆의 \"사고\"를 켠 뒤 다시 검증하세요.",
            "Este modelo no admite el razonamiento desactivado. Activa «Razonamiento» junto al modelo y vuelve a verificar.",
            "Ce modèle n’accepte pas le raisonnement désactivé. Activez « Raisonnement » à côté du modèle, puis vérifiez à nouveau.",
            "Dieses Modell lässt sich nicht mit ausgeschaltetem Denkmodus nutzen. Schalte „Denkmodus“ neben dem Modell ein und prüfe erneut.",
        ],
    },
    Msg {
        key: "metric.near7",
        text: [
            "近7天 {} 段 · 近30天 {} 段",
            "近7天 {} 段 · 近30天 {} 段",
            "{} in 7d · {} in 30d",
            "直近7日 {} 件 · 30日 {} 件",
            "7일 {}건 · 30일 {}건",
            "{} in 7d · {} in 30d",
            "{} in 7d · {} in 30d",
            "{} in 7d · {} in 30d",
        ],
    },
];
/// 语言在 [`Msg::text`] 里的槽位；顺序与 [`LANGS`] 一致。
fn lang_index(lang: Lang) -> usize {
    match lang {
        Lang::ZhCn => 0,
        Lang::ZhTw => 1,
        Lang::En => 2,
        Lang::Ja => 3,
        Lang::Ko => 4,
        Lang::Es => 5,
        Lang::Fr => 6,
        Lang::De => 7,
    }
}

fn find_entry<'a>(entries: &'a [Msg], key: &str) -> Option<&'a Msg> {
    entries.iter().find(|entry| entry.key == key)
}

/// Translate a catalog key into the chosen language, falling back to the
/// `zh-CN` source of truth for any key whose requested locale is untranslated,
/// and finally to the bare key when the key is not present at all.
pub fn tr<'a, L: IntoLang>(entries: &'a [Msg], lang: L, key: &'a str) -> &'a str {
    let lang = lang.into_lang();
    match find_entry(entries, key) {
        Some(entry) => {
            let value = entry.text[lang_index(lang)];
            if value.is_empty() && lang != Lang::ZhCn {
                entry.text[0]
            } else if value.is_empty() {
                // zh-CN is the source of truth and should never be empty.
                key
            } else {
                value
            }
        }
        None => key,
    }
}

/// Translate and substitute `{}` (sequential) and `{n}` (positional)
/// placeholders. Reuses the same fallback rules as [`tr`].
pub fn fmt<L: IntoLang>(entries: &[Msg], lang: L, key: &str, args: &[&dyn Display]) -> String {
    let template = tr(entries, lang, key);
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut auto_index = 0usize;
    while let Some(open) = rest.find('{') {
        if let Some(relative_close) = rest[open + 1..].find('}') {
            let close = open + 1 + relative_close;
            let field = &rest[open + 1..close];
            let argument = if field.is_empty() {
                let index = auto_index;
                auto_index += 1;
                index
            } else if let Ok(index) = field.parse::<usize>() {
                index
            } else {
                // Not a placeholder we understand (e.g. `{name}`): keep it.
                out.push_str(&rest[..open]);
                out.push('{');
                out.push_str(field);
                out.push('}');
                rest = &rest[close + 1..];
                continue;
            };
            out.push_str(&rest[..open]);
            if let Some(value) = args.get(argument) {
                out.push_str(&value.to_string());
            }
            rest = &rest[close + 1..];
            continue;
        }
        // No closing brace: keep the `{` literally and make progress.
        out.push_str(&rest[..open]);
        out.push('{');
        rest = &rest[open + 1..];
    }
    out.push_str(rest);
    out
}

/// Global lookup against [`CATALOG`].
pub fn tr_catalog<L: IntoLang>(lang: L, key: &'static str) -> &'static str {
    tr(CATALOG, lang, key)
}

/// Localize provider guidance at display time; stored validation codes stay unchanged.
pub fn provider_error_text(lang: Lang, message: &str) -> &str {
    if message == "llmThinkingRequired" || message.ends_with(": llmThinkingRequired") {
        return tr_catalog(lang, "settings.providers.llmThinkingRequired");
    }
    // 钥匙串相关的两条错误在 core 里以英文常量形式返回，界面上要按语言显示。
    if message == crate::linux_access::VAULT_LOCKED_MESSAGE {
        return tr_catalog(lang, "linuxAccess.keyringLocked");
    }
    if message == crate::linux_access::VAULT_UNLOCK_CANCELLED_MESSAGE {
        return tr_catalog(lang, "linuxAccess.keyringUnlockCancelled");
    }
    message
}

/// Global formatted lookup against [`CATALOG`].
pub fn fmt_catalog<L: IntoLang>(lang: L, key: &str, args: &[&dyn Display]) -> String {
    fmt(CATALOG, lang, key, args)
}

/// Ergonomic conversion for the call sites that already hold a concrete
/// [`Lang`] or a [`LocalePref`]. Kept tiny so UI code stays readable.
pub trait IntoLang {
    fn into_lang(self) -> Lang;
}

impl IntoLang for Lang {
    fn into_lang(self) -> Lang {
        self
    }
}

impl IntoLang for LocalePref {
    fn into_lang(self) -> Lang {
        self.resolve()
    }
}

impl IntoLang for &LocalePref {
    fn into_lang(self) -> Lang {
        self.resolve()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CATALOG: &[Msg] = &[
            Msg {
        key: "k.full",
        text: row8("完整", "完整", "Full", "完全", "전체", "Full", "Full", "Full"),
    },
        // `k.missing_en` deliberately leaves `en` empty to exercise fallback.
            Msg {
        key: "k.missing_en",
        text: row8("来源", "來源", "", "ソース", "출처", "", "", ""),
    },
    ];

    #[test]
    fn fallback_uses_zh_cn_source_of_truth_when_locale_is_empty() {
        assert_eq!(tr(TEST_CATALOG, Lang::En, "k.missing_en"), "来源");
        assert_eq!(tr(TEST_CATALOG, Lang::ZhCn, "k.missing_en"), "来源");
        // Fully translated keys return their own locale.
        assert_eq!(tr(TEST_CATALOG, Lang::En, "k.full"), "Full");
    }

    #[test]
    fn thinking_required_provider_errors_are_localized_without_changing_other_errors() {
        for lang in LANGS {
            let expected = tr_catalog(lang, "settings.providers.llmThinkingRequired");
            assert_ne!(expected, "settings.providers.llmThinkingRequired");
            for message in [
                "llmThinkingRequired",
                "Provider: llmThinkingRequired",
                "LLM provider failed: parse error: llmThinkingRequired",
            ] {
                assert_eq!(provider_error_text(lang, message), expected);
            }
            for message in ["providerHttpStatus:401", "notllmThinkingRequired"] {
                assert_eq!(provider_error_text(lang, message), message);
            }
        }
    }

    #[test]
    fn unknown_key_returns_the_key_itself() {
        assert_eq!(tr(TEST_CATALOG, Lang::Ja, "k.unknown"), "k.unknown");
    }

    #[test]
    fn zh_cn_source_of_truth_is_fully_populated_and_complete() {
        for entry in CATALOG {
            assert!(
                !entry.text[0].is_empty(),
                "zh-CN must never be empty for key {}",
                entry.key
            );
        }
    }

    #[test]
    fn every_catalog_key_is_translated_in_all_supported_locales() {
        for entry in CATALOG {
            for lang in LANGS {
                assert!(
                    !entry.text[lang_index(lang)].is_empty(),
                    "{} is missing a translation for {}",
                    entry.key,
                    lang.tag()
                );
            }
        }
    }

    #[test]
    fn catalog_keys_are_unique() {
        for (i, left) in CATALOG.iter().enumerate() {
            for right in &CATALOG[i + 1..] {
                assert_ne!(left.key, right.key);
            }
        }
    }

    #[test]
    fn system_locale_detection_maps_common_locale_envs() {
        assert_eq!(Lang::parse("zh_CN.UTF-8"), Some(Lang::ZhCn));
        assert_eq!(Lang::parse("zh-Hant-TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::parse("zh_TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::parse("ja_JP.UTF-8"), Some(Lang::Ja));
        assert_eq!(Lang::parse("ko_KR"), Some(Lang::Ko));
        assert_eq!(Lang::parse("en_US"), Some(Lang::En));
        assert_eq!(Lang::parse("fr_FR"), Some(Lang::Fr));
        assert_eq!(Lang::parse("es_ES"), Some(Lang::Es));
        assert_eq!(Lang::parse("de_DE.UTF-8"), Some(Lang::De));
        assert_eq!(Lang::parse("ru_RU"), None);
        assert_eq!(Lang::parse(""), None);
    }

    #[test]
    fn system_preference_resolves_from_the_host_locale() {
        for (variable, value, expected) in [
            ("LC_ALL", "zh_TW.UTF-8", Lang::ZhTw),
            ("LC_MESSAGES", "ko_KR.UTF-8", Lang::Ko),
            ("LANG", "ja_JP", Lang::Ja),
        ] {
            // LocalePref::System must route through env-based detection.
            let prev = std::env::var(variable).ok();
            std::env::set_var(variable, value);
            assert_eq!(LocalePref::System.resolve(), expected);
            match prev {
                Some(value) => std::env::set_var(variable, value),
                None => std::env::remove_var(variable),
            }
        }
    }

    #[test]
    fn locale_preference_roundtrips_through_wire_tags() {
        assert_eq!(LocalePref::System, LocalePref::from_tag("system"));
        assert_eq!(
            LocalePref::Lang(Lang::ZhTw),
            LocalePref::from_tag(Lang::ZhTw.tag())
        );
        // Unknown tags degrade to System (follow OS), never to a wrong guess.
        assert_eq!(LocalePref::from_tag("xx_YY"), LocalePref::System);
    }

}

