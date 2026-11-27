package com.openless.app

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings

/**
 * Walks the user through granting SYSTEM_ALERT_WINDOW. The Rust command
 * request_android_overlay_permission launches this Activity via an Intent.
 */
class OverlayPermissionActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.M &&
                !OpenLessPermissionBridge.canDrawOverlaysSafely(this)
        ) {
            val intent =
                Intent(
                    Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                    Uri.parse("package:$packageName"),
                )
            startActivity(intent)
        }
        finish()
    }
}
