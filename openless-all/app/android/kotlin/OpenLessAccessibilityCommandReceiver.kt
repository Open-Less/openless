package com.openless.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

class OpenLessAccessibilityCommandReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != ACTION_PASTE) return
        val pasted = OpenLessAccessibilityService.performPasteFromCommand()
        if (!pasted) {
            Log.w(TAG, "paste command did not find an editable focused field")
        }
    }

    companion object {
        const val ACTION_PASTE = "com.openless.app.accessibility.PASTE"
        private const val TAG = "OpenLessA11yCommand"
    }
}
