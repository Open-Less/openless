package com.openless.app

import android.app.Activity
import android.app.Application
import android.os.Bundle

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
        if (OpenLessNative.nativeGetOverlayTriggerMode() != "background") {
            return
        }
        if (!OpenLessNative.nativeCanDrawOverlays()) {
            return
        }
        OpenLessNative.nativeShowOverlay()
    }

    private fun maybeHideOverlayOnForeground() {
        if (OpenLessNative.nativeGetOverlayTriggerMode() == "always") {
            return
        }
        if (OpenLessNative.nativeIsOverlayVisible()) {
            OpenLessNative.nativeHideOverlay()
        }
    }
}
