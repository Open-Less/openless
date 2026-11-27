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
        if (action == ACTION_VOCABULARY) {
            OpenLessAccessibilityService.handleVocabularyCommand(intent)
            return
        }
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
                val result =
                    if (OpenLessAccessibilityService.instance != null) {
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
            ACTION_CAPTURE_SELECTION_TARGET -> {
                val target = OpenLessAccessibilityService.captureSelectionTargetFromCommand()
                receiver.send(
                    if (!target.isNullOrEmpty()) AccessibilityPasteResult.SUCCESS.code
                    else AccessibilityPasteResult.NO_FOCUSED_EDITOR.code,
                    Bundle().apply { putString(EXTRA_SELECTION_TARGET, target.orEmpty()) },
                )
            }
            ACTION_REPLACE_CAPTURED_SELECTION -> {
                val generation = intent.getLongExtra(EXTRA_SELECTION_GENERATION, 0L)
                val replacement = intent.getStringExtra(EXTRA_SELECTION_REPLACEMENT).orEmpty()
                sendResult(
                    receiver,
                    OpenLessAccessibilityService.replaceCapturedSelectionFromCommand(
                        generation,
                        replacement,
                    ),
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
        const val ACTION_VOCABULARY = "com.openless.app.accessibility.VOCABULARY"
        const val ACTION_PASTE = "com.openless.app.accessibility.PASTE"
        const val ACTION_PING = "com.openless.app.accessibility.PING"
        const val ACTION_CAPTURE_SELECTED_TEXT =
            "com.openless.app.accessibility.CAPTURE_SELECTED_TEXT"
        const val ACTION_CAPTURE_SELECTION_TARGET =
            "com.openless.app.accessibility.CAPTURE_SELECTION_TARGET"
        const val ACTION_REPLACE_CAPTURED_SELECTION =
            "com.openless.app.accessibility.REPLACE_CAPTURED_SELECTION"
        const val EXTRA_RESULT_RECEIVER = "result_receiver"
        const val EXTRA_RESULT_REASON = "result_reason"
        const val EXTRA_PASTE_TEXT = "paste_text"
        const val EXTRA_SELECTED_TEXT = "selected_text"
        const val EXTRA_SELECTION_TARGET = "selection_target"
        const val EXTRA_SELECTION_GENERATION = "selection_generation"
        const val EXTRA_SELECTION_REPLACEMENT = "selection_replacement"
        private const val TAG = "OpenLessA11yCommand"
    }
}
