package com.openless.app

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper

/**
 * Transient foreground hop used when Android rejects microphone FGS promotion from a background
 * overlay tap. The service performs the real start while this Activity is foreground.
 */
class OpenLessOverlayRecordingActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        startService(
            Intent(this, OpenLessOverlayService::class.java)
                .setAction(OpenLessOverlayService.ACTION_START_RECORDING),
        )
        Handler(Looper.getMainLooper()).postDelayed({ finish() }, 500)
    }
}
