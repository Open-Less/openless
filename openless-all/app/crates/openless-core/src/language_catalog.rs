//! Shared display names and provider language identifiers.
//!
//! A locale identifier is a routing hint, not a promise that the current provider, model or OS
//! supports recognition in that language. Provider availability is still checked at runtime.

use once_cell::sync::Lazy;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Language {
    code: String,
    native_name: String,
    asr_code: String,
    apple_locale: String,
    #[serde(default)]
    aliases: Vec<String>,
}

static LANGUAGES: Lazy<Vec<Language>> = Lazy::new(|| {
    serde_json::from_str(include_str!("../../../contract/language-catalog.json"))
        .expect("bundled language catalog is valid")
});

fn language(name: &str) -> Option<&'static Language> {
    let name = name.trim();
    LANGUAGES.iter().find(|language| {
        language.native_name == name
            || language.code.eq_ignore_ascii_case(name)
            || language
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

pub fn asr_language_code(native_name: &str) -> Option<String> {
    language(native_name).map(|language| language.asr_code.clone())
}

pub fn apple_speech_locale(native_name: &str) -> Option<String> {
    language(native_name).map(|language| language.apple_locale.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_names_route_to_iso_and_apple_identifiers() {
        for (name, code, locale) in [
            ("Nederlands", "nl", "nl-NL"),
            ("Polski", "pl", "pl-PL"),
            ("Türkçe", "tr", "tr-TR"),
            ("Bahasa Indonesia", "id", "id-ID"),
            ("Filipino", "tl", "fil-PH"),
            ("Norsk", "no", "nb-NO"),
            ("Українська", "uk", "uk-UA"),
            ("বাংলা", "bn", "bn-IN"),
            ("עברית", "he", "he-IL"),
        ] {
            assert_eq!(asr_language_code(name).as_deref(), Some(code));
            assert_eq!(
                crate::provider_rules::zenmux_language_code(name).as_deref(),
                Some(code)
            );
            assert_eq!(apple_speech_locale(name).as_deref(), Some(locale));
        }
    }

    #[test]
    fn legacy_chinese_names_and_unknown_values_keep_distinct_semantics() {
        assert_eq!(asr_language_code("繁體中文").as_deref(), Some("zh"));
        assert_eq!(apple_speech_locale("繁體中文").as_deref(), Some("zh-TW"));
        assert_eq!(apple_speech_locale("简体中文").as_deref(), Some("zh-CN"));
        assert_eq!(apple_speech_locale("zh-TW").as_deref(), Some("zh-TW"));
        assert_eq!(asr_language_code("unknown language"), None);
        assert_eq!(apple_speech_locale(""), None);
    }
}
