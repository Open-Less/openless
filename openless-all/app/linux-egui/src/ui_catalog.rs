//! Generated React 2.0 catalogs are shared with the native renderer. Keep the
//! same keys and fallback rules, without runtime network or JS dependencies.
use crate::Lang;
use std::collections::BTreeMap;
use std::sync::OnceLock;
type Catalog = BTreeMap<String, BTreeMap<String, String>>;
fn catalog() -> &'static Catalog {
    static DATA: OnceLock<Catalog> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/ui-locales.json"))
            .expect("generated UI catalog")
    })
}
thread_local! {static LANGUAGE:std::cell::Cell<Lang>=const{std::cell::Cell::new(Lang::En)};}
pub fn set_language(lang: Lang) {
    LANGUAGE.with(|value| value.set(lang));
}
pub fn key<'a>(key: &'a str) -> &'a str {
    LANGUAGE.with(|lang| {
        catalog()
            .get(lang.get().tag())
            .and_then(|rows| rows.get(key))
            .or_else(|| catalog().get("zh-CN").and_then(|rows| rows.get(key)))
            .map(String::as_str)
            .unwrap_or(key)
    })
}
pub fn source<'a>(source: &'a str) -> &'a str {
    LANGUAGE.with(|lang| translate_source(lang.get(), source))
}
pub fn translate_source<'a>(lang: Lang, source: &'a str) -> &'a str {
    if lang == Lang::ZhCn {
        return source;
    }
    let rows = catalog();
    rows.get("zh-CN")
        .and_then(|zh| zh.iter().find(|(_, value)| value.as_str() == source))
        .and_then(|(key, _)| rows.get(lang.tag()).and_then(|values| values.get(key)))
        .map(String::as_str)
        .unwrap_or(source)
}
