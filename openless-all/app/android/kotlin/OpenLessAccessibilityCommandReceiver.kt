package com.openless.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.ResultReceiver
import android.util.Log

class OpenLessAccessibilityCommandReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != ACTION_PASTE) return
        val pasted = OpenLessAccessibilityService.performPasteFromCommand()
        resultReceiver(intent)?.send(
            if (pasted) RESULT_PASTE_SUCCESS else RESULT_PASTE_FAILED,
            Bundle().apply { putBoolean(EXTRA_PASTE_RESULT, pasted) },
        )
        if (!pasted) {
            Log.w(TAG, "paste command did not find an editable focused field")
        }
    }

    @Suppress("DEPRECATION")
    private fun resultReceiver(intent: Intent): ResultReceiver? {
        return intent.getParcelableExtra(EXTRA_RESULT_RECEIVER) as? ResultReceiver
    }

    companion object {
        const val ACTION_PASTE = "com.openless.app.accessibility.PASTE"
        const val EXTRA_RESULT_RECEIVER = "result_receiver"
        const val EXTRA_PASTE_RESULT = "paste_result"
        const val RESULT_PASTE_FAILED = 0
        const val RESULT_PASTE_SUCCESS = 1
        private const val TAG = "OpenLessA11yCommand"
    }
}
