package com.openless.app

import android.inputmethodservice.InputMethodService
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection

/**
 * OpenLess 输入法服务（v2）。
 * Rust 侧通过 JNI / Tauri plugin 调用 [commitText] 把识别结果提交到当前输入框。
 */
class OpenLessImeService : InputMethodService() {

    companion object {
        @Volatile
        var instance: OpenLessImeService? = null
            private set

        @JvmStatic
        fun commitText(text: String): Boolean {
            val service = instance ?: return false
            val ic = service.currentInputConnection ?: return false
            return ic.commitText(text, 1)
        }
    }

    override fun onCreate() {
        super.onCreate()
        instance = this
    }

    override fun onDestroy() {
        if (instance === this) {
            instance = null
        }
        super.onDestroy()
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
    }
}
