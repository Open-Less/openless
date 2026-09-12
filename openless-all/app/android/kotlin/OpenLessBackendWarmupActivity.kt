package com.openless.app

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.content.Context
import android.content.Intent

/** Starts the Tauri/Rust runtime without presenting the settings UI. */
class OpenLessBackendWarmupActivity : MainActivity() {
    // Activity 实例化阶段尚未 attach Context，不能访问 Activity.mainLooper。
    private val warmupHandler = Handler(Looper.getMainLooper())
    private val sendToBackground = Runnable {
        if (!settingsRequested && !isFinishing && !isDestroyed) {
            // Tauri/Rust runtime is owned by this Activity. Keep it alive as the
            // single UI/runtime host, but never relaunch the editor's package here:
            // a package launch intent only knows that app's launcher Activity, which
            // for apps like Settings or WeChat mini programs is not the screen the
            // user was actually typing in, and replacing it destroys their context.
            // Moving this task behind the current one preserves the exact
            // Activity/window that requested the IME.
            overridePendingTransition(0, 0)
            moveTaskToBack(true)
            OpenLessImeService.requestInputPanelAfterWarmup(260L)
        }
    }
    private var settingsRequested = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        activeInstance = java.lang.ref.WeakReference(this)
        settingsRequested = intent.getBooleanExtra(EXTRA_SHOW_SETTINGS, false)

        // 不再修改窗口透明度或触摸属性。主 Activity 必须以正常窗口完成
        // Tauri/WebView 初始化，完成后仅退到后台，避免留下黑色/空白窗口状态。
        // Tauri/WebView keeps initializing natively after super.onCreate() returns.
        // Backgrounding this window while that is still in flight has produced a
        // native "destroyed mutex" abort in HWUI's worker pool; suppressing the
        // enter transition avoids extra render work racing with that teardown.
        overridePendingTransition(0, 0)
        warmupHandler.postDelayed(sendToBackground, 180L)
    }

    @Suppress("DEPRECATION", "MissingSuperCall")
    override fun onBackPressed() {
        // This Activity is the single, process-lifetime Tauri/Rust host and must
        // never actually finish() while the process is alive: finishing destroys
        // the window Surface (unlike moveTaskToBack, which only hides it), and
        // that race with HWUI's worker-pool teardown is what produces the native
        // "destroyed mutex" abort. The default back behavior would finish() this
        // Activity once there is no more back-stack, so always background it
        // instead — skipping super.onBackPressed() is intentional here.
        overridePendingTransition(0, 0)
        moveTaskToBack(true)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (intent.getBooleanExtra(EXTRA_SHOW_SETTINGS, false)) {
            settingsRequested = true
            warmupHandler.removeCallbacks(sendToBackground)
        }
    }

    override fun onDestroy() {
        warmupHandler.removeCallbacks(sendToBackground)
        if (activeInstance?.get() === this) {
            activeInstance = null
        }
        super.onDestroy()
    }

    companion object {
        @Volatile
        private var activeInstance: java.lang.ref.WeakReference<OpenLessBackendWarmupActivity>? = null

        private const val EXTRA_SHOW_SETTINGS = "com.openless.app.extra.SHOW_SETTINGS"

        /** The single Tauri host is still alive even while its task is in the background. */
        fun isRunning(): Boolean {
            val activity = activeInstance?.get() ?: return false
            return !activity.isFinishing && !activity.isDestroyed
        }

        /** Bring the existing Tauri host forward instead of creating a black second host. */
        fun openSettingsIfRunning(context: Context): Boolean {
            val activity = activeInstance?.get() ?: return false
            if (activity.isFinishing || activity.isDestroyed) return false
            context.startActivity(Intent(context, OpenLessBackendWarmupActivity::class.java).apply {
                putExtra(EXTRA_SHOW_SETTINGS, true)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP)
                addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
                addFlags(Intent.FLAG_ACTIVITY_NO_ANIMATION)
            })
            return true
        }

        /**
         * Show settings, reusing the existing Tauri host if one is alive. Only
         * starts a fresh Activity when none exists yet. Always targets this
         * class (never the bare MainActivity) so there is ever only one
         * tracked Tauri host, regardless of whether it was created for warmup
         * or for settings — starting MainActivity directly here would spin up
         * an untracked second host and re-run Tauri/Rust setup from scratch.
         */
        fun openSettings(context: Context) {
            if (openSettingsIfRunning(context)) return
            context.startActivity(Intent(context, OpenLessBackendWarmupActivity::class.java).apply {
                putExtra(EXTRA_SHOW_SETTINGS, true)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(Intent.FLAG_ACTIVITY_NO_ANIMATION)
            })
        }
    }
}
