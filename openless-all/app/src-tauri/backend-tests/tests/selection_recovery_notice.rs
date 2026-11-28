#[path = "../../src/selection/recovery_notice.rs"]
mod recovery_notice;

#[test]
fn only_confirmed_long_originals_show_recovery_notice() {
    for character in ['a', '中', '😀'] {
        for length in [3999, 4000, 4001] {
            let original = character.to_string().repeat(length);
            for inserted in [false, true] {
                let message =
                    recovery_notice::original_recovery_message("zh-CN", &original, inserted);
                assert_eq!(message.is_some(), inserted && length > 4000);
            }
        }
    }
    let original = format!(" {} ", "😀".repeat(3999));
    assert!(recovery_notice::original_recovery_message("en", &original, true).is_some());
    assert!(recovery_notice::original_recovery_message("en", &original, false).is_none());
}

#[test]
fn unreadable_selection_has_a_retry_message_in_all_eight_locales() {
    let locales = ["zh-CN", "zh-TW", "en", "ja", "ko", "de", "fr", "es"];
    let messages: std::collections::HashSet<_> = locales.iter()
        .map(|locale| recovery_notice::no_selection_message(locale)).collect();
    assert_eq!(messages.len(), locales.len());
    assert!(messages.iter().all(|message| !message.trim().is_empty()));
    assert_eq!(recovery_notice::no_selection_message("unknown"), recovery_notice::no_selection_message("en"));
    assert!(recovery_notice::no_selection_message("zh-CN").contains("重试"));
}
