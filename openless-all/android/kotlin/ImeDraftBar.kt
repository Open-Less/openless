package com.openless.app

import android.content.Context
import android.graphics.drawable.GradientDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.ViewGroup
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView

/**
 * 草稿编辑栏（纯展示/交互外壳）。
 *
 * 内容、选区与提交逻辑全部归宿主（Service/DraftConnection）：本栏**不创建** editText、
 * **不 setText**、不改选区、不读写偏好、不碰 JNI/InputConnection/网络。
 * [editor] 由宿主传入并在重挂（模式/主题重建）时原样换父节点，因此文本与选区自然保留。
 *
 * 交互约束：
 * - confirm/cancel 为普通 [TextView] + 原生 `setOnClickListener`，不依赖共享 ActionButton
 *   正在演进的 interaction-policy 参数；
 * - `submitting=true` 时两个按钮真正 `isEnabled=false`，并取消挂起触摸，点击不会回调；
 * - 每次有效点击先回调 [onFeedback] 一次（触感由宿主决定），再回调业务；
 * - 编辑框只拿 View focus（`requestFocus`），绝不请求窗口焦点、不调用 showSoftInput；
 *   聚焦 post 会校验"editor 仍属于本栏且已附着窗口"，旧栏 detach 后不能抢新栏焦点；
 * - 控件统一走 [ImePalette]，错误态用 `palette.error` 且写入可读 contentDescription。
 *
 * 尺寸：整体 wrap_content，尊重父布局的 AT_MOST 约束；过高的草稿由宿主外层 ScrollView
 * 限高，本栏不主动占屏。
 */
internal class ImeDraftBar(
    context: Context,
    private val palette: ImePalette,
    private val editor: EditText,
    private val onCancel: () -> Unit,
    private val onConfirm: () -> Unit,
    private val onFeedback: () -> Unit = {},
) : LinearLayout(context) {

    val status: TextView = TextView(context)
    val cancel: TextView = TextView(context)
    val confirm: TextView = TextView(context)

    private var submitting = false
    private var focusToken = 0
    private var pendingFocus: Runnable? = null

    init {
        orientation = VERTICAL
        setPadding(dp(16), dp(10), dp(16), dp(8))
        setBackgroundColor(palette.surface)
        // Swallow stray touches in the bar so they cannot reach the app underneath.
        isClickable = true

        // Re-attach the host-owned editor without creating a second instance; its
        // text/selection belong to the host and are never touched here.
        (editor.parent as? ViewGroup)?.removeView(editor)
        styleEditor(editor)
        addView(
            editor,
            LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT),
        )

        stylePill(cancel, fill = palette.auxiliary, textColor = palette.text)
        // Fixed default labels; the host may relabel through the public `cancel`/`confirm`
        // fields when it rebuilds for its own locale, without the bar reading preferences.
        cancel.text = "取消"
        cancel.contentDescription = "取消草稿"

        status.apply {
            textSize = 13f
            gravity = Gravity.CENTER
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            setTextColor(palette.secondaryText)
        }

        stylePill(confirm, fill = palette.accent, textColor = palette.surface)
        confirm.text = "确认"
        confirm.contentDescription = "确认草稿"

        cancel.setOnClickListener { onCancelClicked() }
        confirm.setOnClickListener { onConfirmClicked() }

        val row = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
        row.addView(cancel, LayoutParams(dp(84), dp(36)))
        row.addView(
            status,
            LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f).apply {
                marginStart = dp(8)
                marginEnd = dp(8)
            },
        )
        row.addView(confirm, LayoutParams(dp(84), dp(36)))
        addView(
            row,
            LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(8)
            },
        )
    }

    /**
     * 更新状态文案与按钮可用性；不触碰 editor 内容/选区。
     *
     * @param message 状态文案（宿主提供；本栏只显示，不记录）
     * @param isError 错误态：用 `palette.error` 着色并写入可读 contentDescription
     * @param submitting 提交中：两个按钮真正禁用，且取消挂起触摸
     * @param confirmAccent 确认按钮填色（宿主按云笔记/普通听写选择）
     */
    fun render(
        message: String,
        isError: Boolean,
        submitting: Boolean,
        confirmAccent: Int = palette.accent,
    ) {
        this.submitting = submitting
        status.text = message
        status.setTextColor(if (isError) palette.error else palette.secondaryText)
        status.contentDescription = message.takeIf { isError && it.isNotBlank() }
        status.importantForAccessibility = if (isError && message.isNotBlank()) {
            IMPORTANT_FOR_ACCESSIBILITY_YES
        } else {
            IMPORTANT_FOR_ACCESSIBILITY_AUTO
        }
        stylePill(confirm, fill = confirmAccent, textColor = palette.surface)
        setControlsEnabled(!submitting)
    }

    /**
     * 取消挂起交互：作废未执行的聚焦 post、移除回调、清 pressed 与未决输入事件。
     * 不触碰 editor 内容/选区。
     */
    fun cancelInteractions() {
        invalidatePendingFocus()
        cancel.isPressed = false
        confirm.isPressed = false
        cancel.cancelPendingInputEvents()
        confirm.cancelPendingInputEvents()
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        postEditorFocus()
    }

    override fun onDetachedFromWindow() {
        // Must run before the platform tears the view hierarchy down: an old bar that
        // is detached can never re-focus the editor after it has been re-attached
        // elsewhere.
        invalidatePendingFocus()
        super.onDetachedFromWindow()
    }

    private fun onCancelClicked() {
        if (submitting || !cancel.isEnabled) return
        onFeedback()
        onCancel()
    }

    private fun onConfirmClicked() {
        if (submitting || !confirm.isEnabled) return
        onFeedback()
        onConfirm()
    }

    private fun postEditorFocus() {
        invalidatePendingFocus()
        val token = focusToken
        val runnable = Runnable {
            // Only the newest bar that still owns this editor may take View focus.
            if (token != focusToken) return@Runnable
            if (!isAttachedToWindow) return@Runnable
            if (editor.parent !== this) return@Runnable
            editor.requestFocus()
        }
        pendingFocus = runnable
        editor.post(runnable)
    }

    private fun invalidatePendingFocus() {
        focusToken++
        pendingFocus?.let { runnable ->
            editor.removeCallbacks(runnable)
            pendingFocus = null
        }
    }

    private fun setControlsEnabled(enabled: Boolean) {
        cancel.isEnabled = enabled
        confirm.isEnabled = enabled
        cancel.alpha = if (enabled) 1f else 0.45f
        confirm.alpha = if (enabled) 1f else 0.45f
        if (!enabled) {
            // Stop any touch that was already tracking one of the buttons so a
            // pending UP cannot deliver a click while submitting.
            cancel.isPressed = false
            confirm.isPressed = false
            cancel.cancelPendingInputEvents()
            confirm.cancelPendingInputEvents()
        }
    }

    private fun styleEditor(edit: EditText) {
        edit.setTextColor(palette.text)
        edit.background = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = dp(10).toFloat()
            setColor(palette.key)
            setStroke(dp(1), palette.separator)
        }
        edit.setPadding(dp(12), dp(8), dp(12), dp(8))
    }

    private fun stylePill(view: TextView, fill: Int, textColor: Int) {
        view.apply {
            textSize = 15f
            gravity = Gravity.CENTER
            setTextColor(textColor)
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(18).toFloat()
                setColor(fill)
            }
            isClickable = true
        }
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()
}
