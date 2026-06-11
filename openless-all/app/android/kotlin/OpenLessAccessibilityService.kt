package com.openless.app

import android.accessibilityservice.AccessibilityService
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Rect
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ResultReceiver
import android.provider.Settings
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Detects IME windows for overlay keyboard trigger mode and performs paste insertion.
 */
class OpenLessAccessibilityService : AccessibilityService() {
    private val mainHandler = Handler(Looper.getMainLooper())
    private val heartbeatRunnable = object : Runnable {
        override fun run() {
            markServiceAlive()
            mainHandler.postDelayed(this, HEARTBEAT_INTERVAL_MS)
        }
    }
    private val keyboardRefreshRunnable = Runnable { updateKeyboardOverlayState() }
    private var lastEditableFocus: AccessibilityNodeInfo? = null

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
        startHeartbeat()
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
                rememberFocusedEditable(event)
                updateKeyboardOverlayState()
                scheduleKeyboardOverlayRefresh()
            }
        }
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        mainHandler.removeCallbacks(heartbeatRunnable)
        mainHandler.removeCallbacks(keyboardRefreshRunnable)
        lastEditableFocus?.recycle()
        lastEditableFocus = null
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

    private fun startHeartbeat() {
        mainHandler.removeCallbacks(heartbeatRunnable)
        heartbeatRunnable.run()
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
            Log.i(TAG, "keyboard overlay event visible=${imeBounds != null} bounds=$imeBounds")
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
        return OpenLessAndroidPreferences.isKeyboardOverlayTrigger(this)
    }

    private fun canDrawOverlays(): Boolean {
        return if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.M) {
            Settings.canDrawOverlays(this)
        } else {
            true
        }
    }

    private fun performPasteToFocusedField(): Boolean {
        val target = findEditableTarget() ?: return false
        return try {
            target.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
            pasteWithRetryOrSetText(target)
        } finally {
            target.recycle()
        }
    }

    private fun rememberFocusedEditable(event: AccessibilityEvent) {
        val source = event.source ?: return
        try {
            if (!source.isEditable) return
            lastEditableFocus?.recycle()
            lastEditableFocus = AccessibilityNodeInfo.obtain(source)
        } finally {
            source.recycle()
        }
    }

    private fun findEditableTarget(): AccessibilityNodeInfo? {
        lastEditableFocus?.let { cached ->
            if (cached.refresh() && cached.isEditable) {
                return AccessibilityNodeInfo.obtain(cached)
            }
        }
        val root = rootInActiveWindow ?: return null
        editableFocusedNode(root, AccessibilityNodeInfo.FOCUS_INPUT)?.let { return it }
        editableFocusedNode(root, AccessibilityNodeInfo.FOCUS_ACCESSIBILITY)?.let { return it }
        return findEditableInTree(root, 0)
    }

    private fun editableFocusedNode(root: AccessibilityNodeInfo, focusType: Int): AccessibilityNodeInfo? {
        val focused = root.findFocus(focusType) ?: return null
        if (focused.isEditable) {
            return focused
        }
        focused.recycle()
        return null
    }

    private fun findEditableInTree(node: AccessibilityNodeInfo, depth: Int): AccessibilityNodeInfo? {
        if (depth > MAX_EDITABLE_SEARCH_DEPTH) return null
        var firstEditable: AccessibilityNodeInfo? = null
        if (node.isEditable) {
            if (node.isFocused) {
                return AccessibilityNodeInfo.obtain(node)
            }
            firstEditable = AccessibilityNodeInfo.obtain(node)
        }
        for (index in 0 until node.childCount) {
            val child = node.getChild(index) ?: continue
            try {
                findEditableInTree(child, depth + 1)?.let { found ->
                    firstEditable?.recycle()
                    return found
                }
            } finally {
                child.recycle()
            }
        }
        return firstEditable
    }

    private fun pasteWithRetryOrSetText(target: AccessibilityNodeInfo): Boolean {
        sleepQuietly(PASTE_INITIAL_DELAY_MS)
        repeat(PASTE_RETRY_COUNT) { attempt ->
            if (target.performAction(AccessibilityNodeInfo.ACTION_PASTE)) {
                Log.i(TAG, "paste=true attempt=${attempt + 1} package=${target.packageName}")
                return true
            }
            sleepQuietly(PASTE_RETRY_DELAY_MS)
        }
        val setText = appendClipboardTextWithSetText(target)
        Log.i(TAG, "paste=false setText=$setText package=${target.packageName}")
        return setText
    }

    private fun appendClipboardTextWithSetText(target: AccessibilityNodeInfo): Boolean {
        if (target.isPassword) return false
        val clipboardText = clipboardText().takeIf { it.isNotEmpty() } ?: return false
        val existingText = target.text?.toString().orEmpty()
        val args = Bundle().apply {
            putCharSequence(
                AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                existingText + clipboardText,
            )
        }
        return target.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
    }

    private fun clipboardText(): String {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return ""
        val clip = clipboard.primaryClip ?: return ""
        if (clip.itemCount <= 0) return ""
        return clip.getItemAt(0)?.coerceToText(this)?.toString().orEmpty()
    }

    private fun sleepQuietly(delayMs: Long) {
        try {
            Thread.sleep(delayMs)
        } catch (_: InterruptedException) {
            Thread.currentThread().interrupt()
        }
    }

    private fun captureSelectedTextFromFocusedNode(): String {
        val root = rootInActiveWindow ?: return ""
        val focused = root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
            ?: root.findFocus(AccessibilityNodeInfo.FOCUS_ACCESSIBILITY)
        focused?.let {
            return try {
                selectedTextFromNode(it)
            } finally {
                it.recycle()
            }
        }
        return selectedTextFromTree(root)
    }

    private fun selectedTextFromTree(node: AccessibilityNodeInfo?): String {
        if (node == null) return ""
        selectedTextFromNode(node).takeIf { it.isNotBlank() }?.let { return it }
        for (index in 0 until node.childCount) {
            val child = node.getChild(index) ?: continue
            try {
                selectedTextFromTree(child).takeIf { it.isNotBlank() }?.let { return it }
            } finally {
                child.recycle()
            }
        }
        return ""
    }

    private fun selectedTextFromNode(node: AccessibilityNodeInfo): String {
        val text = node.text?.toString() ?: return ""
        val start = node.textSelectionStart
        val end = node.textSelectionEnd
        if (start < 0 || end < 0 || start == end) return ""
        val from = minOf(start, end).coerceIn(0, text.length)
        val to = maxOf(start, end).coerceIn(0, text.length)
        if (from >= to) return ""
        return text.substring(from, to)
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
        fun captureSelectedText(): String {
            return instance?.captureSelectedTextFromFocusedNode().orEmpty()
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
            val latch = CountDownLatch(1)
            val success = AtomicBoolean(false)
            val receiver = object : ResultReceiver(null) {
                override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
                    success.set(resultCode == OpenLessAccessibilityCommandReceiver.RESULT_PASTE_SUCCESS)
                    latch.countDown()
                }
            }
            return try {
                val intent = Intent(context, OpenLessAccessibilityCommandReceiver::class.java).apply {
                    action = OpenLessAccessibilityCommandReceiver.ACTION_PASTE
                    putExtra(OpenLessAccessibilityCommandReceiver.EXTRA_RESULT_RECEIVER, receiver)
                }
                context.sendBroadcast(intent)
                if (!latch.await(PASTE_COMMAND_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                    Log.w(TAG, "accessibility paste result timed out")
                    return false
                }
                success.get()
            } catch (error: Throwable) {
                Log.w(TAG, "send accessibility paste request failed", error)
                false
            }
        }

        @Suppress("DEPRECATION")
        private fun prefsMode(): Int = Context.MODE_PRIVATE or Context.MODE_MULTI_PROCESS

        private val KEYBOARD_REFRESH_DELAYS_MS = longArrayOf(120L, 360L, 900L, 1600L)
        private const val MAX_EDITABLE_SEARCH_DEPTH = 4
        private const val PASTE_INITIAL_DELAY_MS = 50L
        private const val PASTE_RETRY_COUNT = 3
        private const val PASTE_RETRY_DELAY_MS = 80L
        private const val PASTE_COMMAND_TIMEOUT_MS = 800L
        private const val TAG = "OpenLessAccessibility"
        private const val PREFS_NAME = "openless_accessibility"
        private const val PREF_KEY_LAST_HEARTBEAT = "last_heartbeat"
        private const val HEARTBEAT_INTERVAL_MS = 5_000L
        private const val HEARTBEAT_STALE_MS = 15_000L
    }
}
