package com.openless.app

/**
 * 语音 UI 的**纯投影**。
 *
 * 设计约束（重要）：
 * - 单向下行：只把宿主（Service）已有的 legacy 状态/switches 映射成可渲染快照，
 *   **不发送任何命令**，也不回写/修正业务 flags；
 * - 状态真实性：`recording`/`processing` 永远如实回显入参 flag，绝不根据字符串
 *   （如 "speaking"/"thinking"）虚构录音或处理中状态；
 * - 可取消性：Webhook 提交（cloudSubmitting）**不承诺可取消**（`canCancel=false`），
 *   但允许在其未完成时开始新的录音（`canStart=true`，与旧协议一致）；
 * - 无 Android SDK 依赖，可直接用 JVM JUnit 验证。
 *
 * 说明：`state` 仅用于把终态文案映射为 [ImeVoicePhase]，不参与任何业务动作。
 */
internal enum class ImeVoicePhase { IDLE, STARTING, RECORDING, PROCESSING, SUBMITTING, DONE, CANCELLED, ERROR }

internal enum class ImeVoiceMode { DICTATION, RAW, QUICK_NOTE, CLOUD_NOTE }

internal data class ImeVoiceUiSnapshot(
    val phase: ImeVoicePhase,
    val mode: ImeVoiceMode,
    val message: String,
    val audioLevel: Float,
    val recording: Boolean,
    val processing: Boolean,
    val editorLocked: Boolean,
    val canCancel: Boolean,
    val canStart: Boolean,
)

/** Capabilities only narrow from observed state; they never grant another entry's controls. */
internal fun ImeVoiceUiSnapshot.withEligibility(
    ownsRequest: Boolean,
    microphoneGranted: Boolean,
    sensitiveField: Boolean,
    foreignBusy: Boolean,
): ImeVoiceUiSnapshot = copy(
    canCancel = canCancel && ownsRequest,
    canStart = canStart && microphoneGranted && !sensitiveField && !foreignBusy,
)

internal object ImeVoiceUiProjection {

    /**
     * 从 legacy 字段投影出 UI 快照（纯函数）。
     *
     * 状态优先级（高 → 低）：recording（awaitingStart ? STARTING : RECORDING）
     * → processing → cloudSubmitting → legacy `state` 终态（done/cancelled/error）
     * → IDLE。`state` 中的 "speaking"/"thinking" 在没有真实 flag 时按 IDLE 处理。
     *
     * 模式优先级（与现有颜色/强调优先级一致）：RAW > QUICK_NOTE > CLOUD_NOTE > DICTATION。
     */
    fun fromLegacy(
        state: String,
        recording: Boolean,
        processing: Boolean,
        cloudSubmitting: Boolean,
        raw: Boolean,
        quickNote: Boolean,
        cloud: Boolean,
        message: String,
        level: Float = 0f,
        awaitingStart: Boolean = false,
    ): ImeVoiceUiSnapshot {
        val phase = when {
            recording -> if (awaitingStart) ImeVoicePhase.STARTING else ImeVoicePhase.RECORDING
            processing -> ImeVoicePhase.PROCESSING
            cloudSubmitting -> ImeVoicePhase.SUBMITTING
            else -> terminalPhase(state)
        }
        val mode = when {
            raw -> ImeVoiceMode.RAW
            quickNote -> ImeVoiceMode.QUICK_NOTE
            cloud || cloudSubmitting -> ImeVoiceMode.CLOUD_NOTE
            else -> ImeVoiceMode.DICTATION
        }
        // 只有真实录音时才展示电平；NaN/Inf/越界一律收敛到 [0,1]，其余状态归 0。
        val audioLevel = if (recording && level.isFinite()) level.coerceIn(0f, 1f) else 0f
        return ImeVoiceUiSnapshot(
            phase = phase,
            mode = mode,
            message = message,
            audioLevel = audioLevel,
            recording = recording,
            processing = processing,
            editorLocked = recording || processing || cloudSubmitting,
            canCancel = recording || processing,
            canStart = !recording && !processing,
        )
    }

    /** 只在没有 recording/processing/cloudSubmitting 真实 flag 时使用。 */
    private fun terminalPhase(state: String): ImeVoicePhase = when (state.trim().lowercase()) {
        "done" -> ImeVoicePhase.DONE
        "cancelled", "canceled" -> ImeVoicePhase.CANCELLED
        "error" -> ImeVoicePhase.ERROR
        else -> ImeVoicePhase.IDLE
    }
}

/**
 * 一次语音请求的所有权凭证。
 *
 * 所有权**不会**因停止采集而自动释放：UI 适配层应保存本对象，直到处理结束、
 * 取消或输入目标失效，再用 [matches] 拒绝迟到/串场的回调。
 */
internal data class ImeRequestOwner(val epoch: Long, val requestId: Long) {
    fun matches(epoch: Long, requestId: Long): Boolean =
        this.epoch == epoch && this.requestId == requestId
}
