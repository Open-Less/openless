package com.openless.app

import android.app.Activity
import android.app.Application
import android.os.Bundle
import android.util.Log

/**
 * Registers activity lifecycle hooks for overlay background trigger mode.
 */
class OpenLessApplication : Application() {
    override fun onCreate() {
        super.onCreate()
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
        if (!OpenLessNative.nativeCanDrawOverlays(this)) {
            return
        }
        OpenLessNative.nativeShowOverlay(this)
    }

    private fun maybeHideOverlayOnForeground() {
        if (effectiveOverlayTriggerMode() == "always") {
            if (OpenLessNative.nativeCanDrawOverlays(this) && !OpenLessNative.nativeIsOverlayVisible()) {
                OpenLessNative.nativeShowOverlay(this)
            }
            return
        }
        if (OpenLessNative.nativeIsOverlayVisible()) {
            OpenLessNative.nativeHideOverlay(this)
        }
    }

    private fun effectiveOverlayTriggerMode(): String {
        val configured = OpenLessAndroidPreferences.overlayTriggerMode(this) ?: try {
            OpenLessNative.nativeGetOverlayTriggerMode()
        } catch (error: Throwable) {
            Log.w(TAG, "overlay trigger mode unavailable", error)
            "background"
        }
        if (configured == "keyboard" && !OpenLessAccessibilityService.isEnabled(this)) {
            return "always"
        }
        return configured
    }

    companion object {
        private const val TAG = "OpenLessApplication"
    }
}
