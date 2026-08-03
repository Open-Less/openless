package com.openless.app

import android.app.Activity
import android.app.ActivityManager
import android.app.Application
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.util.Log

/**
 * Registers activity lifecycle hooks for overlay background trigger mode.
 */
class OpenLessApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        OpenLessAppContext.initialize(this)
        if (isMainProcess()) {
            OpenLessShizukuBridge.initialize()
        }
        registerActivityLifecycleCallbacks(object : ActivityLifecycleCallbacks {
            override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) = Unit
            override fun onActivityStarted(activity: Activity) {
                if (activity.javaClass.name.endsWith("MainActivity")) {
                    maybeHideOverlayOnForeground()
                }
            }
            override fun onActivityResumed(activity: Activity) = Unit
            override fun onActivityPaused(activity: Activity) = Unit
            override fun onActivityStopped(activity: Activity) {
                if (activity.javaClass.name.endsWith("MainActivity")) {
                    maybeShowOverlayOnBackground()
                }
            }
            override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) = Unit
            override fun onActivityDestroyed(activity: Activity) = Unit
        })
    }

    private fun maybeShowOverlayOnBackground() {
        val configured = configuredOverlayTriggerMode()
        val shouldShow = configured == "background" ||
            configured == "always"
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
        return if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.M) {
            Settings.canDrawOverlays(this)
        } else {
            true
        }
    }

    private fun sendOverlayAction(action: String) {
        try {
            startService(Intent(this, OpenLessOverlayService::class.java).apply {
                this.action = action
            })
        } catch (error: Throwable) {
            Log.w(TAG, "overlay action failed: $action", error)
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
        return activityManager.runningAppProcesses
            ?.firstOrNull { it.pid == pid }
            ?.processName
    }

    companion object {
        private const val TAG = "OpenLessApplication"
    }
}
