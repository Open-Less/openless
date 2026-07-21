//! Phase 1 的选区润色工作流。
//!
//! 本模块刻意不处理热键或焦点恢复；它在流程边界发出无焦点 capsule 状态，完成
//! “捕获 -> 润色 -> 安全插入”。Windows 在云端等待期间会重新校验原始窗口、焦点控件
//! 和选区文本，避免把结果粘贴到用户后来切换到的应用或控件中。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use super::{
    emit_selection_polish_capsule, enabled_phrases, polish_text, raw_style_pack_uses_llm,
    schedule_selection_polish_capsule_idle, Coordinator, Inner, CAPSULE_AUTO_HIDE_DELAY_MS,
};
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    selection::{SelectionContext, SelectionInsertionTarget},
    types::{CapsuleState, DictationSession, InsertStatus, PolishMode, SelectionPolishOutputMode},
};

/// 所有 Coordinator 实例共享的串行保护，避免同一选区被并发润色并重复插入。
static SELECTION_POLISH_BUSY: AtomicBool = AtomicBool::new(false);

/// 预览窗可安全读取的内容；原窗口句柄不离开 Rust 后端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionPolishPreviewPayload {
    pub text: String,
    pub source_text: String,
}

/// 待用户确认的选区润色任务。它只存在于内存，取消或确认后立即清除。
#[derive(Debug, Clone)]
pub(crate) struct PendingSelectionPolishPreview {
    insertion_target: SelectionInsertionTarget,
    source_text: String,
    polished_text: String,
    source_app: Option<String>,
    mode: PolishMode,
    style_pack_id: String,
    llm_provider: Option<String>,
    llm_model: Option<String>,
    polish_ms: Option<u64>,
    started_at: std::time::Instant,
}

impl PendingSelectionPolishPreview {
    fn payload(&self) -> SelectionPolishPreviewPayload {
        SelectionPolishPreviewPayload {
            text: self.polished_text.clone(),
            source_text: self.source_text.clone(),
        }
    }
}

/// 从选区捕获结果得出的下一步动作。保持为纯逻辑，便于覆盖 provider / 插入边界。
#[derive(Debug, PartialEq, Eq)]
enum SelectionPolishPlan {
    NoSelection,
    Polish,
}

fn selection_polish_plan(selection: Option<&SelectionContext>) -> SelectionPolishPlan {
    match selection {
        Some(_) => SelectionPolishPlan::Polish,
        None => SelectionPolishPlan::NoSelection,
    }
}

/// provider 成功后的插入判定。
///
/// `Ok(None)` 表示模型只返回了空白，调用方必须保持原选区不变，不能触发插入。
fn insertion_text_from_provider_result(
    result: Result<String, String>,
) -> Result<Option<String>, String> {
    result.map(|text| (!text.trim().is_empty()).then_some(text))
}

/// 一个调用范围内的 busy 标记；无论 provider 或插入失败，析构时都会解除标记。
struct SelectionPolishBusyGuard<'a> {
    busy: &'a AtomicBool,
}

impl<'a> SelectionPolishBusyGuard<'a> {
    fn try_acquire(busy: &'a AtomicBool) -> Option<Self> {
        (!busy.swap(true, Ordering::AcqRel)).then_some(Self { busy })
    }
}

impl Drop for SelectionPolishBusyGuard<'_> {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::Release);
    }
}

/// 面向 capsule 的短提示必须是稳定、无敏感信息的用户文案。底层 provider 错误仍写入
/// 日志并作为调用结果返回，但不会把 endpoint / 认证等实现细节浮到用户正在输入的应用上。
fn selection_polish_feedback_message(code: &str) -> &'static str {
    match code {
        "selectionPolishNoSelection" => "未选中内容",
        "selectionPolishEmptyOutput" => "未生成可替换文本",
        "selectionPolishInsertFailed" => "替换失败，请重试",
        "selectionPolishTargetUnavailable" => "目标输入框不可用，请重新选择",
        "selectionPolishTargetChanged" | "selectionPolishSelectionChanged" => "选区已变化，未替换",
        _ => "润色失败，请重试",
    }
}

fn selection_polish_success_message(
    status: InsertStatus,
    output_mode: SelectionPolishOutputMode,
) -> &'static str {
    if output_mode == SelectionPolishOutputMode::PreviewConfirm {
        return "已打开预览，等待确认";
    }
    match status {
        InsertStatus::Inserted | InsertStatus::PasteSent => "已替换",
        InsertStatus::CopiedFallback => "已复制结果，请手动粘贴",
        InsertStatus::Failed => "替换失败，请重试",
    }
}

