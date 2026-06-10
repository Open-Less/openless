package com.openless.app

import android.app.Activity
import android.app.Application
import android.content.Intent
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
        val trigger = effectiveOverlayTriggerMode()
        if (trigger != "background" && trigger != "always") {
            return
        }
        if (!canDrawOverlays()) {
            return
        }
        sendOverlayAction(OpenLessOverlayService.ACTION_SHOW)
    }

    private fun maybeHideOverlayOnForeground() {
        if (effectiveOverlayTriggerMode() == "always") {
            if (canDrawOverlays()) {
                sendOverlayAction(OpenLessOverlayService.ACTION_SHOW)
            }
            return
        }
        sendOverlayAction(OpenLessOverlayService.ACTION_HIDE)
    }

    private fun effectiveOverlayTriggerMode(): String {
        val configured = OpenLessAndroidPreferences.overlayTriggerMode(this) ?: "background"
        if (configured == "keyboard" && !OpenLessAccessibilityService.isOperational(this)) {
            return "always"
        }
        return configured
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

    companion object {
        private const val TAG = "OpenLessApplication"
    }
}
