package com.openless.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.ResultReceiver
import android.util.Log

class OpenLessAccessibilityCommandReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        val action = intent?.action ?: return
        val receiver = resultReceiver(intent) ?: return
        when (action) {
            ACTION_PASTE -> {
                val pasteText = intent.getStringExtra(EXTRA_PASTE_TEXT)
                val result = OpenLessAccessibilityService.performPasteFromCommand(pasteText)
                sendResult(receiver, result)
                if (result != AccessibilityPasteResult.SUCCESS) {
                    Log.w(TAG, "paste command failed reason=${result.reason}")
                }
            }
            ACTION_PING -> {
                val result = if (OpenLessAccessibilityService.instance != null) {
                    AccessibilityPasteResult.SUCCESS
                } else {
                    AccessibilityPasteResult.SERVICE_NOT_CONNECTED
                }
                sendResult(receiver, result)
            }
            ACTION_CAPTURE_SELECTED_TEXT -> {
                val selectedText = OpenLessAccessibilityService.captureSelectedTextFromCommand()
                receiver.send(
                    if (selectedText != null) {
                        AccessibilityPasteResult.SUCCESS.code
                    } else {
                        AccessibilityPasteResult.SERVICE_NOT_CONNECTED.code
                    },
                    Bundle().apply {
                        putString(EXTRA_SELECTED_TEXT, selectedText.orEmpty())
                    },
                )
            }
        }
    }

    private fun sendResult(receiver: ResultReceiver, result: AccessibilityPasteResult) {
        receiver.send(
            result.code,
            Bundle().apply { putString(EXTRA_RESULT_REASON, result.reason) },
        )
    }

    @Suppress("DEPRECATION")
    private fun resultReceiver(intent: Intent): ResultReceiver? {
        return intent.getParcelableExtra(EXTRA_RESULT_RECEIVER) as? ResultReceiver
    }

    companion object {
        const val ACTION_PASTE = "com.openless.app.accessibility.PASTE"
        const val ACTION_PING = "com.openless.app.accessibility.PING"
        const val ACTION_CAPTURE_SELECTED_TEXT = "com.openless.app.accessibility.CAPTURE_SELECTED_TEXT"
        const val EXTRA_RESULT_RECEIVER = "result_receiver"
        const val EXTRA_RESULT_REASON = "result_reason"
        const val EXTRA_PASTE_TEXT = "paste_text"
        const val EXTRA_SELECTED_TEXT = "selected_text"
        /** @deprecated Use [AccessibilityPasteResult] codes */
        const val EXTRA_PASTE_RESULT = "paste_result"
        /** @deprecated Use [AccessibilityPasteResult.SUCCESS.code] */
        const val RESULT_PASTE_SUCCESS = 1
        /** @deprecated Use failure codes from [AccessibilityPasteResult] */
        const val RESULT_PASTE_FAILED = 4
        private const val TAG = "OpenLessA11yCommand"
    }
}
