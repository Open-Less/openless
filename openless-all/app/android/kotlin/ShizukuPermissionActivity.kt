package com.openless.app

import android.app.Activity
import android.app.AlertDialog
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import rikka.shizuku.Shizuku

/**
 * Translucent activity that requests Shizuku binder permission from the user.
 */
class ShizukuPermissionActivity : Activity(), Shizuku.OnRequestPermissionResultListener {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Shizuku.addRequestPermissionResultListener(this)
        if (OpenLessShizukuBridge.isLegacyShizukuBackend()) {
            OpenLessShizukuBridge.setLastPermissionMessageKey("unsupported_backend")
            finish()
            return
        }
        try {
            if (Shizuku.pingBinder() && Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED) {
                OpenLessShizukuBridge.setLastPermissionMessageKey("already_granted")
                finish()
                return
            }
            if (!Shizuku.pingBinder()) {
                Log.w(TAG, "Shizuku binder unavailable during permission request")
                OpenLessShizukuBridge.setLastPermissionMessageKey("binder_unavailable")
                finish()
                return
            }
            if (Shizuku.shouldShowRequestPermissionRationale()) {
                AlertDialog.Builder(this)
                    .setMessage(R.string.openless_shizuku_permission_blocked)
                    .setPositiveButton(R.string.openless_shizuku_open_manager) { _, _ ->
                        OpenLessShizukuBridge.openShizukuApp(this)
                        OpenLessShizukuBridge.setLastPermissionMessageKey("permission_permanently_denied")
                        finish()
                    }
                    .setNegativeButton(android.R.string.cancel) { _, _ ->
                        OpenLessShizukuBridge.setLastPermissionMessageKey("permission_permanently_denied")
                        finish()
                    }
                    .setOnCancelListener {
                        OpenLessShizukuBridge.setLastPermissionMessageKey("permission_permanently_denied")
                        finish()
                    }
                    .show()
                return
            }
            Shizuku.requestPermission(REQUEST_CODE)
        } catch (error: Throwable) {
            Log.w(TAG, "Shizuku permission request failed", error)
            OpenLessShizukuBridge.setLastPermissionMessageKey("unsupported_backend")
            finish()
        }
    }

    override fun onRequestPermissionResult(requestCode: Int, grantResult: Int) {
        if (requestCode == REQUEST_CODE) {
            val granted = grantResult == PackageManager.PERMISSION_GRANTED
            Log.i(TAG, "Shizuku permission result granted=$granted")
            OpenLessShizukuBridge.setLastPermissionMessageKey(
                if (granted) "granted" else "denied",
            )
            finish()
        }
    }

    override fun onDestroy() {
        Shizuku.removeRequestPermissionResultListener(this)
        super.onDestroy()
    }

    companion object {
        private const val TAG = "OpenLessShizukuPerm"
        private const val REQUEST_CODE = 9201
    }
}
