package com.openless.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log

/** Keeps the OpenLess process alive while the system IME is active, without showing an overlay. */
class OpenLessRuntimeService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    // Registers this Service (not an Activity) as the Context every
    // Rust->Kotlin JNI call routes through — see
    // OpenLessNative.nativeRegisterActivityContext()'s doc comment for why.
    // This Service starts in onCreate()/onCreateInputView() and is stopped
    // in OpenLessImeService.onDestroy(), so the registration stays valid
    // for as long as the IME itself is alive, independent of whether
    // OpenLessBackendWarmupActivity (background warmup / settings host)
    // exists, is backgrounded, or has been reclaimed by the OS.
    override fun onCreate() {
        super.onCreate()
        // #region agent log
        runCatching {
            OpenLessNative.nativeRegisterActivityContext(this)
            android.util.Log.i(
                "OpenLessDbg58c22b",
                """{"sessionId":"58c22b","hypothesisId":"A","location":"OpenLessRuntimeService.onCreate","message":"registered runtime service context","data":{"hasCtx":${OpenLessNative.nativeHasRegisteredActivityContext()}},"timestamp":${System.currentTimeMillis()}}""",
            )
        }
            .onFailure { error ->
                Log.w(TAG, "register runtime service context failed", error)
                android.util.Log.w(
                    "OpenLessDbg58c22b",
                    """{"sessionId":"58c22b","hypothesisId":"A","location":"OpenLessRuntimeService.onCreate","message":"register failed","data":{"error":"${error.message}"},"timestamp":${System.currentTimeMillis()}}""",
                )
            }
        // #endregion
    }

    override fun onDestroy() {
        // #region agent log
        android.util.Log.i(
            "OpenLessDbg58c22b",
            """{"sessionId":"58c22b","hypothesisId":"A","location":"OpenLessRuntimeService.onDestroy","message":"unregistering runtime service context","data":{"hasCtxBefore":${runCatching { OpenLessNative.nativeHasRegisteredActivityContext() }.getOrDefault(false)}},"timestamp":${System.currentTimeMillis()}}""",
        )
        // #endregion
        runCatching { OpenLessNative.nativeUnregisterActivityContext(this) }
        // Re-baseline to Application so settings / overlay / mic IPC keep working
        // after the user switches away from the OpenLess IME.
        runCatching { OpenLessNative.nativeRegisterActivityContext(applicationContext) }
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // A null intent here is the OS's own restart-after-death signal for
        // a START_STICKY service (documented Service.onStartCommand()
        // contract) — the strongest available evidence that the whole
        // process was actually killed, as opposed to any of the other
        // signals below which can also fire while the process survives.
        if (intent == null) {
            OpenLessProcessRestartStats(this, "sticky").recordStart()
        }
        if (intent?.action == ACTION_RUNTIME_ACTIVITY_DESTROYED) {
            // reason disambiguates *why* onDestroy() fired — added because
            // the plain "actkill" total conflated at least three different
            // causes (self-triggered recreate() for a stuck WebView, a
            // config change, and genuine unexplained OS reclamation) into
            // one number, making it impossible to tell from the stats
            // screen alone whether "actkill" was ever actually the OS
            // killing something. Recorded as its own sub-category
            // alongside (not instead of) the original "actkill" so
            // existing dashboards/screenshots keep meaning the same thing.
            val reason = intent.getStringExtra(EXTRA_DESTROY_REASON) ?: "unknown"
            Log.w(TAG, "runtime host activity was destroyed (reason=$reason); re-checking backend")
            OpenLessProcessRestartStats(this, "actkill").recordStart()
            OpenLessProcessRestartStats(this, "actkill_$reason").recordStart()
        }
        if (intent?.action == ACTION_RUNTIME_EXITED) {
            Log.w(TAG, "Tauri RunEvent::Exit fired; recording and re-checking backend")
            OpenLessProcessRestartStats(this, "rtexit").recordStart()
        }
        try {
            val notification = buildNotification()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
                    } else {
                        0
                    },
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (error: Throwable) {
            Log.e(TAG, "failed to promote IME runtime service", error)
            stopSelf(startId)
            return START_NOT_STICKY
        }
        // START_STICKY keeps the backend service alive, but it must not launch
        // a visible Tauri Activity merely because the service restarted or a
        // previous Activity was destroyed. Doing so steals the foreground from
        // the app the user is currently using and, after Wry has destroyed the
        // old WebView, can create an empty black Activity. UI warmup remains
        // explicitly initiated by the launcher/settings or the IME path when
        // an input interaction actually requires it.
        return START_STICKY
    }

    private fun buildNotification(): Notification {
        // Renamed from "openless_ime_runtime" (not just lowered the same
        // channel's importance): once a channel is created, its importance
        // is user-owned from then on — createNotificationChannel() with a
        // different importance on an EXISTING channel id is a documented
        // no-op, so an already-installed app would never actually see this
        // get quieter without a fresh channel id forcing a fresh channel.
        val channelId = "openless_ime_runtime_v2"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(
                    channelId,
                    "OpenLess 输入法服务",
                    NotificationManager.IMPORTANCE_MIN,
                )
            )
        }
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, channelId)
                .setContentTitle("OpenLess 输入法")
                .setContentText("输入法面板已就绪")
                .setSmallIcon(R.mipmap.ic_launcher)
                .setOngoing(true)
                .build()
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
                .setContentTitle("OpenLess 输入法")
                .setContentText("输入法面板已就绪")
                .setSmallIcon(R.mipmap.ic_launcher)
                .setOngoing(true)
                .build()
        }
    }

    companion object {
        private const val NOTIFICATION_ID = 42002
        private const val TAG = "OpenLessRuntimeService"
        private const val ACTION_RUNTIME_ACTIVITY_DESTROYED = "com.openless.app.action.RUNTIME_ACTIVITY_DESTROYED"
        private const val EXTRA_DESTROY_REASON = "reason"

        // String literal duplicated on the Rust side (mobile_runtime.rs's
        // RunEvent::Exit handler) rather than shared as a constant — Rust
        // calls this Service by fully-qualified class/action name through
        // the generic start_service_action() JNI helper, the same way
        // native_bridge.rs already targets OpenLessOverlayService.
        private const val ACTION_RUNTIME_EXITED = "com.openless.app.action.RUNTIME_EXITED"

        /**
         * Called from OpenLessBackendWarmupActivity.onDestroy() — this
         * service is the supervisor that decides whether/when to relaunch
         * the runtime host Activity (via ensureBackendReady(), already run
         * unconditionally at the end of onStartCommand()), not the dying
         * Activity deciding for itself. The action is only for logging here
         * today; onStartCommand() already re-checks on every start
         * regardless of why it was started.
         *
         * @param reason one of "config" (isChangingConfigurations was true —
         *   a rotation/density/locale change, not a kill), "finishing"
         *   (isFinishing was true — unexpected, this Activity never calls
         *   finish() on itself deliberately), or "os" (none of the above —
         *   the only case that's actually the system reclaiming this task).
         */
        fun notifyRuntimeActivityDestroyed(context: android.content.Context, reason: String) {
            context.startService(
                Intent(context, OpenLessRuntimeService::class.java)
                    .setAction(ACTION_RUNTIME_ACTIVITY_DESTROYED)
                    .putExtra(EXTRA_DESTROY_REASON, reason),
            )
        }
    }
}
