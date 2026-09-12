package com.openless.app

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Shader
import android.graphics.drawable.GradientDrawable
import android.inputmethodservice.InputMethodService
import android.text.InputType
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.LinearLayout
import android.widget.TextView

/** Minimal system IME surface. Voice transport is intentionally added in a later phase. */
class OpenLessImeService : InputMethodService(), OpenLessOverlayBridge.OverlayStateListener {
    private enum class InputMode { VOICE, STROKE, ENGLISH }

    private var sessionEpoch = 0L
    private var recording = false
    private var processing = false
    private var inputMode = InputMode.VOICE
    private var symbolMode = false
    private var keyboardShift = false
    private var state = "idle"
    private var currentMessage = "点击开始说话"
    private var lastBackendWarmupAt = 0L
    private var status: TextView? = null
    private var voiceButton: VoiceButton? = null
    private var englishUi = false
    private val strokeRepository by lazy { StrokeInputRepository(this) }
    private val phraseRepository by lazy { StrokePhraseRepository(this) }
    private val userFrequency by lazy { StrokeUserFrequency(this) }
    private var strokeCode = ""
    private var strokeQueryEpoch = 0L
    private var confirmedText = ""
    private var phraseQueryEpoch = 0L
    private var strokePreview: TextView? = null
    private var strokeCandidates: LinearLayout? = null

    private fun ui(zh: String, en: String) = if (englishUi) en else zh

    private fun restoreInputMode() {
        inputMode = when (getSharedPreferences("openless_ime_ui", MODE_PRIVATE).getString("input_mode", "voice")) {
            "stroke" -> InputMode.STROKE
            "english" -> InputMode.ENGLISH
            else -> InputMode.VOICE
        }
    }

    private fun saveInputMode(mode: InputMode) {
        getSharedPreferences("openless_ime_ui", MODE_PRIVATE).edit()
            .putString("input_mode", mode.name.lowercase())
            .apply()
    }

    private fun refreshLanguage() {
        val locale = getSharedPreferences("openless_ime_ui", MODE_PRIVATE)
            .getString("locale", null) ?: resources.configuration.locales[0].toLanguageTag()
        englishUi = !locale.startsWith("zh", ignoreCase = true)
    }

    override fun onCreate() {
        super.onCreate()
        restoreInputMode()
        activeInstance = java.lang.ref.WeakReference(this)
        OpenLessOverlayBridge.imeListener = this
        OpenLessOverlayBridge.imeTextListener = ::commitImeText
        startRuntimeService()
        // Load the offline stroke dictionary while the IME is idle, so the
        // first stroke key does not pay the asset parsing cost.
        strokeRepository.preloadAsync()
    }

    override fun onDestroy() {
        if (activeInstance?.get() === this) {
            activeInstance = null
        }
        if (OpenLessOverlayBridge.imeListener === this) {
            OpenLessOverlayBridge.imeListener = null
        }
        if (OpenLessOverlayBridge.imeTextListener != null) {
            OpenLessOverlayBridge.imeTextListener = null
        }
        stopRuntimeService()
        strokeRepository.shutdown()
        phraseRepository.shutdown()
        super.onDestroy()
    }

