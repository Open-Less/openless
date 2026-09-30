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

/** Detects IME windows for overlay keyboard trigger mode and performs paste insertion. */
class OpenLessAccessibilityService : AccessibilityService() {
    private val mainHandler = Handler(Looper.getMainLooper())
    private val keyboardRefreshRunnable = Runnable { updateKeyboardOverlayState() }
    private var lastEditableFocus: AccessibilityNodeInfo? = null
    private var vocabularyGeneration = 0L
    private var vocabularyNode: AccessibilityNodeInfo? = null
    private var vocabularyDeadline = 0L
    private var vocabularyStarted = 0L
    private var vocabularyCard: android.view.View? = null
    private val vocabularyRead = Runnable { readVocabularyText() }
    private val vocabularyTimeout = Runnable { stopVocabularyObservation() }
    private var vocabularyCardTimeout: Runnable? = null
    @Volatile private var vocabularyEpoch = 0L

    private fun postVocabulary(action: () -> Unit) {
        val epoch = vocabularyEpoch
        mainHandler.post {
            if (instance === this && vocabularyEpoch == epoch) action()
        }
    }

    private fun invalidateVocabularyLifecycle() {
        vocabularyEpoch++
        stopVocabularyObservation()
        hideVocabularyCard()
    }

    private var vocabularyCardRequest = 0L

    private fun requestVocabulary(operation: String, data: Bundle = Bundle(), observation: Boolean = false, callback: (Boolean, Bundle?) -> Unit) {
        val epoch = vocabularyEpoch
        val generation = vocabularyGeneration
        OpenLessVocabularyIpc.request(this, operation, data) { ok, response ->
            if (instance === this && vocabularyEpoch == epoch && (!observation || vocabularyGeneration == generation)) {
                callback(ok, response)
            }
        }
    }

    private fun refreshVocabularyCard() {
        val request = ++vocabularyCardRequest
        requestVocabulary("pending") { ok, response ->
            if (request == vocabularyCardRequest) showVocabularyCard(if (ok) response?.getString("json") ?: "[]" else "[]")
        }
    }

    private fun startVocabularyObservation(generation: Long) {
        if (generation == 0L || vocabularyGeneration == generation) return
        stopVocabularyObservation()
        vocabularyGeneration = generation
        requestVocabulary("active", Bundle().apply { putLong("generation", generation) }, true) { active, response ->
            val deadline = response?.getLong("deadline") ?: 0L
            if (active && deadline > android.os.SystemClock.elapsedRealtime()) {
                captureVocabularyNode(generation, deadline)
            } else stopVocabularyObservation()
        }
    }

