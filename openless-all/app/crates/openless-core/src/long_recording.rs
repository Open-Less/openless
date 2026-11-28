//! Bounded recovery work for recordings longer than five minutes.

pub(crate) const THRESHOLD_MS: u64 = 5 * 60 * 1_000;
pub(crate) const RECOVERY_CHUNK_BYTES: usize = crate::DICTATION_SAMPLE_RATE as usize * 2 * 60;
pub(crate) const POLISH_CHUNK_CHARS: usize = 4_000;

pub(crate) fn duration_ms(bytes: u64) -> u64 {
    bytes.saturating_mul(1_000) / (u64::from(crate::DICTATION_SAMPLE_RATE) * 2)
}

/// Keep every original character, including whitespace at a chunk boundary.
/// Prefer sentence/paragraph boundaries without allowing an unbounded request.
pub(crate) fn text_chunks(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut boundary = None;
        let mut end = rest.len();
        for (index, (offset, character)) in rest.char_indices().enumerate() {
            if index == POLISH_CHUNK_CHARS {
                end = boundary.unwrap_or(offset);
                break;
            }
            if index >= POLISH_CHUNK_CHARS / 2
                && matches!(character, '\n' | '。' | '！' | '？' | '.' | '!' | '?')
            {
                boundary = Some(offset + character.len_utf8());
            }
        }
        chunks.push(&rest[..end]);
        rest = &rest[end..];
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_five_minutes_is_normal_and_half_hour_exceeds_the_threshold() {
        let per_second = u64::from(crate::DICTATION_SAMPLE_RATE) * 2;
        assert_eq!(duration_ms(300 * per_second), THRESHOLD_MS);
        assert!(duration_ms(300 * per_second + 32) > THRESHOLD_MS);
        assert_eq!(duration_ms(31 * 60 * per_second), 31 * 60 * 1_000);
    }

    #[test]
    fn unicode_and_boundary_whitespace_reassemble_without_a_missing_tail() {
        for text in [
            format!(
                "{}。\n  {}尾部唯一内容",
                "中文😀".repeat(4_000),
                "detail ".repeat(3_000)
            ),
            "x".repeat(20_001),
        ] {
            let chunks = text_chunks(&text);
            assert_eq!(chunks.concat(), text);
            assert!(chunks
                .iter()
                .all(|chunk| chunk.chars().count() <= POLISH_CHUNK_CHARS));
        }
    }
}
