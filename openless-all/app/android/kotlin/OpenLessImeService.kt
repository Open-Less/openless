package com.openless.app

import android.graphics.Color
import android.inputmethodservice.InputMethodService
import android.text.InputType
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView

/** Minimal system IME surface. Voice transport is intentionally added in a later phase. */
class OpenLessImeService : InputMethodService() {
    private var sessionEpoch = 0L
    private var status: TextView? = null

    override fun onCreateInputView(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(16, 12, 16, 12)
            setBackgroundColor(Color.rgb(245, 245, 245))
        }
        status = TextView(this).apply {
            text = "OpenLess：空闲"
            setTextColor(Color.DKGRAY)
            setPadding(0, 0, 0, 8)
        }
        root.addView(status, matchParent())
        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        row.addView(button("🎙 开始/停止") { updateStatus("OpenLess：语音链路待接入") })
        row.addView(button("取消") { invalidateSession("OpenLess：已取消") })
        root.addView(row, matchParent())
        val actions = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        actions.addView(button("测试上屏") { commitTestText() })
        actions.addView(button("切换输入法") {
            (getSystemService(INPUT_METHOD_SERVICE) as? InputMethodManager)?.showInputMethodPicker()
        })
        actions.addView(button("设置") { openSettings() })
        root.addView(actions, matchParent())
        return root
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        sessionEpoch++
        if (isSensitiveField(attribute)) {
            updateStatus("OpenLess：敏感字段，已禁用听写")
        } else {
            updateStatus("OpenLess：空闲")
        }
    }

    override fun onFinishInput() {
        invalidateSession("OpenLess：输入目标已失效")
        super.onFinishInput()
    }

    private fun commitTestText() {
        val attribute = currentInputEditorInfo
        if (isSensitiveField(attribute)) {
            updateStatus("OpenLess：敏感字段，禁止上屏")
            return
        }
        val connection = currentInputConnection
        if (connection == null) {
            updateStatus("OpenLess：没有有效输入连接")
            return
        }
        val epoch = sessionEpoch
        if (epoch != sessionEpoch || !connection.commitText(TEST_TEXT, 1)) {
            updateStatus("OpenLess：输入连接已失效")
            return
        }
        updateStatus("OpenLess：已上屏")
    }

    private fun invalidateSession(message: String) {
        sessionEpoch++
        updateStatus(message)
    }

    private fun isSensitiveField(attribute: EditorInfo?): Boolean {
        val inputType = attribute?.inputType ?: return false
        val variation = inputType and InputType.TYPE_MASK_VARIATION
        return (inputType and InputType.TYPE_MASK_CLASS) == InputType.TYPE_CLASS_NUMBER ||
            variation == InputType.TYPE_TEXT_VARIATION_PASSWORD ||
            variation == InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD ||
            variation == InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
    }

    private fun button(label: String, action: () -> Unit) = Button(this).apply {
        text = label
        setOnClickListener { action() }
        layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
    }

    private fun matchParent() = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT,
        ViewGroup.LayoutParams.WRAP_CONTENT,
    )

    private fun updateStatus(message: String) {
        status?.text = message
    }

    private fun openSettings() {
        packageManager.getLaunchIntentForPackage(packageName)?.let(::startActivity)
    }

    companion object {
        private const val TEST_TEXT = "OpenLess IME 测试上屏"
    }
}