    override fun onCreateInputView(): View {
        refreshLanguage()
        startRuntimeService()
        if (inputMode == InputMode.ENGLISH) return buildKeyboardView()
        if (inputMode == InputMode.STROKE) return buildStrokeView()
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(16), dp(8), dp(16), dp(4))
            setBackgroundColor(Color.rgb(48, 48, 48))
            clipChildren = false
            clipToPadding = false
        }
        val header = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
        }
        val brand = TextView(this).apply {
            text = "◔  OpenLess"
            textSize = 18f
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            setTextColor(Color.WHITE)
            gravity = android.view.Gravity.CENTER_VERTICAL
            contentDescription = ui("打开 OpenLess 设置", "Open OpenLess settings")
            setOnClickListener {
                openSettings()
            }
        }
        header.addView(brand, LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(150), dp(38)))
        root.addView(header, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            dp(38),
        ))
        status = TextView(this).apply {
            text = displayStatus(currentMessage)
            textSize = 16f
            gravity = android.view.Gravity.CENTER
            setTextColor(Color.rgb(190, 190, 190))
            setPadding(0, dp(6), 0, dp(4))
        }
        root.addView(status, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            dp(38),
        ))

        voiceButton = VoiceButton(this).apply {
            isClickable = true
            setOnClickListener { toggleDictation() }
            contentDescription = ui("OpenLess 语音听写", "OpenLess dictation")
        }
        val buttonHolder = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER
            setBackgroundColor(Color.TRANSPARENT)
            clipChildren = false
            clipToPadding = false
        }
        buttonHolder.addView(voiceButton!!, LinearLayout.LayoutParams(dp(176), dp(72)))
        root.addView(buttonHolder, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            0,
            1f,
        ))

        val footer = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
            clipChildren = false
            clipToPadding = false
        }
        val inputMethodButton = TextView(this).apply {
            text = "⌨"
            textSize = 22f
            gravity = android.view.Gravity.CENTER
            setTextColor(Color.rgb(205, 205, 205))
            contentDescription = ui("切换输入法", "Switch input method")
            setOnClickListener {
                (getSystemService(INPUT_METHOD_SERVICE) as? InputMethodManager)
                    ?.showInputMethodPicker()
            }
        }
        val returnButton = TextView(this).apply {
            text = "return"
            textSize = 18f
            gravity = android.view.Gravity.CENTER
            setTextColor(Color.WHITE)
            background = roundedButton(Color.rgb(82, 82, 82), dp(24))
            contentDescription = ui("回车", "Return")
            setOnClickListener {
                currentInputConnection?.sendKeyEvent(android.view.KeyEvent(
                    android.view.KeyEvent.ACTION_DOWN,
                    android.view.KeyEvent.KEYCODE_ENTER,
                ))
                currentInputConnection?.sendKeyEvent(android.view.KeyEvent(
                    android.view.KeyEvent.ACTION_UP,
                    android.view.KeyEvent.KEYCODE_ENTER,
                ))
            }
        }
        val backspaceButton = TextView(this).apply {
            text = "⌫"
            textSize = 23f
            gravity = android.view.Gravity.CENTER
            setTextColor(Color.WHITE)
            background = roundedButton(Color.rgb(68, 68, 68), dp(20))
            contentDescription = ui("退格", "Backspace")
            setOnClickListener { currentInputConnection?.deleteSurroundingText(1, 0) }
        }
        val atButton = TextView(this).apply {
            text = "@"
            textSize = 23f
            gravity = android.view.Gravity.CENTER
            setTextColor(Color.WHITE)
            background = roundedButton(Color.rgb(68, 68, 68), dp(20))
            contentDescription = ui("输入 @", "Insert at sign")
            setOnClickListener { currentInputConnection?.commitText("@", 1) }
        }
        val sideButtons = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = android.view.Gravity.CENTER
            // Lift the two utility buttons slightly above the bottom edge while
            // keeping the fixed 300dp keyboard panel height unchanged.
            translationY = -dp(10).toFloat()
            // 单独留出足够高度，避免退格按钮被语音区域或父布局裁切。
            addView(backspaceButton, LinearLayout.LayoutParams(dp(44), dp(36)))
            addView(atButton, LinearLayout.LayoutParams(dp(44), dp(36)).apply {
                topMargin = dp(12)
            })
        }
        footer.addView(inputMethodButton, LinearLayout.LayoutParams(dp(44), dp(84)))
        val returnHolder = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER
            addView(returnButton, LinearLayout.LayoutParams(dp(120), dp(48)))
        }
        footer.addView(returnHolder, LinearLayout.LayoutParams(0, dp(84), 1f))
        footer.addView(sideButtons, LinearLayout.LayoutParams(dp(44), dp(84)))
        root.addView(footer, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            dp(84),
        ))
        return root
    }

    private fun refreshInputView() {
        setInputView(onCreateInputView())
    }

    private fun buildModeToggle(): View = ModeToggle(this, inputMode, englishUi) { selected ->
        if (recording || processing) cancelDictation()
        inputMode = selected
        saveInputMode(selected)
        symbolMode = false
        keyboardShift = false
        strokeCode = ""
        strokeQueryEpoch++
        confirmedText = ""
        phraseQueryEpoch++
        refreshInputView()
    }

    private fun buildKeyboardView(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(8), dp(8), dp(8), dp(8))
            setBackgroundColor(Color.rgb(48, 48, 48))
        }
        val header = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
        }
        val brand = TextView(this).apply {
            text = "◔  OpenLess"
            textSize = 18f
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            setTextColor(Color.WHITE)
            gravity = android.view.Gravity.CENTER_VERTICAL
            contentDescription = ui("打开 OpenLess 设置", "Open OpenLess settings")
            setOnClickListener {
                openSettings()
            }
        }
        header.addView(brand, LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(150), dp(38)))
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)))

        // 保持和 Typeless 类似的五排结构：数字、字母三排、底部功能排。
        addKeyboardRow(root, listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "0"))
        if (symbolMode) {
            addKeyboardRow(root, listOf("-", "/", ":", ";", "(", ")", "$", "&", "@", "\""))
            addKeyboardRow(root, listOf(".", ",", "?", "!", "'", "#", "%", "*", "+", "="))
            addKeyboardRow(root, listOf("[", "]", "{", "}", "_", "\\", "|", "~", "<", ">"))
        } else {
            addKeyboardRow(root, listOf("q", "w", "e", "r", "t", "y", "u", "i", "o", "p"))
            addKeyboardRow(root, listOf("a", "s", "d", "f", "g", "h", "j", "k", "l"))
            addKeyboardRow(root, listOf("⇧", "z", "x", "c", "v", "b", "n", "m", "⌫"))
        }

        val bottom = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
        }
        val modeButton = keyboardKey(if (symbolMode) "ABC" else ui("符号", "#+="), 1f) {
            symbolMode = !symbolMode
            keyboardShift = false
            refreshInputView()
        }
        val spaceButton = keyboardKey("", 2.7f) {
            currentInputConnection?.commitText(" ", 1)
        }
        val returnButton = keyboardKey("return", 1.35f) {
            sendEnterKey()
        }
        bottom.addView(modeButton)
        bottom.addView(spaceButton)
        bottom.addView(returnButton)
        root.addView(bottom, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        return root
    }

    private fun buildStrokeView(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(4), dp(3), dp(4), dp(3))
            setBackgroundColor(Color.rgb(48, 48, 48))
        }
        val header = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        val brand = TextView(this).apply {
            text = "◔  OpenLess"
            textSize = 18f
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            setTextColor(Color.WHITE)
            gravity = android.view.Gravity.CENTER_VERTICAL
            contentDescription = ui("打开 OpenLess 设置", "Open OpenLess settings")
            setOnClickListener { openSettings() }
        }
        header.addView(brand, LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(150), dp(38)))
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)))

        // Stroke mode follows the reference layout: a compact stroke row,
        // candidate row, punctuation column, stroke grid, and action rail.
        val top = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        val strokeRow = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        strokePreview = TextView(this).apply {
            text = ui("—", "—")
            textSize = 16f
            setTextColor(Color.rgb(210, 210, 210))
            gravity = android.view.Gravity.CENTER_VERTICAL
            setPadding(dp(8), 0, 0, 0)
        }
        strokeRow.addView(strokePreview, LinearLayout.LayoutParams(0, dp(30), 1f))
        top.addView(strokeRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(30)))

        val candidateRow = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        val candidatesScroll = android.widget.HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            strokeCandidates = LinearLayout(context).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
            addView(strokeCandidates, ViewGroup.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        candidateRow.addView(candidatesScroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(30)))
        top.addView(candidateRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(30)))
        root.addView(top, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(60)))

        val body = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
        val punctuation = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; gravity = android.view.Gravity.CENTER }
        listOf(",", "°", "?", "!", "~").forEach { mark ->
            punctuation.addView(keyboardKey(mark, 1f) { currentInputConnection?.commitText(mark, 1) }.apply {
                textSize = 18f
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
                }
            })
        }
        body.addView(punctuation, LinearLayout.LayoutParams(dp(42), ViewGroup.LayoutParams.MATCH_PARENT))

        val grid = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; gravity = android.view.Gravity.CENTER }
        val strokeRows = listOf(
            listOf("1\n一" to "h", "2\n丨" to "s", "3\n丿" to "p"),
            listOf("4\n丶" to "n", "5\n乛" to "z", "6\n${ui("通配", "Wildcard")}" to "*"),
            listOf("7\n${ui("分词", "Word")}" to " ", "8\n：" to ":", "9\n；" to ";"),
            listOf(ui("中", "CN") to "中", "⌨" to "voice", ui("符号", "Symbols") to "symbols"),
        )
        strokeRows.forEach { rowItems ->
            val row = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
            rowItems.forEach { (label, code) ->
                val key = if (code == "voice") keyboardKey("🎙", 1f) {
                    currentInputConnection?.commitText(" ", 1)
                }.apply {
                    setOnLongClickListener {
                        inputMode = InputMode.VOICE
                        saveInputMode(inputMode)
                        clearStrokes()
                        refreshInputView()
                        true
                    }
                } else keyboardKey(label, 1f) {
                    when (code) {
                        "symbols" -> currentInputConnection?.commitText("#", 1)
                        " " -> currentInputConnection?.commitText(" ", 1)
                        else -> if (code in listOf("h", "s", "p", "n", "z", "*")) appendStroke(code) else currentInputConnection?.commitText(code, 1)
                    }
                }
                key.textSize = if (code == "voice") 18f else 17f
                row.addView(key)
            }
            grid.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        body.addView(grid, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))

        val actions = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; gravity = android.view.Gravity.CENTER }
        listOf("⌫" to { deleteStroke() }, "↵" to { sendEnterKey() }, ui("清空", "Clear") to { clearStrokes() }, "123" to { inputMode = InputMode.ENGLISH; saveInputMode(inputMode); clearStrokes(); refreshInputView() }).forEach { (label, action) ->
            actions.addView(keyboardKey(label, 1f, action).apply {
                textSize = 17f
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
                    setMargins(dp(1), dp(2), dp(1), dp(2))
                }
                background = roundedButton(Color.rgb(92, 28, 48), dp(7))
            })
        }
        body.addView(actions, LinearLayout.LayoutParams(dp(58), ViewGroup.LayoutParams.MATCH_PARENT))
        root.addView(body, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        return root
    }

    private fun appendStroke(stroke: String) {
        if (strokeCode.length >= 32) return
        if (strokeCode.isEmpty()) {
            phraseQueryEpoch++
            strokeCandidates?.removeAllViews()
        }
        strokeCode += stroke
        strokePreview?.text = ui("笔画：$strokeCode", "Strokes: $strokeCode")
        val query = ++strokeQueryEpoch
        strokeRepository.searchAsync(strokeCode) { result ->
            if (query != strokeQueryEpoch || inputMode != InputMode.STROKE) return@searchAsync
            strokeCandidates?.removeAllViews()
            result.forEach { candidate ->
                strokeCandidates?.addView(keyboardKey(candidate, 1f) { commitStrokeCandidate(candidate) }.apply { textSize = 18f }, LinearLayout.LayoutParams(dp(36), dp(30)))
            }
        }
    }

    private fun deleteStroke() {
        if (strokeCode.isNotEmpty()) {
            strokeCode = strokeCode.dropLast(1)
            strokeQueryEpoch++
            strokePreview?.text = ui("笔画：$strokeCode", "Strokes: $strokeCode")
            strokeCandidates?.removeAllViews()
            if (strokeCode.isNotEmpty()) appendStroke("")
        } else {
            currentInputConnection?.deleteSurroundingText(1, 0)
            if (confirmedText.isNotEmpty()) {
                confirmedText = confirmedText.dropLast(1)
                phraseQueryEpoch++
                refreshAssociations()
            } else {
                strokeCandidates?.removeAllViews()
            }
        }
    }

    private fun clearStrokes() {
        strokeCode = ""
        strokeQueryEpoch++
        strokePreview?.text = ui("笔画：请选择", "Strokes: choose strokes")
        strokeCandidates?.removeAllViews()
    }

    private fun commitStrokeCandidate(candidate: String) {
        if (isSensitiveField(currentInputEditorInfo)) return
        val connection = currentInputConnection ?: return
        val contextBeforeCommit = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        if (!connection.commitText(candidate, 1)) return
        userFrequency.record(currentInputEditorInfo?.packageName.orEmpty(), contextBeforeCommit, candidate)
        confirmedText = (confirmedText + candidate).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
    }

    private fun refreshAssociations() {
        val context = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        val query = ++phraseQueryEpoch
        if (context.isEmpty()) return
        val packageName = currentInputEditorInfo?.packageName.orEmpty()
        phraseRepository.searchAsync(context, packageName) { result ->
            if (query != phraseQueryEpoch || inputMode != InputMode.STROKE || confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT) != context) return@searchAsync
            strokeCandidates?.removeAllViews()
            result.forEach { candidate ->
                val matchedPrefix = candidate.matchedPrefix.ifEmpty { context }
                strokeCandidates?.addView(keyboardKey(candidate.text, 1f) { commitAssociation(candidate.text, matchedPrefix) }.apply { textSize = 18f }, LinearLayout.LayoutParams(dp(68), dp(30)))
            }
        }
    }

    private fun commitAssociation(displayText: String, matchedContext: String) {
        if (isSensitiveField(currentInputEditorInfo) || !displayText.startsWith(matchedContext)) return
        val suffix = displayText.removePrefix(matchedContext)
        val connection = currentInputConnection ?: return
        if (suffix.isNotEmpty() && !connection.commitText(suffix, 1)) return
        userFrequency.record(currentInputEditorInfo?.packageName.orEmpty(), matchedContext, displayText)
        confirmedText = (confirmedText + suffix).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
    }

    private fun addKeyboardRow(parent: LinearLayout, keys: List<String>) {
        val row = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER
        }
        keys.forEach { key ->
            row.addView(keyboardKey(key, 1f) { handleKeyboardKey(key) })
        }
        parent.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    private fun keyboardKey(label: String, weight: Float, action: () -> Unit): TextView {
        return TextView(this).apply {
            text = if ('\n' in label) {
                android.text.SpannableString(label).apply {
                    setSpan(android.text.style.RelativeSizeSpan(0.55f), 0, 1, android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
                }
            } else label
            textSize = if (label == "return") 17f else 22f
            gravity = android.view.Gravity.CENTER
            setTextColor(Color.rgb(245, 245, 245))
            background = roundedButton(Color.rgb(78, 78, 78), dp(10))
            contentDescription = label.ifBlank { ui("空格", "Space") }
            setOnClickListener { action() }
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, weight).apply {
                setMargins(dp(3), dp(3), dp(3), dp(3))
            }
        }
    }

    private fun handleKeyboardKey(key: String) {
        when (key) {
            "⌫" -> currentInputConnection?.deleteSurroundingText(1, 0)
            "⇧" -> {
                keyboardShift = !keyboardShift
                refreshInputView()
            }
            "ABC" -> {
                symbolMode = false
                keyboardShift = false
                refreshInputView()
            }
            else -> {
                val text = if (keyboardShift && key.length == 1) key.uppercase() else key
                currentInputConnection?.commitText(text, 1)
                if (keyboardShift) {
                    keyboardShift = false
                    refreshInputView()
                }
            }
        }
    }

    private fun sendEnterKey() {
        currentInputConnection?.sendKeyEvent(android.view.KeyEvent(
            android.view.KeyEvent.ACTION_DOWN,
            android.view.KeyEvent.KEYCODE_ENTER,
        ))
        currentInputConnection?.sendKeyEvent(android.view.KeyEvent(
            android.view.KeyEvent.ACTION_UP,
            android.view.KeyEvent.KEYCODE_ENTER,
        ))
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        super.onStartInput(attribute, restarting)
        restoreInputMode()
        refreshLanguage()
        startRuntimeService()
        sessionEpoch++
        confirmedText = ""
        phraseQueryEpoch++
        strokeCode = ""
        recording = false
        processing = false
        if (isSensitiveField(attribute)) {
            updateStatus("敏感字段，已禁用听写")
        } else {
            setState("idle", "点击开始说话")
        }
    }

    override fun onFinishInput() {
        if (recording) {
            runNativeAction("取消听写") { OpenLessNative.nativeCancelDictation() }
        }
        recording = false
        processing = false
        invalidateSession("输入目标已失效")
        super.onFinishInput()
    }

    private fun toggleDictation() {
        if (isSensitiveField(currentInputEditorInfo)) {
            updateStatus("敏感字段，禁止听写")
            return
        }
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            updateStatus("请先授予麦克风权限")
            return
        }
        if (recording) {
            recording = false
            processing = true
            setState("thinking", "正在思考")
            runNativeAction("停止听写") { OpenLessNative.nativeStopDictationForIme() }
        } else {
            recording = true
            processing = false
            setState("speaking", "再次点击结束")
            runNativeAction("开始听写") { OpenLessNative.nativeStartDictationForIme() }
        }
    }

    private fun cancelDictation() {
        recording = false
        processing = false
        invalidateSession("已取消")
        runNativeAction("取消听写") { OpenLessNative.nativeCancelDictation() }
    }

    private fun runNativeAction(action: String, call: () -> Unit) {
        try {
            call()
        } catch (error: Throwable) {
            recording = false
            processing = false
            updateStatus("${action}失败，请打开 OpenLess 后重试")
            android.util.Log.w("OpenLessImeService", "$action native bridge unavailable", error)
        }
    }

    override fun onCapsuleStateChanged(state: String, message: String?, level: Float) {
        voiceButton?.audioLevel = level.coerceIn(0f, 1f)
        when (state) {
            "recording" -> {
                recording = true
                processing = false
                setState("speaking", "再次点击结束")
            }
            "transcribing" -> {
                recording = false
                processing = true
                setState("thinking", "正在思考")
            }
            "polishing" -> {
                recording = false
                processing = true
                setState("thinking", "正在思考")
            }
            "done" -> {
                recording = false
                processing = false
                setState("done", message ?: "已完成")
            }
            "cancelled" -> {
                recording = false
                processing = false
                setState("idle", message ?: "已取消")
            }
            "error" -> {
                recording = false
                processing = false
                setState("error", message ?: "识别失败")
            }
            "idle" -> if (!recording) {
                setState("idle", message ?: "点击开始说话")
            }
        }
    }

    private fun commitTestText() {
        val attribute = currentInputEditorInfo
        if (isSensitiveField(attribute)) {
            updateStatus("敏感字段，禁止上屏")
            return
        }
        val connection = currentInputConnection
        if (connection == null) {
            updateStatus("没有有效输入连接")
            return
        }
        val epoch = sessionEpoch
        if (epoch != sessionEpoch || !connection.commitText(TEST_TEXT, 1)) {
            updateStatus("输入连接已失效")
            return
        }
        updateStatus("已上屏")
    }

    private fun invalidateSession(message: String) {
        sessionEpoch++
        updateStatus(message)
    }

    private fun isSensitiveField(attribute: EditorInfo?): Boolean {
        val inputType = attribute?.inputType ?: return false
        val variation = inputType and InputType.TYPE_MASK_VARIATION
        return (inputType and InputType.TYPE_MASK_CLASS) == InputType.TYPE_CLASS_NUMBER ||
            variation == InputType.TYPE_TEXT_VARIATION_PASSWORD ||
            variation == InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD ||
            variation == InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
    }

    private fun updateStatus(message: String) {
        currentMessage = message
        status?.text = displayStatus(message)
        voiceButton?.isRecording = recording
        voiceButton?.isProcessing = processing
    }

    private fun commitImeText(text: String) {
        if (text.isBlank()) {
            recording = false
            processing = false
            setState("error", "没有识别到文字")
            return
        }
        val connection = currentInputConnection
        if (connection == null || !connection.commitText(text, 1)) {
            recording = false
            processing = false
            setState("error", "输入连接已失效")
            return
        }
        recording = false
        processing = false
        setState("done", "已上屏")
    }

    private fun setState(nextState: String, message: String) {
        state = nextState
        updateStatus(message)
    }

    private fun displayStatus(message: String): String {
        if (!englishUi) return message
        return when (message) {
            "点击开始说话" -> "Tap to speak"
            "再次点击结束" -> "Tap again to finish"
            "正在思考" -> "Thinking"
            "已完成", "已上屏" -> "Done"
            "已取消" -> "Cancelled"
            "敏感字段，已禁用听写", "敏感字段，禁止听写", "敏感字段，禁止上屏" -> "Dictation disabled in this field"
            "请先授予麦克风权限" -> "Microphone permission required"
            "输入目标已失效", "输入连接已失效", "没有有效输入连接" -> "Tap a text field to continue"
            "没有识别到文字" -> "No speech recognized"
            "识别失败" -> "Recognition failed. Please try again."
            else -> if (message.any { it in '\u4e00'..'\u9fff' }) {
                // Provider details remain in logs; the IME always uses its UI language.
                when {
                    message.contains("失败") || state == "error" -> "Dictation failed. Please try again."
                    state == "thinking" -> "Thinking"
                    state == "speaking" -> "Tap again to finish"
                    state == "done" -> "Done"
                    else -> "Tap to speak"
                }
            } else message
        }
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private fun roundedButton(color: Int, radius: Int): GradientDrawable {
        return GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = radius.toFloat()
            setColor(color)
        }
    }

    private fun startRuntimeService() {
        try {
            val intent = android.content.Intent(this, OpenLessRuntimeService::class.java)
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O) {
                startForegroundService(intent)
            } else {
                startService(intent)
            }
            ensureBackendReady()
        } catch (error: Throwable) {
            android.util.Log.w("OpenLessImeService", "failed to start IME runtime service", error)
        }
    }

    private fun openSettings() {
        requestHideSelf(0)
        android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
            // If cold-start created the Tauri host through the IME, that Activity is
            // already the real settings UI. Starting MainActivity as a second Tauri
            // host produces the black window that only disappears after Back.
            if (OpenLessBackendWarmupActivity.openSettingsIfRunning(this)) {
                return@postDelayed
            }
            startActivity(android.content.Intent(this, MainActivity::class.java).apply {
                addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(android.content.Intent.FLAG_ACTIVITY_CLEAR_TOP)
                addFlags(android.content.Intent.FLAG_ACTIVITY_SINGLE_TOP)
                addFlags(android.content.Intent.FLAG_ACTIVITY_NO_ANIMATION)
            })
        }, 180L)
    }

    private fun ensureBackendReady() {
        val now = android.os.SystemClock.elapsedRealtime()
        if (now - lastBackendWarmupAt < 5_000L) return
        // The Tauri host owns the Rust backend and remains alive in the background.
        // Re-launching it for every new editor focus creates a full-screen transition
        // and steals focus, which is the visible flash when switching applications.
        if (OpenLessBackendWarmupActivity.isRunning()) return
        try {
            OpenLessNative.requireBackendContract()
        } catch (error: Throwable) {
            lastBackendWarmupAt = now
            android.util.Log.i("OpenLessImeService", "backend is not ready; launching main process", error)
            android.os.Handler(mainLooper).postDelayed({
                runCatching {
                    android.content.Intent(this, OpenLessBackendWarmupActivity::class.java).apply {
                        currentInputEditorInfo?.packageName?.let {
                            putExtra(OpenLessBackendWarmupActivity.EXTRA_RETURN_PACKAGE, it)
                        }
                        addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                        addFlags(android.content.Intent.FLAG_ACTIVITY_NO_ANIMATION)
                    }?.let(::startActivity)
                }.onFailure { launchError ->
                    android.util.Log.w("OpenLessImeService", "failed to launch main process", launchError)
                }
            }, 120L)
        }
    }

    private fun stopRuntimeService() {
        runCatching { stopService(android.content.Intent(this, OpenLessRuntimeService::class.java)) }
    }

    private class ModeToggle(
        context: android.content.Context,
        private val selectedMode: InputMode,
        private val englishUi: Boolean,
        private val onModeSelected: (InputMode) -> Unit,
    ) : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG)

        private fun dp(value: Int): Float = value * resources.displayMetrics.density

        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            val inset = dp(1)
            val radius = height / 2f
            paint.color = Color.rgb(28, 28, 28)
            canvas.drawRoundRect(inset, inset, width - inset, height - inset, radius, radius, paint)

            paint.color = Color.rgb(88, 86, 88)
            val segmentWidth = width / 3f
            val selectedIndex = when (selectedMode) {
                InputMode.VOICE -> 0
                InputMode.STROKE -> 1
                InputMode.ENGLISH -> 2
            }
            val segmentLeft = selectedIndex * segmentWidth + inset
            val segmentRight = (selectedIndex + 1) * segmentWidth - inset
            canvas.drawRoundRect(segmentLeft, inset, segmentRight, height - inset, radius, radius, paint)

            paint.color = Color.WHITE
            paint.strokeWidth = dp(2.4f.toInt())
            paint.strokeCap = Paint.Cap.ROUND
            val centerX = segmentWidth * 0.5f
            val centerY = height / 2f
            val bars = floatArrayOf(.28f, .58f, .82f, 1f, .68f, .44f, .28f)
            val gap = dp(5)
            bars.forEachIndexed { index, factor ->
                val x = centerX + (index - 3) * gap
                val half = height * 0.32f * factor
                canvas.drawLine(x, centerY - half, x, centerY + half, paint)
            }
            paint.textSize = dp(if (englishUi) 11 else 15)
            paint.textAlign = Paint.Align.CENTER
            paint.typeface = android.graphics.Typeface.create("sans-serif", android.graphics.Typeface.NORMAL)
            canvas.drawText(if (englishUi) "Stroke" else "笔", segmentWidth * 1.5f, centerY - (paint.ascent() + paint.descent()) / 2f, paint)
            canvas.drawText("EN", segmentWidth * 2.5f, centerY - (paint.ascent() + paint.descent()) / 2f, paint)
        }

        override fun onTouchEvent(event: android.view.MotionEvent): Boolean {
            if (event.action == android.view.MotionEvent.ACTION_UP) {
                val index = (event.x / (width / 3f)).toInt().coerceIn(0, 2)
                onModeSelected(when (index) {
                    0 -> InputMode.VOICE
                    1 -> InputMode.STROKE
                    else -> InputMode.ENGLISH
                })
            }
            return true
        }
    }

    private class VoiceButton(context: android.content.Context) : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        var isRecording: Boolean = false
            set(value) {
                field = value
                invalidate()
            }
        var isProcessing: Boolean = false
            set(value) {
                field = value
                invalidate()
            }

        var audioLevel: Float = 0f
            set(value) {
                field = value
                invalidate()
            }

        private var phase = 0f
        private val animator = object : Runnable {
            override fun run() {
                phase += 0.18f
                invalidate()
                postDelayed(this, 50L)
            }
        }

        override fun onAttachedToWindow() {
            super.onAttachedToWindow()
            post(animator)
        }

        override fun onDetachedFromWindow() {
            removeCallbacks(animator)
            super.onDetachedFromWindow()
        }

        private fun dp(value: Int): Int =
            (value * resources.displayMetrics.density).toInt()

        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            val centerX = width / 2f
            val centerY = height / 2f
            val scale = if (isRecording) {
                1f + 0.04f + audioLevel * 0.20f
            } else {
                1f
            }
            val pillWidth = width * 0.86f
            val pillHeight = minOf(pillWidth * 192f / 470f, height * 0.90f)
            val left = centerX - pillWidth / 2f
            val top = centerY - pillHeight * scale / 2f
            val right = centerX + pillWidth / 2f
            val bottom = centerY + pillHeight * scale / 2f
            val radius = pillHeight * 0.5f
            // 录音/思考状态只显示动画，完全移除胶囊背景；待机状态保留话筒按钮。
            if (!isRecording && !isProcessing) {
                paint.color = Color.rgb(54, 54, 54)
                canvas.drawRoundRect(left, top, right, bottom, radius, radius, paint)
            }

            paint.color = Color.WHITE
            paint.strokeWidth = width * 0.025f
            paint.strokeCap = Paint.Cap.ROUND
            if (!isRecording && !isProcessing) {
                // Reference crop: capsule (393,380)-(863,572), 470 x 192.
                // All geometry uses that ONE coordinate system and uniform scaling.
                canvas.save()
                val unit = pillHeight / 192f
                canvas.translate(centerX - 235f * unit, centerY - 96f * unit)
                canvas.scale(unit, unit)
                paint.style = Paint.Style.FILL
                canvas.drawRoundRect(213f, 40f, 259f, 113f, 23f, 23f, paint)
                paint.style = Paint.Style.STROKE
                paint.strokeWidth = 11f
                val arc = Path().apply {
                    moveTo(196f, 96f)
                    cubicTo(198f, 118f, 215f, 130f, 236f, 130f)
                    cubicTo(257f, 130f, 274f, 118f, 275f, 96f)
                }
                canvas.drawPath(arc, paint)
                canvas.drawLine(236f, 130f, 236f, 146f, paint)
                paint.style = Paint.Style.FILL
                canvas.restore()
                return
            }
            if (isRecording) {
                // Monochrome waveform: quiet input stays compact while speech
                // expands the bars clearly with the live microphone level.
                val live = (audioLevel * 1.15f).coerceIn(0f, 1f)
                val heights = floatArrayOf(.12f, .22f, .40f, .68f, .92f, 1f, .78f, .50f, .28f)
                val gap = width * 0.095f
                val startX = centerX - gap * (heights.size - 1) / 2f
                heights.forEachIndexed { index, heightFactor ->
                    val x = startX + index * gap
                    val shimmer = 0.82f + 0.18f * kotlin.math.sin(
                        (phase * 1.8f + index * 1.37f).toDouble(),
                    ).toFloat()
                    val halfHeight = minOf(height * 0.95f, dp(66).toFloat()) *
                        (0.035f + live * 0.965f) * heightFactor * shimmer
                    paint.color = Color.rgb(222, 222, 222)
                    paint.strokeWidth = dp(3).toFloat()
                    canvas.drawLine(x, centerY - halfHeight, x, centerY + halfHeight, paint)
                }
            } else if (isProcessing) {
                // Analysis state uses the same restrained monochrome palette.
                val colors = intArrayOf(
                    Color.rgb(245, 245, 245), Color.rgb(205, 205, 205),
                    Color.rgb(170, 170, 170), Color.rgb(235, 235, 235),
                    Color.rgb(190, 190, 190), Color.rgb(220, 220, 220),
                )
                val orbit = minOf(width * 0.28f, height * 0.52f)
                val dotRadius = minOf(width * 0.055f, height * 0.15f)
                colors.forEachIndexed { index, color ->
                    val angle = phase * 0.65f + index * (Math.PI.toFloat() / 3f)
                    val x = centerX + kotlin.math.cos(angle.toDouble()).toFloat() * orbit
                    val y = centerY + kotlin.math.sin(angle.toDouble()).toFloat() * orbit
                    paint.color = color
                    canvas.drawCircle(x, y, dotRadius, paint)
                }
            }
        }
    }

    companion object {
        private const val TEST_TEXT = "OpenLess IME 测试上屏"
        private const val MAX_ASSOCIATION_CONTEXT = 8

        @Volatile
        private var activeInstance: java.lang.ref.WeakReference<OpenLessImeService>? = null

        /** Re-open the IME after the one-time backend Activity gives focus back. */
        fun requestInputPanelAfterWarmup(delayMs: Long = 260L) {
            val service = activeInstance?.get() ?: return
            android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
                if (activeInstance?.get() === service) {
                    service.requestShowSelf(InputMethodManager.SHOW_IMPLICIT)
                }
            }, delayMs)
        }
    }
}