/// 展示终态并让它在短暂停留后自动收起。收起回调带着 emit 代数，旧会话永远不会
/// 覆盖后面开始的 selection、语音或 QA 胶囊。
fn finish_selection_polish_capsule(
    inner: &Arc<Inner>,
    state: CapsuleState,
    message: impl Into<String>,
) {
    let event_epoch = emit_selection_polish_capsule(inner, state, message);
    schedule_selection_polish_capsule_idle(inner, event_epoch, CAPSULE_AUTO_HIDE_DELAY_MS);
}

pub(super) async fn run_selection_polish(inner: &Arc<Inner>) -> Result<(), String> {
    let _busy_guard = SelectionPolishBusyGuard::try_acquire(&SELECTION_POLISH_BUSY)
        .ok_or_else(|| "selectionPolishBusy".to_string())?;
    let started_at = std::time::Instant::now();

    // Must happen before copy/capture and before any async provider work.  The
    // final check below deliberately does *not* restore this target: a user who
    // changed windows made an intentional context switch, so the safe behavior
    // is to leave both apps untouched.
    let insertion_target = crate::selection::capture_selection_insertion_target();
    let capture = crate::selection::capture_selection_with_status();
    if selection_polish_plan(capture.selection.as_ref()) == SelectionPolishPlan::NoSelection {
        let code = capture
            .warning_code
            .unwrap_or("selectionPolishNoSelection")
            .to_string();
        finish_selection_polish_capsule(
            inner,
            CapsuleState::Cancelled,
            selection_polish_feedback_message(&code),
        );
        return Err(code);
    }
    let selection = capture.selection.expect("selection plan checked above");
    if !crate::selection::selection_insertion_target_is_captured(&insertion_target) {
        let code = "selectionPolishTargetUnavailable";
        finish_selection_polish_capsule(
            inner,
            CapsuleState::Error,
            selection_polish_feedback_message(code),
        );
        return Err(code.to_string());
    }

    // 选区已成功读取后才开始显示处理中，避免无选区时先闪过加载动画再显示提示。
    emit_selection_polish_capsule(inner, CapsuleState::Polishing, "正在润色...");

    let hotwords = enabled_phrases(inner);
    let prefs = inner.prefs.get();
    let pack = match inner
        .style_packs
        .get_or_default_active(&prefs.selection_polish_style_pack_id)
    {
        Ok(pack) => pack,
        Err(error) => {
            log::warn!("[selection-polish] load active style pack failed: {error}");
            finish_selection_polish_capsule(
                inner,
                CapsuleState::Error,
                selection_polish_feedback_message("selectionPolishStylePackFailed"),
            );
            return Err(error.to_string());
        }
    };
    let effective_mode = pack.base_mode;
    let raw_text = selection.text;
    let source_app = selection.source_app;
    let mut llm_call = None;
    let mut polish_ms = None;

    // 与 `repolish` 同样读取当前 style pack、词表和语言偏好；但前台上下文必须
    // 来自选区捕获时的源应用，避免在 provider 等待期间重新读取/校验目标窗口。
    // 选区润色只读取风格包的书面文本 Prompt；旧包缺少该字段时回退为安全默认。
    let selection_style_prompt = crate::types::style_pack_prompt(
        &pack,
        crate::types::StylePromptKind::Selection,
    );
    log::info!(
        "[style-pack] runtime dispatch scope=selection pack={} kind={:?} mode={:?} prompt_chars={}",
        pack.id,
        pack.kind,
        effective_mode,
        selection_style_prompt.chars().count()
    );
    let provider_result = if effective_mode == PolishMode::Raw && !raw_style_pack_uses_llm(&pack) {
        Ok(raw_text.clone())
    } else {
        polish_text(
            &raw_text,
            effective_mode,
            &hotwords,
            &selection_style_prompt,
            &prefs.working_languages,
            prefs.chinese_script_preference,
            prefs.output_language_preference,
            prefs.llm_thinking_enabled,
            source_app.as_deref(),
            &[],
            &mut llm_call,
            &mut polish_ms,
        )
        .await
        .map_err(|error| error.to_string())
    };

    let text_to_insert = match insertion_text_from_provider_result(provider_result) {
        Ok(Some(text)) => text,
        Ok(None) => {
            let code = "selectionPolishEmptyOutput";
            finish_selection_polish_capsule(
                inner,
                CapsuleState::Error,
                selection_polish_feedback_message(code),
            );
            return Err(code.to_string());
        }
        Err(error) => {
            log::warn!("[selection-polish] provider failed: {error}");
            finish_selection_polish_capsule(
                inner,
                CapsuleState::Error,
                selection_polish_feedback_message("selectionPolishProviderFailed"),
            );
            return Err(error);
        }
    };

    let status = match prefs.selection_polish_output_mode {
        SelectionPolishOutputMode::DirectReplace => {
            let target_validation =
                crate::selection::validate_selection_insertion_target(&insertion_target, &raw_text);
            if let Some(error_code) = target_validation.error_code() {
                log::info!(
                    "[selection-polish] skipped insertion because the captured target or selection changed ({error_code})"
                );
                finish_selection_polish_capsule(
                    inner,
                    CapsuleState::Cancelled,
                    selection_polish_feedback_message(error_code),
                );
                return Err(error_code.to_string());
            }
            inner.inserter.insert(
                &text_to_insert,
                prefs.restore_clipboard_after_paste,
                prefs.paste_shortcut,
            )
        }
        SelectionPolishOutputMode::PreviewConfirm => {
            let (llm_provider, llm_model) = match llm_call.as_ref() {
                Some(label) => (Some(label.provider.clone()), Some(label.model.clone())),
                None => (None, None),
            };
            *inner.selection_polish_preview.lock() = Some(PendingSelectionPolishPreview {
                insertion_target,
                source_text: raw_text,
                polished_text: text_to_insert,
                source_app,
                mode: effective_mode,
                style_pack_id: pack.id,
                llm_provider,
                llm_model,
                polish_ms,
                started_at,
            });
            if let Some(app) = inner.app.lock().clone() {
                crate::show_selection_polish_preview(&app);
            }
            finish_selection_polish_capsule(
                inner,
                CapsuleState::Done,
                selection_polish_success_message(InsertStatus::Inserted, prefs.selection_polish_output_mode),
            );
            return Ok(());
        }
    };
    let dictionary_entry_count = if status != InsertStatus::Failed {
        match inner.vocab.record_hits(&text_to_insert) {
            Ok(hits) => Some(hits.min(u32::MAX as u64) as u32),
            Err(error) => {
                log::error!("[selection-polish] record vocabulary hits failed: {error}");
                Some(0)
            }
        }
    } else {
        Some(0)
    };
    let (llm_provider, llm_model) = match llm_call {
        Some(label) => (Some(label.provider), Some(label.model)),
        None => (None, None),
    };
    let raw_chars = raw_text.chars().count();
    let session = DictationSession {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        source: crate::types::HistorySource::SelectionPolish,
        raw_transcript: raw_text,
        final_text: text_to_insert.clone(),
        mode: effective_mode,
        style_pack_id: Some(pack.id.clone()),
        translation_active: false,
        polish_source: None,
        app_bundle_id: None,
        app_name: source_app,
        insert_status: status,
        error_code: (status == InsertStatus::Failed)
            .then_some("selectionPolishInsertFailed".into()),
        duration_ms: Some(started_at.elapsed().as_millis() as u64),
        dictionary_entry_count,
        has_audio_recording: None,
        asr_provider: None,
        asr_model: None,
        llm_provider,
        llm_model,
        asr_ms: None,
        polish_ms,
    };
    if let Err(error) = inner.history.append_with_retention(
        session,
        prefs.history_retention_days,
        prefs.history_max_entries,
    ) {
        log::error!("[selection-polish] history append failed: {error}");
    }
    if status == InsertStatus::Failed {
        let code = "selectionPolishInsertFailed";
        finish_selection_polish_capsule(
            inner,
            CapsuleState::Error,
            selection_polish_feedback_message(code),
        );
        return Err(code.to_string());
    }
    log::info!(
        "[selection-polish] completed raw_chars={} polished_chars={} insert_status={status:?}",
        raw_chars,
        text_to_insert.chars().count(),
    );
    finish_selection_polish_capsule(
        inner,
        CapsuleState::Done,
        selection_polish_success_message(status, prefs.selection_polish_output_mode),
    );
    Ok(())
}

