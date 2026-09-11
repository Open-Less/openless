package com.openless.app

import android.app.Activity
import android.app.ActivityManager
import android.app.Application
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.net.Uri
import android.util.Log

/** Registers activity lifecycle hooks for overlay background trigger mode. */
class OpenLessApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        OpenLessAppContext.initialize(this)
        if (isMainProcess()) {
            OpenLessShizukuBridge.initialize()
        }
        registerActivityLifecycleCallbacks(
            object : ActivityLifecycleCallbacks {
                override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) =
                    Unit

                override fun onActivityStarted(activity: Activity) {
                    if (activity.javaClass.name.endsWith("MainActivity")) {
                        maybeRequestBatteryOptimizationExemption(activity)
                        maybeHideOverlayOnForeground()
                    }
                }

                override fun onActivityResumed(activity: Activity) {
                    if (activity.javaClass == MainActivity::class.java) {
                        watchInterfaceLanguage(activity as MainActivity)
                    }
                }

                override fun onActivityPaused(activity: Activity) {
                    if (activity.javaClass == MainActivity::class.java) {
                        readInterfaceLanguage(activity as MainActivity)
                        localeHandler.removeCallbacksAndMessages(null)
                    }
                }

                override fun onActivityStopped(activity: Activity) {
                    if (activity.javaClass.name.endsWith("MainActivity")) {
                        maybeShowOverlayOnBackground()
                    }
                }

                override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) =
                    Unit

                override fun onActivityDestroyed(activity: Activity) = Unit
            }
        )
    }

    private fun maybeShowOverlayOnBackground() {
        val configured = configuredOverlayTriggerMode()
        val shouldShow = configured == "background" || configured == "always"
        if (!shouldShow) {
            return
        }
        if (!canDrawOverlays()) {
            return
        }
        sendOverlayAction(OpenLessOverlayService.ACTION_SHOW)
    }

    private fun maybeHideOverlayOnForeground() {
        if (configuredOverlayTriggerMode() == "always") {
            if (canDrawOverlays()) {
                sendOverlayAction(OpenLessOverlayService.ACTION_SHOW)
            }
            return
        }
        sendOverlayAction(OpenLessOverlayService.ACTION_HIDE)
    }

    private fun configuredOverlayTriggerMode(): String {
        return OpenLessAndroidPreferences.overlayTriggerMode(this) ?: "background"
    }

    private fun canDrawOverlays(): Boolean {
        return OpenLessPermissionBridge.canDrawOverlaysSafely(this)
    }

    private fun sendOverlayAction(action: String) {
        try {
            startService(
                Intent(this, OpenLessOverlayService::class.java).apply {
                    this.action = action
                }
            )
        } catch (error: Throwable) {
            Log.w(TAG, "overlay action failed: $action", error)
        }
    }

    // The interface language lives in the Tauri WebView, not preferences.json.
    // Mirror only that setting for the native IME; never read editor content.
    private val localeHandler = android.os.Handler(android.os.Looper.getMainLooper())

    private fun watchInterfaceLanguage(activity: Activity) {
        localeHandler.removeCallbacksAndMessages(null)
        val poll = object : Runnable {
            override fun run() {
                if (activity.isFinishing || activity.isDestroyed) return
                readInterfaceLanguage(activity)
                localeHandler.postDelayed(this, 500L)
            }
        }
        localeHandler.post(poll)
    }

    private fun readInterfaceLanguage(activity: Activity) {
        fun findWebView(view: android.view.View): android.webkit.WebView? {
            if (view is android.webkit.WebView) return view
            if (view is android.view.ViewGroup) {
                for (index in 0 until view.childCount) {
                    findWebView(view.getChildAt(index))?.let { return it }
                }
            }
            return null
        }
        val webView = findWebView(activity.window.decorView) ?: return
        webView.evaluateJavascript(
            "(function(){return localStorage.getItem('ol.locale') || document.documentElement.lang || navigator.language;})()",
        ) { result ->
            val locale = result.trim('"').lowercase(java.util.Locale.ROOT)
            if (!locale.matches(Regex("[a-z]{2,3}(-[a-z0-9]{2,8})*"))) return@evaluateJavascript
            val prefs = getSharedPreferences("openless_ime_ui", MODE_PRIVATE)
            if (prefs.getString("locale", null) != locale) {
                prefs.edit().putString("locale", locale).apply()
            }
        }
    }

    private fun maybeRequestBatteryOptimizationExemption(activity: Activity) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
        val power = getSystemService(POWER_SERVICE) as? PowerManager ?: return
        if (power.isIgnoringBatteryOptimizations(packageName)) return
        val prefs = getSharedPreferences("openless_runtime", MODE_PRIVATE)
        if (prefs.getBoolean("battery_optimization_prompted", false)) return
        prefs.edit().putBoolean("battery_optimization_prompted", true).apply()
        runCatching {
            activity.startActivity(
                Intent(
                    Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                    Uri.parse("package:$packageName"),
                )
            )
        }.onFailure { error ->
            Log.w(TAG, "battery optimization exemption request failed", error)
        }
    }

    private fun isMainProcess(): Boolean {
        val processName = currentProcessName() ?: return true
        return processName == packageName
    }

    private fun currentProcessName(): String? {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            return Application.getProcessName()
        }
        val pid = android.os.Process.myPid()
        val activityManager = getSystemService(ACTIVITY_SERVICE) as? ActivityManager ?: return null
        return activityManager.runningAppProcesses?.firstOrNull { it.pid == pid }?.processName
    }

    companion object {
        private const val TAG = "OpenLessApplication"
    }
}
