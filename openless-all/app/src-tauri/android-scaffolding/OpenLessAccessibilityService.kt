package com.openless.app

import android.accessibilityservice.AccessibilityService
import android.content.Intent
import android.graphics.Rect
import android.os.Handler
import android.os.Looper
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo

/**
 * Detects IME windows for overlay keyboard trigger mode and performs paste insertion.
 */
class OpenLessAccessibilityService : AccessibilityService() {
    private val mainHandler = Handler(Looper.getMainLooper())
    private val keyboardRefreshRunnable = Runnable { updateKeyboardOverlayState() }

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
        updateKeyboardOverlayState()
        scheduleKeyboardOverlayRefresh()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (event == null) return
        when (event.eventType) {
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED,
            AccessibilityEvent.TYPE_WINDOWS_CHANGED,
            AccessibilityEvent.TYPE_VIEW_FOCUSED -> {
                updateKeyboardOverlayState()
                scheduleKeyboardOverlayRefresh()
            }
        }
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        mainHandler.removeCallbacks(keyboardRefreshRunnable)
        if (instance === this) {
            instance = null
        }
        super.onDestroy()
    }

    private fun scheduleKeyboardOverlayRefresh() {
        mainHandler.removeCallbacks(keyboardRefreshRunnable)
        for (delayMs in KEYBOARD_REFRESH_DELAYS_MS) {
            mainHandler.postDelayed(keyboardRefreshRunnable, delayMs)
        }
    }

    private fun updateKeyboardOverlayState() {
        if (!shouldTrackKeyboard()) {
            return
        }
        if (!OpenLessNative.nativeCanDrawOverlays(this)) {
            return
        }
        val imeBounds = findInputMethodBounds()
        val intent = Intent(this, OpenLessOverlayService::class.java).apply {
            action = OpenLessOverlayService.ACTION_KEYBOARD_CHANGED
            putExtra(OpenLessOverlayService.EXTRA_KEYBOARD_VISIBLE, imeBounds != null)
            imeBounds?.let {
                putExtra(OpenLessOverlayService.EXTRA_KEYBOARD_TOP, it.top)
                putExtra(OpenLessOverlayService.EXTRA_KEYBOARD_BOTTOM, it.bottom)
            }
        }
        startService(intent)
    }

    private fun findInputMethodBounds(): Rect? {
        for (window in windows) {
            if (window.type != AccessibilityWindowInfo.TYPE_INPUT_METHOD) {
                continue
            }
            val bounds = Rect()
            window.getBoundsInScreen(bounds)
            if (!bounds.isEmpty) {
                return bounds
            }
        }
        return null
    }

    private fun shouldTrackKeyboard(): Boolean {
        val localMode = OpenLessAndroidPreferences.overlayTriggerMode(this)
        if (localMode == "keyboard") {
            return true
        }
        if (localMode == "always" || localMode == "background") {
            return isOverlayVisible()
        }
        return try {
            OpenLessNative.nativeGetOverlayTriggerMode() == "keyboard" ||
                OpenLessNative.nativeIsOverlayVisible()
        } catch (_: Throwable) {
            false
        }
    }

    private fun isOverlayVisible(): Boolean {
        return try {
            OpenLessNative.nativeIsOverlayVisible()
        } catch (_: Throwable) {
            false
        }
    }

    private fun performPasteToFocusedField(): Boolean {
        val root = rootInActiveWindow ?: return false
        val focused = root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
            ?: root.findFocus(AccessibilityNodeInfo.FOCUS_ACCESSIBILITY)
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

        @JvmStatic
        fun pasteToFocusedField(): Boolean {
            return instance?.performPasteToFocusedField() == true
        }

        @JvmStatic
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

        private val KEYBOARD_REFRESH_DELAYS_MS = longArrayOf(120L, 360L, 900L, 1600L)
    }
}
