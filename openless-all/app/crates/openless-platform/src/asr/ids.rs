//! 本地 ASR 的 provider id 与判定工具。
//!
//! 这些 id 是**对外契约**（设置页、云同步、诊断日志都引用），不属于任何平台实现，
//! 所以集中在这里，避免宿主与各引擎实现各写一份字符串。

/// 已退役的本地 Qwen3 通道（仅用于 UI 下线与清理）。
pub const PROVIDER_ID: &str = "local-qwen3";
pub const LOCAL_QWEN3_MLX_PROVIDER_ID: &str = "local-qwen3-mlx";
pub const LOCAL_QWEN3_C_PROVIDER_ID: &str = "local-qwen3-c";
pub const LOCAL_WHISPER_PROVIDER_ID: &str = "local-whisper";
pub const APPLE_SPEECH_PROVIDER_ID: &str = "apple-speech";

/// 本地 Whisper。
pub fn is_local_whisper(id: &str) -> bool {
    id == LOCAL_WHISPER_PROVIDER_ID
}

/// 已退役的本地 Qwen3 系列。
pub fn is_local_qwen3(id: &str) -> bool {
    matches!(
        id,
        PROVIDER_ID | LOCAL_QWEN3_MLX_PROVIDER_ID | LOCAL_QWEN3_C_PROVIDER_ID
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitepser_and_qwen3_ids_are_distinct() {
        assert!(is_local_whisper("local-whisper"));
        assert!(!is_local_whisper("local-qwen3"));
        assert!(is_local_qwen3("local-qwen3"));
        assert!(is_local_qwen3("local-qwen3-mlx"));
        assert!(is_local_qwen3("local-qwen3-c"));
        assert!(!is_local_qwen3("sherpa-onnx-local"));
    }
}
