pub const PASTE_RESULT_SUCCESS: &str = "SUCCESS";
pub const PASTE_RESULT_SHIZUKU_UNAVAILABLE: &str = "SHIZUKU_UNAVAILABLE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TieredInsertOutcome {
    Inserted,
    ClipboardFallback,
}

pub fn resolve_tiered_insert_status(
    accessibility_result: Option<&str>,
    shizuku_result: Option<&str>,
) -> TieredInsertOutcome {
    if accessibility_result == Some(PASTE_RESULT_SUCCESS) {
        return TieredInsertOutcome::Inserted;
    }

    if shizuku_result == Some(PASTE_RESULT_SUCCESS) {
        return TieredInsertOutcome::Inserted;
    }

    TieredInsertOutcome::ClipboardFallback
}

#[cfg(test)]
mod tests {
    use super::{resolve_tiered_insert_status, TieredInsertOutcome, PASTE_RESULT_SUCCESS};

    #[test]
    fn tier1_success_skips_lower_tiers() {
        assert_eq!(
            resolve_tiered_insert_status(Some("SUCCESS"), Some("INJECT_FAILED")),
            TieredInsertOutcome::Inserted,
        );
    }

    #[test]
    fn tier1_failure_falls_through_to_tier2_before_clipboard() {
        assert_eq!(
            resolve_tiered_insert_status(Some("NO_FOCUSED_EDITOR"), Some("SUCCESS")),
            TieredInsertOutcome::Inserted,
        );
        assert_eq!(
            resolve_tiered_insert_status(None, Some("SUCCESS")),
            TieredInsertOutcome::Inserted,
        );
    }

    #[test]
    fn both_tiers_fail_before_clipboard_fallback() {
        assert_eq!(
            resolve_tiered_insert_status(Some("NO_FOCUSED_EDITOR"), Some("INJECT_FAILED")),
            TieredInsertOutcome::ClipboardFallback,
        );
        assert_eq!(
            resolve_tiered_insert_status(Some("PASTE_REJECTED"), Some("SHIZUKU_UNAVAILABLE")),
            TieredInsertOutcome::ClipboardFallback,
        );
        assert_eq!(
            resolve_tiered_insert_status(None, Some("SHIZUKU_UNAVAILABLE")),
            TieredInsertOutcome::ClipboardFallback,
        );
    }
}
