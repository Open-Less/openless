//! ASR 诊断事件上报。
//!
//! 引擎实现（sherpa / foundry / apple）需要把内部阶段写进宿主可导出的诊断日志，
//! 但平台层不能反向依赖宿主的日志模块。这里提供钩子：宿主启动时注册一次，
//! 平台层的 [`voice_diagnostic`] 就转发过去；未注册时退化为 `log::debug!`。

use std::sync::OnceLock;

/// 诊断事件钩子：(probe, hypotheses, stage, event, payload)。
pub type DiagnosticHook =
    Box<dyn Fn(&str, &[&str], &str, &str, serde_json::Value) + Send + Sync>;

static HOOK: OnceLock<DiagnosticHook> = OnceLock::new();

/// 注册诊断钩子。宿主启动时调用一次；重复调用只有第一次生效。
pub fn install_diagnostic_hook(hook: DiagnosticHook) {
    let _ = HOOK.set(hook);
}

/// 上报一条 ASR 诊断事件。
pub fn voice_diagnostic(
    probe: &str,
    hypotheses: &[&str],
    stage: &str,
    event: &str,
    payload: serde_json::Value,
) {
    match HOOK.get() {
        Some(hook) => hook(probe, hypotheses, stage, event, payload),
        // 没有钩子时不要静默丢掉：至少进日志，方便纯平台层测试时观察。
        None => log::debug!(
            "[asr-diag] probe={probe} stage={stage} event={event} payload={payload}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_without_hook_do_not_panic() {
        voice_diagnostic("P2", &["H1"], "worker", "started", serde_json::json!({}));
    }
}
