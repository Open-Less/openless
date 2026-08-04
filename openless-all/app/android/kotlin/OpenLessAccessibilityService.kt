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
import androidx.annotation.Keep
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

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
            AccessibilityEvent.TYPE_WINDOWS_CHANGED -> {
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

    private fun performPasteToFocusedFieldInternal(): AccessibilityPasteResult {
        val root = rootInActiveWindow ?: return AccessibilityPasteResult.NO_FOCUSED_EDITOR
        return try {
            val focused = root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
                ?: root.findFocus(AccessibilityNodeInfo.FOCUS_ACCESSIBILITY)
                ?: return AccessibilityPasteResult.NO_FOCUSED_EDITOR
            try {
                focused.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
                if (pasteWithRetryOrSetText(focused)) {
                    AccessibilityPasteResult.SUCCESS
                } else {
                    AccessibilityPasteResult.PASTE_REJECTED
                }
            } finally {
                focused.recycle()
            }
        } finally {
            root.recycle()
        }
    }

    private fun pasteWithRetryOrSetText(target: AccessibilityNodeInfo): Boolean {
        val clipboardText = clipboardText().takeIf { it.isNotEmpty() } ?: return false
        val beforeText = nodeText(target)
        sleepQuietly(PASTE_INITIAL_DELAY_MS)
        repeat(PASTE_RETRY_COUNT) { attempt ->
            if (target.performAction(AccessibilityNodeInfo.ACTION_PASTE)) {
                sleepQuietly(PASTE_VERIFY_DELAY_MS)
                if (target.refresh() && pasteAppearsApplied(beforeText, nodeText(target), clipboardText)) {
                    Log.i(
                        TAG,
                        "paste=true verified attempt=${attempt + 1} package=${target.packageName}",
                    )
                    return true
                }
                Log.w(
                    TAG,
                    "paste=unverified attempt=${attempt + 1} package=${target.packageName}",
                )
            }
            sleepQuietly(PASTE_RETRY_DELAY_MS)
        }
        val setText = appendClipboardTextWithSetText(target)
        sleepQuietly(PASTE_VERIFY_DELAY_MS)
        val verified =
            setText &&
                target.refresh() &&
                pasteAppearsApplied(beforeText, nodeText(target), clipboardText)
        Log.i(
            TAG,
            "paste=false setText=$setText verified=$verified package=${target.packageName}",
        )
        return verified
    }

    private fun nodeText(target: AccessibilityNodeInfo): String {
        return target.text?.toString().orEmpty()
    }

    private fun pasteAppearsApplied(
        beforeText: String,
        afterText: String,
        clipboardText: String,
    ): Boolean {
        return OpenLessPasteVerification.pasteAppearsApplied(beforeText, afterText, clipboardText)
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
        try {
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
        } finally {
            root.recycle()
        }
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

    companion object {
        /** Matches [isEnabled] / Settings.Secure component id format (full class name). */
        @JvmStatic
        fun serviceComponentId(): String =
            "${BuildConfig.APPLICATION_ID}/${OpenLessAccessibilityService::class.java.name}"

        @Volatile
        var instance: OpenLessAccessibilityService? = null
            private set

        @JvmStatic
        @Keep
        fun pasteToFocusedField(): Boolean {
            return pasteToFocusedFieldWithResult() == AccessibilityPasteResult.SUCCESS
        }

        @JvmStatic
        @Keep
        fun pasteToFocusedFieldResult(): String {
            return pasteToFocusedFieldWithResult().reason
        }

        @JvmStatic
        @Keep
        fun captureSelectedText(): String {
            instance?.let { return it.captureSelectedTextFromFocusedNode() }
            return captureSelectedTextFromAccessibilityProcess()
        }

        @JvmStatic
        @Keep
        fun isEnabled(context: Context): Boolean {
            val enabled = Settings.Secure.getInt(
                context.contentResolver,
                Settings.Secure.ACCESSIBILITY_ENABLED,
                0,
            ) == 1
            if (!enabled) {
                return false
            }
            val services = Settings.Secure.getString(
                context.contentResolver,
                Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            ) ?: return false
            return OpenLessAccessibilityComponentIds.enabledListContains(
                services,
                serviceComponentId(),
            )
        }

        @JvmStatic
        @Keep
        fun pingAccessibilityProcess(context: Context): Boolean {
            if (!isEnabled(context)) return false
            if (instance != null) {
                return true
            }
            val pingResult = sendAccessibilityCommand(
                OpenLessAccessibilityCommandReceiver.ACTION_PING,
                PING_COMMAND_TIMEOUT_MS,
            )
            return pingResult == AccessibilityPasteResult.SUCCESS
        }

        /** @deprecated Use [pingAccessibilityProcess] for UI; paste no longer gates on this. */
        @JvmStatic
        fun isOperational(context: Context): Boolean {
            return pingAccessibilityProcess(context)
        }

        internal fun performPasteFromCommand(): AccessibilityPasteResult {
            return instance?.performPasteToFocusedFieldInternal()
                ?: AccessibilityPasteResult.SERVICE_NOT_CONNECTED
        }

        internal fun captureSelectedTextFromCommand(): String? {
            return instance?.captureSelectedTextFromFocusedNode()
        }

        private fun pasteToFocusedFieldWithResult(): AccessibilityPasteResult {
            instance?.let { return it.performPasteToFocusedFieldInternal() }
            return sendAccessibilityCommand(OpenLessAccessibilityCommandReceiver.ACTION_PASTE)
        }

        private fun sendAccessibilityCommand(
            action: String,
            timeoutMs: Long = PASTE_COMMAND_TIMEOUT_MS,
        ): AccessibilityPasteResult {
            val context = OpenLessAppContext.context ?: return AccessibilityPasteResult.SERVICE_NOT_CONNECTED
            val latch = CountDownLatch(1)
            val resultHolder = AtomicReference(AccessibilityPasteResult.TIMEOUT)
            val receiver = object : ResultReceiver(null) {
                override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
                    resultHolder.set(AccessibilityPasteResult.fromCode(resultCode))
                    latch.countDown()
                }
            }
            var broadcastSent = false
            return try {
                val intent = Intent(context, OpenLessAccessibilityCommandReceiver::class.java).apply {
                    this.action = action
                    putExtra(OpenLessAccessibilityCommandReceiver.EXTRA_RESULT_RECEIVER, receiver)
                }
                context.sendBroadcast(intent)
                broadcastSent = true
                try {
                    if (!latch.await(timeoutMs, TimeUnit.MILLISECONDS)) {
                        Log.w(TAG, "accessibility command timed out action=$action")
                        AccessibilityPasteResult.TIMEOUT
                    } else {
                        resultHolder.get()
                    }
                } catch (error: InterruptedException) {
                    Thread.currentThread().interrupt()
                    Log.w(TAG, "accessibility command interrupted after broadcast action=$action", error)
                    AccessibilityPasteResult.IPC_PROTOCOL_ERROR
                }
            } catch (error: Throwable) {
                Log.w(
                    TAG,
                    "send accessibility command failed action=$action broadcastSent=$broadcastSent",
                    error,
                )
                if (broadcastSent) {
                    AccessibilityPasteResult.IPC_PROTOCOL_ERROR
                } else {
                    AccessibilityPasteResult.SERVICE_NOT_CONNECTED
                }
            }
        }

        private fun captureSelectedTextFromAccessibilityProcess(): String {
            val context = OpenLessAppContext.context ?: return ""
            val latch = CountDownLatch(1)
            val selectedText = AtomicReference("")
            val receiver = object : ResultReceiver(null) {
                override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
                    if (resultCode == AccessibilityPasteResult.SUCCESS.code) {
                        selectedText.set(
                            resultData
                                ?.getString(OpenLessAccessibilityCommandReceiver.EXTRA_SELECTED_TEXT)
                                .orEmpty(),
                        )
                    }
                    latch.countDown()
                }
            }
            return try {
                val intent = Intent(context, OpenLessAccessibilityCommandReceiver::class.java).apply {
                    action = OpenLessAccessibilityCommandReceiver.ACTION_CAPTURE_SELECTED_TEXT
                    putExtra(OpenLessAccessibilityCommandReceiver.EXTRA_RESULT_RECEIVER, receiver)
                }
                context.sendBroadcast(intent)
                if (latch.await(SELECTION_COMMAND_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                    selectedText.get()
                } else {
                    Log.w(TAG, "accessibility selection command timed out")
                    ""
                }
            } catch (error: InterruptedException) {
                Thread.currentThread().interrupt()
                Log.w(TAG, "accessibility selection command interrupted", error)
                ""
            } catch (error: Throwable) {
                Log.w(TAG, "send accessibility selection command failed", error)
                ""
            }
        }

        private val KEYBOARD_REFRESH_DELAYS_MS = longArrayOf(120L, 360L, 900L, 1600L)
        private const val PASTE_INITIAL_DELAY_MS = 50L
        private const val PASTE_VERIFY_DELAY_MS = 80L
        private const val PASTE_RETRY_COUNT = 3
        private const val PASTE_RETRY_DELAY_MS = 80L
        private const val PASTE_COMMAND_TIMEOUT_MS = 800L
        private const val PING_COMMAND_TIMEOUT_MS = 500L
        private const val SELECTION_COMMAND_TIMEOUT_MS = 500L
        private const val TAG = "OpenLessAccessibility"
    }
}
