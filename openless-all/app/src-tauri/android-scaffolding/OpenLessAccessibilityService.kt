package com.openless.app

import android.accessibilityservice.AccessibilityService
import android.content.Intent
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo

/**
 * Detects IME windows for overlay keyboard trigger mode and performs paste insertion.
 */
class OpenLessAccessibilityService : AccessibilityService() {

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (event == null) return
        if (event.eventType != AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) {
            return
        }
        val className = event.className?.toString().orEmpty()
        if (!className.contains("InputMethod", ignoreCase = true)) {
            return
        }
        if (OpenLessNative.nativeGetOverlayTriggerMode() != "keyboard") {
            return
        }
        if (!OpenLessNative.nativeCanDrawOverlays(this)) {
            return
        }
        startService(
            Intent(this, OpenLessOverlayService::class.java).setAction(OpenLessOverlayService.ACTION_SHOW),
        )
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        if (instance === this) {
            instance = null
        }
        super.onDestroy()
    }

    fun pasteToFocusedField(): Boolean {
        val root = rootInActiveWindow ?: return false
        val focused = root.findFocus(AccessibilityEvent.TYPE_VIEW_FOCUSED)
            ?: root.findFocus(AccessibilityEvent.TYPE_VIEW_TEXT_SELECTION_CHANGED)
            ?: return false
        if (!focused.isEditable) {
            focused.recycle()
            return false
        }
        val pasted = focused.performAction(AccessibilityNodeInfo.ACTION_PASTE)
        focused.recycle()
        return pasted
    }

    companion object {
        @Volatile
        var instance: OpenLessAccessibilityService? = null
            private set

        fun pasteToFocusedField(): Boolean {
            return instance?.pasteToFocusedField() == true
        }

        fun isEnabled(context: android.content.Context): Boolean {
            val enabled = android.provider.Settings.Secure.getInt(
                context.contentResolver,
                android.provider.Settings.Secure.ACCESSIBILITY_ENABLED,
                0,
            ) == 1
            if (!enabled) return false
            val services = android.provider.Settings.Secure.getString(
                context.contentResolver,
                android.provider.Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            ) ?: return false
            return services.contains("${context.packageName}/${OpenLessAccessibilityService::class.java.name}")
        }
    }
}
