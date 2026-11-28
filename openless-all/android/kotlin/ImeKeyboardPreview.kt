package com.openless.app

import android.content.Context
import android.view.Gravity
import android.view.MotionEvent
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/** The actual typing component in a non-interactive, backend-free settings preview. */
internal class ImeKeyboardPreview(context: Context, palette: ImePalette, englishUi: Boolean) : FrameLayout(context) {
    init {
        tag = "ime-keyboard-preview"
        // The surrounding size caption is accessible. Do not expose fake keys/actions.
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        val header = LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(10), 0, dp(6), 0)
            addView(TextView(context).apply {
                text = "OpenLess"
                textSize = 16f
                setTextColor(palette.text)
            }, LinearLayout.LayoutParams(LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.MATCH_PARENT).apply { marginEnd = dp(10) })
            addView(ImeModeBar(
                context, palette,
                if (englishUi) listOf("Voice", "Stroke", "Edit", "Keys") else listOf("语音", "笔画", "编辑", "键盘"),
                3,
            ) {}, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f))
        }
        val panel = ImeTypingPanel(
            context, palette, header,
            onAction = {}, onFeedback = {}, onVoiceStart = { false }, onVoiceEnd = {},
            interfaceEnglish = englishUi,
        )
        panel.renderKeyboard(
            ImeKeyboardLayer.LETTERS,
            if (englishUi) ImeTypingLanguage.ENGLISH else ImeTypingLanguage.PINYIN,
            ImeLetterCase.LOWER, if (englishUi) "Return" else "换行", englishUi,
        )
        val words = if (englishUi) listOf("Hello", "keyboard", "preview") else listOf("你好", "输入法", "预览")
        panel.submitCandidates(words.map { ImeTypingPanel.Candidate(it, select = {}) }, if (englishUi) null else "nihao")
        addView(panel, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    }

    override fun onInterceptTouchEvent(event: MotionEvent): Boolean = true
    override fun onTouchEvent(event: MotionEvent): Boolean = true
    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
}
