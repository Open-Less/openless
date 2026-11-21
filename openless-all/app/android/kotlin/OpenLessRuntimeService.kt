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

/** Keeps the runtime alive for IME or active authorization, without showing an overlay. */
class OpenLessRuntimeService : Service() {
    private var lastStartId: Int? = null

    override fun onBind(intent: Intent?): IBinder? = null

    // Registers this Service (not an Activity) as the Context every
    // Rust->Kotlin JNI call routes through — see
    // OpenLessNative.nativeRegisterActivityContext()'s doc comment for why.
    // The IME and user-initiated authorization share this context lifetime,
    // independent of whether the warmup/settings Activity still exists.
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
        runningService = this
    }

    override fun onDestroy() {
        if (runningService === this) runningService = null
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
        // Serialize demand snapshots AND Android start/stop requests. A JNI worker
        // cannot acquire between an idle decision and its stop request.
        return synchronized(demandLock) {
            if (runningService !== this) return@synchronized START_NOT_STICKY
            lastStartId = startId
            val requested = runtimeRequestedLocked()
            // A foreground start must be promoted even if its caller has already
            // released demand. Android treats stopping before promotion as a crash.
            if (!requested && intent?.action != ACTION_START_FOREGROUND_RUNTIME) {
                stopRuntimeIfLatestStart()
                return@synchronized START_NOT_STICKY
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
                Log.e(TAG, "failed to promote runtime service", error)
                stopRuntimeIfLatestStart()
                return@synchronized START_NOT_STICKY
            }
            if (!requested) stopRuntimeIfLatestStart()
            // START_STICKY keeps the backend service alive, but it must not launch
            // a visible Tauri Activity merely because the service restarted or a
            // previous Activity was destroyed. Doing so steals the foreground from
            // the app the user is currently using and, after Wry has destroyed the
            // old WebView, can create an empty black Activity. UI warmup remains
            // explicitly initiated by the launcher/settings or the IME path when
            // an input interaction actually requires it.
            if (imeRequested) START_STICKY else START_NOT_STICKY
        }
    }

    /** Main thread, with demandLock held. Android removes the notification when the service stops. */
    private fun stopRuntimeIfLatestStart() {
        val startId = lastStartId ?: return
        // A newer request may still be waiting for its onStartCommand callback.
        if (stopSelfResult(startId)) runningService = null
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
                .setContentTitle(if (imeRequested) "OpenLess 输入法" else "OpenLess")
                .setContentText(if (imeRequested) "输入法面板已就绪" else "后台运行服务已就绪")
                .setSmallIcon(R.mipmap.ic_launcher)
                .setOngoing(true)
                .build()
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
                .setContentTitle(if (imeRequested) "OpenLess 输入法" else "OpenLess")
                .setContentText(if (imeRequested) "输入法面板已就绪" else "后台运行服务已就绪")
                .setSmallIcon(R.mipmap.ic_launcher)
                .setOngoing(true)
                .build()
        }
    }

    companion object {
        @Volatile var imeRequested = false
            private set
        private val demandLock = Any()
        private var authorizationUsers = 0
        private val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())
        // Service callbacks and reconciliation run on the main thread only.
        private var runningService: OpenLessRuntimeService? = null

        fun authorizationActive(): Boolean = synchronized(demandLock) { authorizationUsers > 0 }
        private fun runtimeRequestedLocked(): Boolean = imeRequested || authorizationUsers > 0

        private fun startRuntimeLocked(context: android.content.Context) {
            val intent = Intent(context, OpenLessRuntimeService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent.setAction(ACTION_START_FOREGROUND_RUNTIME))
            } else {
                context.startService(intent)
            }
        }

        fun setImeRequested(context: android.content.Context, requested: Boolean) {
            synchronized(demandLock) {
                imeRequested = requested
                if (requested) startRuntimeLocked(context)
                else reconcileDemand(context)
            }
        }

        @androidx.annotation.Keep
        @JvmStatic fun acquireAuthorization(context: android.content.Context) {
            synchronized(demandLock) {
                authorizationUsers += 1
                try {
                    startRuntimeLocked(context)
                } catch (error: Throwable) {
                    authorizationUsers -= 1
                    throw error
                }
            }
        }

        @androidx.annotation.Keep
        @JvmStatic fun releaseAuthorization(context: android.content.Context) {
            synchronized(demandLock) {
                authorizationUsers = maxOf(0, authorizationUsers - 1)
            }
            reconcileDemand(context)
        }

        private fun reconcileDemand(context: android.content.Context) {
            val application = context.applicationContext
            mainHandler.post {
                synchronized(demandLock) {
                    // Re-read CURRENT demand after any newer JNI acquire/IME start.
                    // In particular, never post a captured stopService decision.
                    if (!runtimeRequestedLocked()) {
                        runningService?.stopRuntimeIfLatestStart()
                    } else if (runningService?.lastStartId != null) {
                        // Re-deliver onStartCommand to refresh the notification AND
                        // sticky policy. This is an already-running service, not a
                        // new background startForegroundService authorization request.
                        runCatching { application.startService(Intent(application, OpenLessRuntimeService::class.java)) }
                            .onFailure { Log.w(TAG, "failed to reconcile runtime demand", it) }
                    }
                    // If creation is still queued, its onStartCommand reads current
                    // demand. No release/IME teardown may resurrect a missing service.
                }
            }
        }

        private const val NOTIFICATION_ID = 42002
        private const val TAG = "OpenLessRuntimeService"
        private const val ACTION_START_FOREGROUND_RUNTIME = "com.openless.app.action.START_FOREGROUND_RUNTIME"
        private const val ACTION_RUNTIME_ACTIVITY_DESTROYED = "com.openless.app.action.RUNTIME_ACTIVITY_DESTROYED"
        private const val EXTRA_DESTROY_REASON = "reason"

        // String literal duplicated on the Rust side (mobile_runtime.rs's
        // RunEvent::Exit handler) rather than shared as a constant — Rust
        // calls this Service by fully-qualified class/action name through
        // the generic start_service_action() JNI helper, the same way
        // native_bridge.rs already targets OpenLessOverlayService.
        private const val ACTION_RUNTIME_EXITED = "com.openless.app.action.RUNTIME_EXITED"

        /**
         * Called from OpenLessBackendWarmupActivity.onDestroy() to retain the
         * lifecycle counters. onStartCommand() reconciles current IME/auth demand;
         * teardown alone neither keeps an idle FGS alive nor relaunches an Activity.
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