impl Coordinator {
    /// 正式热键与开发调试入口共享同一条选区润色工作流。
    pub async fn trigger_selection_polish(&self) -> Result<(), String> {
        run_selection_polish(&self.inner).await
    }

    /// Development-only IPC 的 Coordinator 入口。
    pub async fn trigger_selection_polish_for_dev(&self) -> Result<(), String> {
        self.trigger_selection_polish().await
    }

    pub(crate) fn selection_polish_preview(&self) -> Option<SelectionPolishPreviewPayload> {
        self.inner
            .selection_polish_preview
            .lock()
            .as_ref()
            .map(PendingSelectionPolishPreview::payload)
    }

    pub(crate) fn cancel_selection_polish_preview(&self) {
        self.inner.selection_polish_preview.lock().take();
        if let Some(app) = self.inner.app.lock().clone() {
            crate::hide_selection_polish_preview(&app);
        }
    }

    pub(crate) fn confirm_selection_polish_preview(&self, text: String) -> Result<(), String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("selectionPolishEmptyOutput".into());
        }
        let preview = self
            .inner
            .selection_polish_preview
            .lock()
            .take()
            .ok_or_else(|| "selectionPolishPreviewUnavailable".to_string())?;

        if !crate::selection::reactivate_selection_insertion_target(&preview.insertion_target) {
            return Err("selectionPolishTargetUnavailable".into());
        }
        let validation = crate::selection::validate_selection_insertion_target(
            &preview.insertion_target,
            &preview.source_text,
        );
        if let Some(code) = validation.error_code() {
            return Err(code.to_string());
        }

        let prefs = self.inner.prefs.get();
        let status = self.inner.inserter.insert(
            &text,
            prefs.restore_clipboard_after_paste,
            prefs.paste_shortcut,
        );
        if status == InsertStatus::Failed {
            return Err("selectionPolishInsertFailed".into());
        }
        let dictionary_entry_count = self
            .inner
            .vocab
            .record_hits(&text)
            .map(|hits| Some(hits.min(u32::MAX as u64) as u32))
            .unwrap_or_else(|error| {
                log::error!("[selection-polish] record vocabulary hits failed: {error}");
                Some(0)
            });
        let session = DictationSession {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now().to_rfc3339(),
            source: crate::types::HistorySource::SelectionPolish,
            raw_transcript: preview.source_text,
            final_text: text.clone(),
            mode: preview.mode,
            style_pack_id: Some(preview.style_pack_id),
            translation_active: false,
            polish_source: None,
            app_bundle_id: None,
            app_name: preview.source_app,
            insert_status: status,
            error_code: None,
            duration_ms: Some(preview.started_at.elapsed().as_millis() as u64),
            dictionary_entry_count,
            has_audio_recording: None,
            asr_provider: None,
            asr_model: None,
            llm_provider: preview.llm_provider,
            llm_model: preview.llm_model,
            asr_ms: None,
            polish_ms: preview.polish_ms,
        };
        if let Err(error) = self.inner.history.append_with_retention(
            session,
            prefs.history_retention_days,
            prefs.history_max_entries,
        ) {
            log::error!("[selection-polish] history append failed: {error}");
        }
        if let Some(app) = self.inner.app.lock().clone() {
            crate::hide_selection_polish_preview(&app);
        }
        finish_selection_polish_capsule(&self.inner, CapsuleState::Done, "已替换");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    #[test]
    fn no_selection_does_not_schedule_provider_work() {
        assert_eq!(
            selection_polish_plan(None),
            SelectionPolishPlan::NoSelection
        );
    }

    #[test]
    fn provider_failure_does_not_produce_insertable_text() {
        assert_eq!(
            insertion_text_from_provider_result(Err("provider unavailable".to_string())),
            Err("provider unavailable".to_string())
        );
    }

    #[test]
    fn empty_provider_output_does_not_produce_insertable_text() {
        assert_eq!(
            insertion_text_from_provider_result(Ok(" \n\t ".to_string())),
            Ok(None)
        );
    }

    #[test]
    fn busy_guard_rejects_overlap_and_releases_after_scope() {
        let busy = AtomicBool::new(false);
        let guard = SelectionPolishBusyGuard::try_acquire(&busy).expect("first run acquires");
        assert!(SelectionPolishBusyGuard::try_acquire(&busy).is_none());
        drop(guard);
        assert!(SelectionPolishBusyGuard::try_acquire(&busy).is_some());
    }

    #[test]
    fn feedback_messages_are_safe_and_specific_for_known_outcomes() {
        assert_eq!(
            selection_polish_feedback_message("selectionPolishNoSelection"),
            "未选中内容"
        );
        assert_eq!(
            selection_polish_feedback_message("selectionPolishInsertFailed"),
            "替换失败，请重试"
        );
        assert_eq!(
            selection_polish_feedback_message("selectionPolishTargetChanged"),
            "选区已变化，未替换"
        );
        assert_eq!(
            selection_polish_feedback_message("provider token invalid"),
            "润色失败，请重试"
        );
    }

    #[test]
    fn copied_fallback_is_not_reported_as_a_completed_replacement() {
        assert_eq!(
            selection_polish_success_message(
                InsertStatus::CopiedFallback,
                SelectionPolishOutputMode::DirectReplace,
            ),
            "已复制结果，请手动粘贴"
        );
        assert_eq!(
            selection_polish_success_message(
                InsertStatus::Inserted,
                SelectionPolishOutputMode::DirectReplace,
            ),
            "已替换"
        );
    }
}
