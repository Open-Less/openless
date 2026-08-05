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
    private var lastEditableFocus: AccessibilityNodeInfo? = null

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
        updateKeyboardOverlayState()
        scheduleKeyboardOverlayRefresh()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (event == null) return
        when (event.eventType) {
            AccessibilityEvent.TYPE_VIEW_CLICKED -> rememberFocusedEditable(event)
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED,
            AccessibilityEvent.TYPE_WINDOWS_CHANGED -> {
                rememberFocusedEditable(event)
                updateKeyboardOverlayState()
                scheduleKeyboardOverlayRefresh()
            }
            AccessibilityEvent.TYPE_VIEW_FOCUSED,
            AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED -> {
                rememberFocusedEditable(event)
                updateKeyboardOverlayState()
                scheduleKeyboardOverlayRefresh()
            }
        }
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        mainHandler.removeCallbacks(keyboardRefreshRunnable)
        invalidateEditableCache()
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

    private fun performPasteToFocusedFieldInternal(pasteText: String? = null): AccessibilityPasteResult {
        val target = findEditableTarget()
        if (target == null) {
            return AccessibilityPasteResult.NO_FOCUSED_EDITOR
        }
        return try {
            target.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
            val ok = pasteWithRetryOrSetText(target, pasteText)
            if (ok) {
                AccessibilityPasteResult.SUCCESS
            } else {
                AccessibilityPasteResult.PASTE_REJECTED
            }
        } finally {
            target.recycle()
        }
    }

    private fun rememberFocusedEditable(event: AccessibilityEvent) {
        val source = event.source ?: return
        try {
            if (OpenLessAccessibilityTarget.isPasteTarget(source)) {
                cacheEditableTarget(source)
                return
            }
            editableFocusedNode(source, AccessibilityNodeInfo.FOCUS_INPUT)?.let { focused ->
                cacheEditableTarget(focused)
                focused.recycle()
                return
            }
            editableFocusedNode(source, AccessibilityNodeInfo.FOCUS_ACCESSIBILITY)?.let { focused ->
                cacheEditableTarget(focused)
                focused.recycle()
            }
        } finally {
            source.recycle()
        }
    }

    private fun invalidateEditableCache() {
        lastEditableFocus?.recycle()
        lastEditableFocus = null
    }

    private fun findEditableTarget(): AccessibilityNodeInfo? {
        lastEditableFocus?.let { cached ->
            if (cached.refresh() && OpenLessAccessibilityTarget.isPasteTarget(cached)) {
                return AccessibilityNodeInfo.obtain(cached)
            }
        }

        val activeRoot = rootInActiveWindow
        val activePackage = activeRoot?.packageName?.toString()
        var pasteTargetsInActive = 0
        if (activeRoot != null) {
            try {
                pasteTargetsInActive = countPasteTargetsInTree(activeRoot, 0)
                findEditableInRoot(activeRoot)?.let { found ->
                    return found
                }
            } finally {
                activeRoot.recycle()
            }
        }

        for (window in windows) {
            if (window.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD) {
                continue
            }
            val root = window.root ?: continue
            try {
                findEditableInRoot(root)?.let { found ->
                    return found
                }
            } finally {
                root.recycle()
            }
        }

        Log.w(
            TAG,
            "findEditableTarget failed activeRoot=$activePackage windowCount=${windows.size} hadCache=${lastEditableFocus != null} pasteTargetsInActive=$pasteTargetsInActive",
        )
        invalidateEditableCache()
        return null
    }

    private fun findEditableInRoot(root: AccessibilityNodeInfo): AccessibilityNodeInfo? {
        editableFocusedNode(root, AccessibilityNodeInfo.FOCUS_INPUT)?.let { fresh ->
            cacheEditableTarget(fresh)
            return fresh
        }
        editableFocusedNode(root, AccessibilityNodeInfo.FOCUS_ACCESSIBILITY)?.let { fresh ->
            cacheEditableTarget(fresh)
            return fresh
        }

        lastEditableFocus?.let { cached ->
            if (OpenLessAccessibilityTarget.isValidCachedEditable(cached, root)) {
                return AccessibilityNodeInfo.obtain(cached)
            }
        }

        return findEditableInTree(root, 0)?.also { found ->
            cacheEditableTarget(found)
        }
    }

    private fun editableFocusedNode(root: AccessibilityNodeInfo, focusType: Int): AccessibilityNodeInfo? {
        val focused = root.findFocus(focusType) ?: return null
        return try {
            if (OpenLessAccessibilityTarget.isPasteTarget(focused)) {
                AccessibilityNodeInfo.obtain(focused)
            } else {
                null
            }
        } finally {
            focused.recycle()
        }
    }

    private fun findEditableInTree(node: AccessibilityNodeInfo, depth: Int): AccessibilityNodeInfo? {
        if (depth > MAX_EDITABLE_SEARCH_DEPTH) return null
        var firstCandidate: AccessibilityNodeInfo? = null
        if (OpenLessAccessibilityTarget.isPasteTarget(node)) {
            if (node.isFocused) {
                return AccessibilityNodeInfo.obtain(node)
            }
            firstCandidate = AccessibilityNodeInfo.obtain(node)
        }
        for (index in 0 until node.childCount) {
            val child = node.getChild(index) ?: continue
            try {
                findEditableInTree(child, depth + 1)?.let { found ->
                    firstCandidate?.recycle()
                    return found
                }
            } finally {
                child.recycle()
            }
        }
        return firstCandidate
    }

    private fun countPasteTargetsInTree(node: AccessibilityNodeInfo, depth: Int): Int {
        if (depth > MAX_EDITABLE_SEARCH_DEPTH) return 0
        var count = if (OpenLessAccessibilityTarget.isPasteTarget(node)) 1 else 0
        for (index in 0 until node.childCount) {
            val child = node.getChild(index) ?: continue
            try {
                count += countPasteTargetsInTree(child, depth + 1)
            } finally {
                child.recycle()
            }
        }
        return count
    }

    private fun cacheEditableTarget(target: AccessibilityNodeInfo) {
        lastEditableFocus?.recycle()
        lastEditableFocus = AccessibilityNodeInfo.obtain(target)
    }

    private fun pasteWithRetryOrSetText(target: AccessibilityNodeInfo, pasteText: String? = null): Boolean {
        val effectiveText = pasteText?.takeIf { it.isNotEmpty() } ?: clipboardText()
        if (effectiveText.isEmpty()) {
            return false
        }
        val beforeText = nodeText(target)
        sleepQuietly(PASTE_INITIAL_DELAY_MS)
        repeat(PASTE_RETRY_COUNT) { attempt ->
            if (target.performAction(AccessibilityNodeInfo.ACTION_PASTE)) {
                sleepQuietly(PASTE_VERIFY_DELAY_MS)
                if (target.refresh() && pasteAppearsApplied(beforeText, nodeText(target), effectiveText)) {
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
        val setText = appendClipboardTextWithSetText(target, effectiveText)
        sleepQuietly(PASTE_VERIFY_DELAY_MS)
        val verified =
            setText &&
                target.refresh() &&
                pasteAppearsApplied(beforeText, nodeText(target), effectiveText)
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

    private fun appendClipboardTextWithSetText(target: AccessibilityNodeInfo, pasteText: String): Boolean {
        if (target.isPassword) return false
        val existingText = target.text?.toString().orEmpty()
        val args = Bundle().apply {
            putCharSequence(
                AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                existingText + pasteText,
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
            return pasteToFocusedFieldWithResult("") == AccessibilityPasteResult.SUCCESS
        }

        @JvmStatic
        @Keep
        fun pasteToFocusedFieldResult(text: String): String {
            return pasteToFocusedFieldWithResult(text).reason
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

        internal fun performPasteFromCommand(pasteText: String? = null): AccessibilityPasteResult {
            return instance?.performPasteToFocusedFieldInternal(pasteText)
                ?: AccessibilityPasteResult.SERVICE_NOT_CONNECTED
        }

        internal fun captureSelectedTextFromCommand(): String? {
            return instance?.captureSelectedTextFromFocusedNode()
        }

        private fun pasteToFocusedFieldWithResult(pasteText: String): AccessibilityPasteResult {
            instance?.let { return it.performPasteToFocusedFieldInternal(pasteText) }
            return sendAccessibilityCommand(
                OpenLessAccessibilityCommandReceiver.ACTION_PASTE,
                PASTE_COMMAND_TIMEOUT_MS,
                pasteText,
            )
        }

        private fun sendAccessibilityCommand(
            action: String,
            timeoutMs: Long = PASTE_COMMAND_TIMEOUT_MS,
            pasteText: String? = null,
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
                    if (!pasteText.isNullOrEmpty()) {
                        putExtra(OpenLessAccessibilityCommandReceiver.EXTRA_PASTE_TEXT, pasteText)
                    }
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
        private const val MAX_EDITABLE_SEARCH_DEPTH = 8
        private const val TAG = "OpenLessAccessibility"
    }
}
