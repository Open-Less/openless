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
