/// UIA character units can contain control characters or multiple Unicode scalars.
/// A saturated read may be a truncated prefix, which is unsafe for the before-caret side.
pub(super) fn decode_bounded_utf16(units: &[u16], cap: usize) -> Result<String, &'static str> {
    if units.len() >= cap {
        return Err("caret range text may have been truncated");
    }
    String::from_utf16(units).map_err(|_| "UIA text contains invalid UTF-16")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_units_need_not_equal_unicode_scalar_count() {
        for text in ["a\rb", "a\r\nb", "a\u{200e}b", "e\u{301}", "🙂👩\u{200d}💻"] {
            assert_eq!(
                decode_bounded_utf16(&text.encode_utf16().collect::<Vec<_>>(), 64).unwrap(),
                text
            );
        }
    }

    #[test]
    fn invalid_utf16_is_rejected() {
        assert!(decode_bounded_utf16(&[0xD800], 64).is_err());
        assert!(decode_bounded_utf16(&[0xDC00], 64).is_err());
    }

    #[test]
    fn saturated_or_overlong_text_is_rejected() {
        assert!(decode_bounded_utf16(&[65; 8], 8).is_err());
        assert!(decode_bounded_utf16(&[65; 9], 8).is_err());
        assert_eq!(decode_bounded_utf16(&[65; 7], 8).unwrap(), "AAAAAAA");
        assert!(decode_bounded_utf16(&[], 0).is_err());
        assert_eq!(decode_bounded_utf16(&[], 8).unwrap(), "");
    }
}
