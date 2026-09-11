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
            // single UI/runtime host, but return focus to the app that owns the editor.
            val editorPackage = intent.getStringExtra(EXTRA_RETURN_PACKAGE)
            val restoredEditor = editorPackage
                ?.takeIf { it.isNotBlank() && it != packageName }
                ?.let { packageManager.getLaunchIntentForPackage(it) }
                ?.let { launchIntent ->
                    runCatching {
                        launchIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                        launchIntent.addFlags(Intent.FLAG_ACTIVITY_NO_ANIMATION)
                        startActivity(launchIntent)
                    }.isSuccess
                } == true
            if (!restoredEditor) {
                moveTaskToBack(true)
            }
            OpenLessImeService.requestInputPanelAfterWarmup(if (restoredEditor) 420L else 260L)
        }
    }
    private var settingsRequested = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        activeInstance = java.lang.ref.WeakReference(this)
        settingsRequested = intent.getBooleanExtra(EXTRA_SHOW_SETTINGS, false)

        // 不再修改窗口透明度或触摸属性。主 Activity 必须以正常窗口完成
        // Tauri/WebView 初始化，完成后仅退到后台，避免留下黑色/空白窗口状态。
        warmupHandler.postDelayed(sendToBackground, 180L)
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
        const val EXTRA_RETURN_PACKAGE = "com.openless.app.extra.RETURN_PACKAGE"

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
    }
}
