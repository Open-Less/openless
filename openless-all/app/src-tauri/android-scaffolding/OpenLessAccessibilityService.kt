package com.openless.app

import android.accessibilityservice.AccessibilityService
import android.content.Context
import android.content.Intent
import android.graphics.Rect
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
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
        markServiceAlive()
        updateKeyboardOverlayState()
        scheduleKeyboardOverlayRefresh()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (event == null) return
        markServiceAlive()
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
        if (!canDrawOverlays()) {
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
        try {
            startService(intent)
        } catch (error: Throwable) {
            Log.w(TAG, "send keyboard overlay event failed", error)
        }
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
        return OpenLessAndroidPreferences.overlayTriggerMode(this) == "keyboard"
    }

    private fun canDrawOverlays(): Boolean {
        return if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.M) {
            Settings.canDrawOverlays(this)
        } else {
            true
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

    private fun markServiceAlive() {
        getSharedPreferences(PREFS_NAME, prefsMode())
            .edit()
            .putLong(PREF_KEY_LAST_HEARTBEAT, System.currentTimeMillis())
            .apply()
    }

    companion object {
        @Volatile
        var instance: OpenLessAccessibilityService? = null
            private set

        @JvmStatic
        fun pasteToFocusedField(): Boolean {
            instance?.let { return it.performPasteToFocusedField() }
            return sendPasteRequestToAccessibilityProcess()
        }

        @JvmStatic
        fun isEnabled(context: Context): Boolean {
            val enabled = Settings.Secure.getInt(
                context.contentResolver,
                Settings.Secure.ACCESSIBILITY_ENABLED,
                0,
            ) == 1
            if (!enabled) return false
            val services = Settings.Secure.getString(
                context.contentResolver,
                Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            ) ?: return false
            return services.contains("${context.packageName}/${OpenLessAccessibilityService::class.java.name}")
        }

        @JvmStatic
        fun isOperational(context: Context): Boolean {
            if (!isEnabled(context)) return false
            val lastHeartbeat = context
                .getSharedPreferences(PREFS_NAME, prefsMode())
                .getLong(PREF_KEY_LAST_HEARTBEAT, 0L)
            if (lastHeartbeat <= 0L) return false
            return System.currentTimeMillis() - lastHeartbeat <= HEARTBEAT_STALE_MS
        }

        internal fun performPasteFromCommand(): Boolean {
            return instance?.performPasteToFocusedField() == true
        }

        private fun sendPasteRequestToAccessibilityProcess(): Boolean {
            val context = OpenLessAppContext.context ?: return false
            if (!isOperational(context)) return false
            return try {
                val intent = Intent(context, OpenLessAccessibilityCommandReceiver::class.java).apply {
                    action = OpenLessAccessibilityCommandReceiver.ACTION_PASTE
                }
                context.sendBroadcast(intent)
                true
            } catch (error: Throwable) {
                Log.w(TAG, "send accessibility paste request failed", error)
                false
            }
        }

        @Suppress("DEPRECATION")
        private fun prefsMode(): Int = Context.MODE_PRIVATE or Context.MODE_MULTI_PROCESS

        private val KEYBOARD_REFRESH_DELAYS_MS = longArrayOf(120L, 360L, 900L, 1600L)
        private const val TAG = "OpenLessAccessibility"
        private const val PREFS_NAME = "openless_accessibility"
        private const val PREF_KEY_LAST_HEARTBEAT = "last_heartbeat"
        private const val HEARTBEAT_STALE_MS = 15_000L
    }
}
