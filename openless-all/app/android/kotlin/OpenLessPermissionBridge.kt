package com.openless.app

import android.Manifest
import android.app.AppOpsManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import androidx.annotation.Keep
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

@Keep
object OpenLessPermissionBridge {
    private const val TAG = "OpenLessPermissionBridge"

    private val requestInFlight = AtomicBoolean(false)

    @Keep
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

    /**
     * Safe overlay permission query for Rust JNI / WebView IPC threads.
     * HyperOS and some OEM skins may throw from Settings.canDrawOverlays off the main thread.
     */
    @Keep
    @JvmStatic
    fun canDrawOverlaysSafely(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            return true
        }
        val appContext = OpenLessAppContext.context ?: context.applicationContext
        return if (Looper.myLooper() == Looper.getMainLooper()) {
            queryCanDrawOverlays(appContext)
        } else {
            val result = AtomicBoolean(false)
            val latch = CountDownLatch(1)
            Handler(Looper.getMainLooper()).post {
                try {
                    result.set(queryCanDrawOverlays(appContext))
                } finally {
                    latch.countDown()
                }
            }
            try {
                latch.await(2, TimeUnit.SECONDS)
            } catch (error: InterruptedException) {
                Thread.currentThread().interrupt()
                Log.w(TAG, "canDrawOverlaysSafely interrupted", error)
            }
            result.get()
        }
    }

    private fun queryCanDrawOverlays(context: Context): Boolean {
        return try {
            Settings.canDrawOverlays(context)
        } catch (error: Throwable) {
            Log.w(TAG, "Settings.canDrawOverlays failed, trying AppOpsManager", error)
            queryCanDrawOverlaysViaAppOps(context)
        }
    }

    private fun queryCanDrawOverlaysViaAppOps(context: Context): Boolean {
        return try {
            val appOps = context.getSystemService(Context.APP_OPS_SERVICE) as AppOpsManager
            val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                appOps.unsafeCheckOpNoThrow(
                    AppOpsManager.OPSTR_SYSTEM_ALERT_WINDOW,
                    android.os.Process.myUid(),
                    context.packageName,
                )
            } else {
                @Suppress("DEPRECATION")
                appOps.checkOpNoThrow(
                    AppOpsManager.OPSTR_SYSTEM_ALERT_WINDOW,
                    android.os.Process.myUid(),
                    context.packageName,
                )
            }
            mode == AppOpsManager.MODE_ALLOWED
        } catch (error: Throwable) {
            Log.w(TAG, "AppOpsManager overlay check failed", error)
            false
        }
    }
}
