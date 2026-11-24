//! macOS 本地化名称查询。
//!
//! 需要与 WebView 的 `Intl.DisplayNames` 完全一致的 CLDR "standard" 名称时，
//! 只能问 Foundation：`[[NSLocale localeWithLocaleIdentifier:locale]
//!   localizedStringForLocaleIdentifier:code]`。

/// 用 `locale`（BCP-47）显示 `code` 的本地化名称。
///
/// 失败或空字符串返回 `None`，调用方回退到语言代码本身。
pub fn localized_language_name(code: &str, locale: &str) -> Option<String> {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    let class = AnyClass::get("NSLocale")?;
    let (identifier, code) = (NSString::from_str(locale), NSString::from_str(code));
    objc2::rc::autoreleasepool(|_| unsafe {
        // SAFETY: 两个 selector 都接收 NSString 并返回 autoreleased 对象；
        // 我们在 pool 结束前把它拷成 Rust String。
        let ns_locale: *mut AnyObject =
            objc2::msg_send![class, localeWithLocaleIdentifier: &*identifier];
        if ns_locale.is_null() {
            return None;
        }
        let name: *mut NSString =
            objc2::msg_send![ns_locale, localizedStringForLocaleIdentifier: &*code];
        name.as_ref()
            .map(|name| name.to_string())
            .filter(|name| !name.is_empty())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn unknown_locale_does_not_panic() {
        // 只验证调用路径；返回值依赖系统 CLDR 数据。
        let _ = super::localized_language_name("en", "zh-Hans");
    }
}
