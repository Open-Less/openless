package com.openless.app

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import java.util.concurrent.atomic.AtomicBoolean

object OpenLessPermissionBridge {
    private const val TAG = "OpenLessPermissionBridge"

    private val requestInFlight = AtomicBoolean(false)

    @JvmStatic
    fun requestRecordAudioPermission(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            return true
        }
        if (context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            return true
        }
        if (!requestInFlight.compareAndSet(false, true)) {
            Log.i(TAG, "RECORD_AUDIO permission request already in flight")
            return false
        }
        return try {
            val intent = Intent(context, MicrophonePermissionActivity::class.java).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
            false
        } catch (error: Throwable) {
            requestInFlight.set(false)
            Log.w(TAG, "failed to launch RECORD_AUDIO permission activity", error)
            context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        }
    }

    @JvmStatic
    fun resolveRecordAudioPermission(granted: Boolean) {
        Log.i(TAG, "RECORD_AUDIO permission completed granted=$granted")
        requestInFlight.set(false)
    }
}