    private fun captureVocabularyNode(generation: Long, deadline: Long) {
        val root = rootInActiveWindow ?: run { stopVocabularyObservation(); return }
        val node = try { root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT) } finally { root.recycle() }
        if (node == null) { stopVocabularyObservation(); return }
        val packageName = node.packageName?.toString()?.lowercase().orEmpty()
        if (!node.isEditable || node.isPassword || packageName.isEmpty() || packageName == this.packageName ||
            listOf("keepass", "bitwarden", "1password", "lastpass", "dashlane", "termux", "password").any { packageName.contains(it) }) {
            node.recycle()
            stopVocabularyObservation()
            return
        }
        vocabularyGeneration = generation
        vocabularyNode = node
        vocabularyStarted = android.os.SystemClock.elapsedRealtime()
        vocabularyDeadline = deadline
        val remaining = deadline - vocabularyStarted
        if (remaining <= 0L) { stopVocabularyObservation(); return }
        mainHandler.post(vocabularyRead)
        mainHandler.postDelayed(vocabularyTimeout, remaining)
    }

    private fun stopVocabularyObservation() {
        val generation = vocabularyGeneration
        mainHandler.removeCallbacks(vocabularyRead)
        mainHandler.removeCallbacks(vocabularyTimeout)
        vocabularyNode?.recycle()
        vocabularyNode = null
        vocabularyGeneration = 0L
        if (generation != 0L) {
            OpenLessVocabularyIpc.request(this, "stop", Bundle().apply { putLong("generation", generation) }) { _, _ -> }
        }
    }

    private fun readVocabularyText() {
        if (vocabularyNode == null) return
        requestVocabulary("active", Bundle().apply { putLong("generation", vocabularyGeneration) }, true) { active, _ ->
            if (active) readConfirmedVocabularyText() else stopVocabularyObservation()
        }
    }

    private fun readConfirmedVocabularyText() {
        val node = vocabularyNode ?: return
        if (android.os.SystemClock.elapsedRealtime() >= vocabularyDeadline || !node.refresh() || !node.isFocused || node.isPassword) {
            stopVocabularyObservation()
            return
        }
        val root = rootInActiveWindow
        val sameWindow = root != null && root.windowId == node.windowId
        root?.recycle()
        if (!sameWindow) { stopVocabularyObservation(); return }
        val text = node.text?.toString()
        if (text == null || text.length > 20_000) { stopVocabularyObservation(); return }
        requestVocabulary("observe", Bundle().apply {
            putLong("generation", vocabularyGeneration)
            putString("text", text)
        }, true) { alive, _ ->
            if (!alive) stopVocabularyObservation()
            else if (android.os.SystemClock.elapsedRealtime() - vocabularyStarted < 1_000L) {
                mainHandler.postDelayed(vocabularyRead, 100L)
            }
        }
    }

    private fun vocabularyEvent(event: AccessibilityEvent) {
        val node = vocabularyNode ?: return
        if (event.eventType == AccessibilityEvent.TYPE_VIEW_FOCUSED ||
            event.eventType == AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED ||
            event.eventType == AccessibilityEvent.TYPE_WINDOWS_CHANGED) {
            if (!node.refresh() || !node.isFocused || node.isPassword) stopVocabularyObservation()
        }
        if (event.eventType != AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED) return
        val source = event.source ?: return
        try {
            if (source != node) return
            if (event.isPassword || source.isPassword) { stopVocabularyObservation(); return }
            mainHandler.removeCallbacks(vocabularyRead)
            mainHandler.postDelayed(vocabularyRead, 700L)
        } finally { source.recycle() }
    }

    private fun hideVocabularyCard() {
        vocabularyCardTimeout?.let { mainHandler.removeCallbacks(it) }
        vocabularyCardTimeout = null
        vocabularyCard?.let { view ->
            try { (getSystemService(WINDOW_SERVICE) as android.view.WindowManager).removeView(view) } catch (_: Exception) { }
        }
        vocabularyCard = null
    }

    private fun showVocabularyCard(json: String) {
        hideVocabularyCard()
        val entries = try { org.json.JSONArray(json) } catch (_: Exception) { return }
        val now = System.currentTimeMillis()
        val pending = (0 until entries.length()).map { entries.getJSONObject(it) }.filter { it.optLong("expiresAtMs") > now }
        if (pending.isEmpty()) return
        val layout = android.widget.LinearLayout(this).apply {
            orientation = android.widget.LinearLayout.VERTICAL
            setPadding(16, 12, 16, 12)
            setBackgroundColor(android.graphics.Color.rgb(38, 38, 42))
            addView(android.widget.TextView(context).apply { text = "记住这个词？"; setTextColor(android.graphics.Color.WHITE) })
        }
        for (entry in pending) {
            val row = android.widget.LinearLayout(this)
            row.addView(android.widget.TextView(this).apply {
                text = entry.optString("pattern") + " → " + entry.optString("replacement")
                setTextColor(android.graphics.Color.WHITE)
                maxLines = 2
            }, android.widget.LinearLayout.LayoutParams(0, -2, 1f))
            for (accept in listOf(true, false)) {
                row.addView(android.widget.Button(this).apply {
                    text = if (accept) "✓" else "×"
                    contentDescription = if (accept) "加入词典" else "忽略"
                    setOnClickListener {
                        isEnabled = false
                        requestVocabulary("resolve", Bundle().apply {
                            putString("id", entry.getString("id"))
                            putBoolean("accept", accept)
                        }) { ok, _ ->
                            isEnabled = true
                            if (ok) refreshVocabularyCard()
                            else android.widget.Toast.makeText(this@OpenLessAccessibilityService, "保存失败，请重试", android.widget.Toast.LENGTH_SHORT).show()
                        }
                    }
                }, android.widget.LinearLayout.LayoutParams((56 * resources.displayMetrics.density).toInt(), -2))
            }
            layout.addView(row)
        }
        val params = android.view.WindowManager.LayoutParams(
            (340 * resources.displayMetrics.density).toInt().coerceAtMost(resources.displayMetrics.widthPixels),
            android.view.WindowManager.LayoutParams.WRAP_CONTENT,
            android.view.WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            android.view.WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE,
            android.graphics.PixelFormat.TRANSLUCENT
        ).apply { gravity = android.view.Gravity.TOP or android.view.Gravity.END; y = (48 * resources.displayMetrics.density).toInt() }
        try {
            (getSystemService(WINDOW_SERVICE) as android.view.WindowManager).addView(layout, params)
            vocabularyCard = layout
            val timeout = Runnable { refreshVocabularyCard() }
            vocabularyCardTimeout = timeout
            mainHandler.postDelayed(timeout, (pending.minOf { it.getLong("expiresAtMs") } - now).coerceAtLeast(1L))
        } catch (_: Exception) { hideVocabularyCard() }
    }

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
        vocabularyEpoch++
        refreshVocabularyCard()
        requestVocabulary("current") { ok, response ->
            val generation = if (ok) response?.getLong("generation", 0L) ?: 0L else 0L
            if (vocabularyGeneration == 0L) startVocabularyObservation(generation)
        }
        updateKeyboardOverlayState()
        scheduleKeyboardOverlayRefresh()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (event == null) return
        vocabularyEvent(event)
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

    override fun onInterrupt() { invalidateVocabularyLifecycle() }

    override fun onDestroy() {
        invalidateVocabularyLifecycle()
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
        val intent =
            Intent(this, OpenLessOverlayService::class.java).apply {
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
        return OpenLessPermissionBridge.canDrawOverlaysSafely(this)
    }

    private fun performPasteToFocusedFieldInternal(
        pasteText: String? = null
    ): AccessibilityPasteResult {
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

    private fun editableFocusedNode(
        root: AccessibilityNodeInfo,
        focusType: Int,
    ): AccessibilityNodeInfo? {
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

    private fun findEditableInTree(
        node: AccessibilityNodeInfo,
        depth: Int,
    ): AccessibilityNodeInfo? {
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

    private fun pasteWithRetryOrSetText(
        target: AccessibilityNodeInfo,
        pasteText: String? = null,
    ): Boolean {
        val effectiveText = pasteText?.takeIf { it.isNotEmpty() } ?: clipboardText()
        if (effectiveText.isEmpty()) {
            return false
        }
        val beforeText = nodeText(target)
        sleepQuietly(PASTE_INITIAL_DELAY_MS)
        repeat(PASTE_RETRY_COUNT) { attempt ->
            if (target.performAction(AccessibilityNodeInfo.ACTION_PASTE)) {
                sleepQuietly(PASTE_VERIFY_DELAY_MS)
                if (
                    target.refresh() &&
                        pasteAppearsApplied(beforeText, nodeText(target), effectiveText)
                ) {
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

    private fun appendClipboardTextWithSetText(
        target: AccessibilityNodeInfo,
        pasteText: String,
    ): Boolean {
        if (target.isPassword) return false
        val existingText = target.text?.toString().orEmpty()
        val args =
            Bundle().apply {
                putCharSequence(
                    AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                    existingText + pasteText,
                )
            }
        return target.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
    }

    private fun clipboardText(): String {
        val clipboard =
            getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return ""
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
            val focused =
                root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
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
        selectedTextFromNode(node)
            .takeIf { it.isNotBlank() }
            ?.let {
                return it
            }
        for (index in 0 until node.childCount) {
            val child = node.getChild(index) ?: continue
            try {
                selectedTextFromTree(child)
                    .takeIf { it.isNotBlank() }
                    ?.let {
                        return it
                    }
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
        private fun sendVocabularyCommand(operation: String, generation: Long = 0L) {
            val context = OpenLessAppContext.context ?: return
            val intent = Intent(context, OpenLessAccessibilityCommandReceiver::class.java).apply {
                action = OpenLessAccessibilityCommandReceiver.ACTION_VOCABULARY
                putExtra("operation", operation)
                putExtra("generation", generation)
            }
            try { context.sendBroadcast(intent) } catch (_: Throwable) { }
        }

        internal fun handleVocabularyCommand(intent: Intent) {
            val service = instance ?: return
            val generation = intent.getLongExtra("generation", 0L)
            service.postVocabulary {
                when (intent.getStringExtra("operation")) {
                    "arm" -> service.startVocabularyObservation(generation)
                    "disarm" -> if (service.vocabularyGeneration == generation) service.stopVocabularyObservation()
                    "refresh" -> service.refreshVocabularyCard()
                }
            }
        }

        @Keep @JvmStatic fun armVocabularyObservation(generation: Long) = sendVocabularyCommand("arm", generation)
        @Keep @JvmStatic fun disarmVocabularyObservation(generation: Long) = sendVocabularyCommand("disarm", generation)
        @Keep @JvmStatic fun showVocabularySuggestions(json: String) = sendVocabularyCommand("refresh")
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
            instance?.let {
                return it.captureSelectedTextFromFocusedNode()
            }
            return captureSelectedTextFromAccessibilityProcess()
        }

        @JvmStatic
        @Keep
        fun isEnabled(context: Context): Boolean {
            val enabled =
                Settings.Secure.getInt(
                    context.contentResolver,
                    Settings.Secure.ACCESSIBILITY_ENABLED,
                    0,
                ) == 1
            if (!enabled) {
                return false
            }
            val services =
                Settings.Secure.getString(
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
            val pingResult =
                sendAccessibilityCommand(
                    OpenLessAccessibilityCommandReceiver.ACTION_PING,
                    PING_COMMAND_TIMEOUT_MS,
                )
            return pingResult == AccessibilityPasteResult.SUCCESS
        }

        internal fun performPasteFromCommand(pasteText: String? = null): AccessibilityPasteResult {
            return instance?.performPasteToFocusedFieldInternal(pasteText)
                ?: AccessibilityPasteResult.SERVICE_NOT_CONNECTED
        }

        internal fun captureSelectedTextFromCommand(): String? {
            return instance?.captureSelectedTextFromFocusedNode()
        }

        private fun pasteToFocusedFieldWithResult(pasteText: String): AccessibilityPasteResult {
            instance?.let {
                return it.performPasteToFocusedFieldInternal(pasteText)
            }
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
            val context =
                OpenLessAppContext.context ?: return AccessibilityPasteResult.SERVICE_NOT_CONNECTED
            val latch = CountDownLatch(1)
            val resultHolder = AtomicReference(AccessibilityPasteResult.TIMEOUT)
            val receiver =
                object : ResultReceiver(null) {
                    override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
                        resultHolder.set(AccessibilityPasteResult.fromCode(resultCode))
                        latch.countDown()
                    }
                }
            var broadcastSent = false
            return try {
                val intent =
                    Intent(context, OpenLessAccessibilityCommandReceiver::class.java).apply {
                        this.action = action
                        putExtra(
                            OpenLessAccessibilityCommandReceiver.EXTRA_RESULT_RECEIVER,
                            receiver,
                        )
                        if (!pasteText.isNullOrEmpty()) {
                            putExtra(
                                OpenLessAccessibilityCommandReceiver.EXTRA_PASTE_TEXT,
                                pasteText,
                            )
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
                    Log.w(
                        TAG,
                        "accessibility command interrupted after broadcast action=$action",
                        error,
                    )
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
            val receiver =
                object : ResultReceiver(null) {
                    override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
                        if (resultCode == AccessibilityPasteResult.SUCCESS.code) {
                            selectedText.set(
                                resultData
                                    ?.getString(
                                        OpenLessAccessibilityCommandReceiver.EXTRA_SELECTED_TEXT
                                    )
                                    .orEmpty()
                            )
                        }
                        latch.countDown()
                    }
                }
            return try {
                val intent =
                    Intent(context, OpenLessAccessibilityCommandReceiver::class.java).apply {
                        action = OpenLessAccessibilityCommandReceiver.ACTION_CAPTURE_SELECTED_TEXT
                        putExtra(
                            OpenLessAccessibilityCommandReceiver.EXTRA_RESULT_RECEIVER,
                            receiver,
                        )
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
