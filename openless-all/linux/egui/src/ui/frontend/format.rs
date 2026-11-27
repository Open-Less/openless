//! Locale-aware value formatting shared by the frontend pages.
//!
//! Kept separate from the pages so the same numbers render identically in the
//! overview dashboard and the history detail panel.

use chrono::{Datelike, Timelike};
use openless_linux_egui::{fmt_l10n, Lang};

/// Recording length as shown on history rows and the `录音 …` label.
/// `—` for unknown/zero, seconds below a minute, minutes above.
pub fn history_duration(ms: Option<u64>, lang: Lang) -> String {
    let Some(ms) = ms else {
        return "—".to_string();
    };
    if ms == 0 {
        return "—".to_string();
    }
    let seconds = ms as f64 / 1000.0;
    if seconds < 60.0 {
        return fmt_l10n(lang, "dur.sec", &[&format!("{seconds:.1}")]);
    }
    fmt_l10n(
        lang,
        "common.duration_minutes",
        &[&format!("{:.1}", seconds / 60.0)],
    )
}

/// One pipeline step's duration. Sub-second steps stay in integer milliseconds
/// (streaming ASR tails are tens of ms, so 0.1s rounding would hide differences).
pub fn step_duration(ms: u64, lang: Lang) -> String {
    if ms < 1000 {
        return fmt_l10n(lang, "dur.ms", &[&ms]);
    }
    history_duration(Some(ms), lang)
}

/// RFC3339 history timestamp → compact local label.
///
/// Today renders as `HH:MM`, the current year as `M/D HH:MM`, older entries as
/// `Y/M/D HH:MM`, mirroring the Tauri `formatHistoryTime`.
pub fn time_label(created_at: &str) -> String {
    let Ok(instant) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return created_at.to_string();
    };
    let local = instant.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    if local.date_naive() == now.date_naive() {
        format!("{:02}:{:02}", local.hour(), local.minute())
    } else if local.year() == now.year() {
        format!(
            "{}/{} {:02}:{:02}",
            local.month(),
            local.day(),
            local.hour(),
            local.minute()
        )
    } else {
        format!(
            "{}/{}/{} {:02}:{:02}",
            local.year(),
            local.month(),
            local.day(),
            local.hour(),
            local.minute()
        )
    }
}
