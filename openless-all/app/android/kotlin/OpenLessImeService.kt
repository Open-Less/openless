package com.openless.app

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Shader
import android.graphics.drawable.GradientDrawable
import android.icu.text.Transliterator
import android.inputmethodservice.InputMethodService
import android.os.Handler
import android.os.Looper
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.text.InputType
import android.view.MotionEvent
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
    private var strokeNumberMode = false
    private var traditionalOutput = false
    private var keyboardShift = false
    private var state = "idle"
    private var currentMessage = "点击开始说话"
    private var lastBackendWarmupAt = 0L
    private var status: TextView? = null
    private var voiceButton: VoiceButton? = null
    private var englishUi = false
    private val simplifiedToTraditional by lazy { Transliterator.getInstance("Hans-Hant") }
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

    private fun outputScript(text: String): String {
        if (!traditionalOutput) return text
        return runCatching { simplifiedToTraditional.transliterate(text) }.getOrDefault(text)
    }

    private fun displayStrokeCode(code: String): String = code.map { stroke ->
        when (stroke) {
            'h' -> '一'
            's' -> '丨'
            'p' -> '丿'
            'n' -> '丶'
            'z' -> '乙'
            '*' -> '＊'
            else -> stroke
        }
    }.joinToString("")

    private fun restoreScriptPreference() {
        val preferences = getSharedPreferences("openless_ime_ui", MODE_PRIVATE)
        traditionalOutput = if (preferences.contains("stroke_traditional_output")) {
            preferences.getBoolean("stroke_traditional_output", false)
        } else {
            OpenLessAndroidPreferences.chineseScriptPreference(this) == "traditional"
        }
    }

    private fun toggleScriptPreference() {
        traditionalOutput = !traditionalOutput
        getSharedPreferences("openless_ime_ui", MODE_PRIVATE).edit()
            .putBoolean("stroke_traditional_output", traditionalOutput)
            .apply()
        refreshInputView()
    }

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
        restoreScriptPreference()
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
        if (inputMode == InputMode.STROKE) return if (strokeNumberMode) buildStrokeNumberView() else buildStrokeView()
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

    private fun buildModeToggle(): View = ModeToggle(this, inputMode) { selected ->
        if (recording || processing) cancelDictation()
        inputMode = selected
        saveInputMode(selected)
        symbolMode = false
        strokeNumberMode = false
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
        // This panel's own root padding (8dp) is narrower than the voice panel's
        // (16dp), which it needs for its body rows. Compensate with margins so
        // the header/toggle still land at the same canonical 16dp/8dp inset as
        // every other panel — otherwise the logo and toggle visibly jump left
        // and up when switching modes.
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)).apply {
            marginStart = dp(8)
            marginEnd = dp(8)
        })

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
        val modeButton = keyboardKey(if (symbolMode) "ABC" else ui("符号", "#+="), 1f, action = {
            symbolMode = !symbolMode
            keyboardShift = false
            refreshInputView()
        })
        val spaceButton = keyboardKey("", 2.7f, action = {
            currentInputConnection?.commitText(" ", 1)
        })
        val returnButton = keyboardKey("return", 1.35f, action = {
            sendEnterKey()
        })
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
        // Stroke mode's root padding is much tighter (4dp/3dp) to fit its dense
        // grid. Compensate with margins so the header/toggle still land at the
        // same canonical 16dp/8dp inset as every other panel.
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)).apply {
            marginStart = dp(12)
            marginEnd = dp(12)
            topMargin = dp(5)
        })

        // Stroke mode follows the reference layout: a compact stroke row,
        // candidate row, punctuation column, stroke grid, and action rail.
        val top = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        val strokeRow = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        strokePreview = TextView(this).apply {
            text = ""
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
            isFillViewport = false
            overScrollMode = View.OVER_SCROLL_NEVER
            strokeCandidates = LinearLayout(context).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
            strokeCandidates?.orientation = LinearLayout.HORIZONTAL
            addView(strokeCandidates, ViewGroup.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        candidateRow.addView(candidatesScroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(30)))
        top.addView(candidateRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(30)))
        root.addView(top, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(60)))

        val body = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
        val punctuation = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = android.view.Gravity.CENTER
            setPadding(dp(2), dp(2), dp(2), dp(2))
            background = roundedButton(Color.rgb(45, 45, 45), dp(4))
        }
        listOf(",", "°", "?", "!", "~").forEachIndexed { index, mark ->
            punctuation.addView(keyboardKey(mark, 1f, action = { currentInputConnection?.commitText(mark, 1) }).apply {
                textSize = 18f
                // The rail is one connected key surface; separators provide the only visual split.
                background = GradientDrawable().apply { setColor(Color.TRANSPARENT) }
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)
            })
            if (index < 4) {
                punctuation.addView(View(this).apply {
                    setBackgroundColor(Color.rgb(28, 28, 28))
                }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)))
            }
        }
        // Match the reference proportions: both side rails occupy the same share of the panel.
        body.addView(punctuation, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 0.16f))

        val grid = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; gravity = android.view.Gravity.CENTER }
        val strokeRows = listOf(
            listOf("1\n一" to "h", "2\n丨" to "s", "3\n丿" to "p"),
            listOf("4\n丶" to "n", "5\n乙" to "z", "6\n通配" to "*"),
            listOf("7\n分词" to " ", "8\n：" to ":", "9\n；" to ";"),
            listOf("繁" to "script", "🎙" to "voice", "符号" to "symbols"),
        )
        strokeRows.forEach { rowItems ->
            val row = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
            rowItems.forEach { (label, code) ->
                val key = if (code == "script") keyboardKey(label, 1f, action = {
                    toggleScriptPreference()
                }, graphicCode = "script").apply {
                    if (traditionalOutput) {
                        background = roundedButton(Color.rgb(112, 78, 92), dp(5))
                    }
                } else if (code == "voice") keyboardKey("0", 1f, action = {
                    currentInputConnection?.commitText(" ", 1)
                }, swipeUpAction = {
                    currentInputConnection?.commitText("0", 1)
                }, swipePreview = "0", microphoneIcon = true).apply {
                    setOnLongClickListener {
                        inputMode = InputMode.VOICE
                        saveInputMode(inputMode)
                        clearStrokes()
                        refreshInputView()
                        if (!recording) toggleDictation()
                        true
                    }
                } else {
                    val swipeDigit = label.substringBefore("\n").takeIf { it.length == 1 && it[0].isDigit() }
                    keyboardKey(label, 1f, action = {
                    when (code) {
                        "symbols" -> currentInputConnection?.commitText("#", 1)
                        " " -> currentInputConnection?.commitText(" ", 1)
                        else -> if (code in listOf("h", "s", "p", "n", "z", "*")) appendStroke(code) else currentInputConnection?.commitText(code, 1)
                    }
                    }, swipeUpAction = swipeDigit?.let { digit ->
                        { currentInputConnection?.commitText(digit, 1) }
                    }, swipePreview = swipeDigit, strokeIconCode = code.takeIf {
                        it in listOf("h", "s", "p", "n", "z")
                    }, graphicCode = code.takeIf {
                        it in listOf("*", ":", ";", " ", "symbols")
                    })
                }
                key.textSize = if (code == "voice") 10f else 17f
                row.addView(key)
            }
            grid.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        body.addView(grid, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 0.65f))

        val actions = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; gravity = android.view.Gravity.CENTER }
        listOf("←" to { deleteStroke() }, "↵" to { sendEnterKey() }, "清除" to { clearStrokes() }, "123" to { strokeNumberMode = true; refreshInputView() }).forEach { (label, action) ->
            actions.addView(keyboardKey(label, 1f, action, repeatOnLongPress = label == "←", repeatAction = action,
                graphicActionCode = label).apply {
                textSize = if (label == "←" || label == "↵") 30f else 17f
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
                    setMargins(dp(1), dp(2), dp(1), dp(2))
                }
                background = roundedButton(Color.rgb(153, 26, 40), dp(5))
            })
        }
        body.addView(actions, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 0.19f))
        root.addView(body, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        // Rebuilds caused by switching back from the numeric panel must restore
        // both the visible code and its candidates from the retained buffer.
        if (strokeCode.isNotEmpty()) {
            strokePreview?.text = displayStrokeCode(strokeCode)
            refreshStrokeCandidates(strokeCode)
        }
        return root
    }

    /** Numeric/symbol quick panel; pending stroke input is intentionally preserved. */
    private fun buildStrokeNumberView(): View {
        refreshLanguage()
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(8), dp(8), dp(8), dp(8))
            setBackgroundColor(Color.rgb(48, 48, 48))
        }
        val header = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        val brand = TextView(this).apply {
            text = "◔  OpenLess"
            textSize = 18f
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            setTextColor(Color.WHITE)
            gravity = android.view.Gravity.CENTER_VERTICAL
            setPadding(dp(8), 0, 0, 0)
            contentDescription = ui("打开 OpenLess 设置", "Open OpenLess settings")
            setOnClickListener { openSettings() }
        }
        header.addView(brand, LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(150), dp(38)))
        // This panel's own root padding (8dp) is narrower than the voice panel's
        // (16dp). Compensate with margins so the header/toggle still land at the
        // same canonical 16dp/8dp inset as every other panel.
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)).apply {
            marginStart = dp(8)
            marginEnd = dp(8)
        })

        val rows = listOf(
            listOf("@", "1", "2", "3", ui("退格", "Backspace")),
            listOf(":", "4", "5", "6", ui("←", "Back")),
            listOf(",", "7", "8", "9", ui("麦克风", "Voice")),
            listOf("+", "-", ".", ui("符号", "Symbols"), ui("返回", "Return")),
        )
        val body = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        rows.forEachIndexed { rowIndex, rowItems ->
            val row = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
            rowItems.forEachIndexed { index, label ->
                val isAction = index == rowItems.lastIndex
                val action: () -> Unit = when {
                    rowIndex == 0 && isAction -> ({ currentInputConnection?.deleteSurroundingText(1, 0) })
                    rowIndex == 1 && isAction -> ({ sendEnterKey() })
                    rowIndex == 2 && isAction -> ({
                        inputMode = InputMode.VOICE
                        saveInputMode(inputMode)
                        strokeNumberMode = false
                        refreshInputView()
                    })
                    rowIndex == 3 && isAction -> ({ strokeNumberMode = false; refreshInputView() })
                    label == ui("符号", "Symbols") -> ({ currentInputConnection?.commitText("#", 1) })
                    else -> ({ currentInputConnection?.commitText(label, 1) })
                }
                row.addView(keyboardKey(label, 1f, action, repeatOnLongPress = rowIndex == 0 && isAction, repeatAction = action).apply {
                    textSize = if (isAction) 15f else 20f
                    if (isAction) background = roundedButton(Color.rgb(153, 26, 40), dp(7))
                })
            }
            body.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
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
        strokePreview?.text = displayStrokeCode(strokeCode)
        refreshStrokeCandidates(strokeCode)
    }

    private fun refreshStrokeCandidates(code: String) {
        val query = ++strokeQueryEpoch
        strokeRepository.searchAsync(code) { result ->
            if (query != strokeQueryEpoch || inputMode != InputMode.STROKE) return@searchAsync
            strokeCandidates?.removeAllViews()
            result.forEach { candidate ->
                strokeCandidates?.addView(keyboardKey(outputScript(candidate), 1f, action = { commitStrokeCandidate(candidate) }).apply {
                    textSize = 18f
                    setSingleLine(true)
                    maxLines = 1
                }, LinearLayout.LayoutParams(dp(44), dp(30)))
            }
        }
    }

    private fun deleteStroke() {
        if (strokeCode.isNotEmpty()) {
            strokeCode = strokeCode.dropLast(1)
            strokeQueryEpoch++
            strokePreview?.text = displayStrokeCode(strokeCode)
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
        strokePreview?.text = ""
        strokeCandidates?.removeAllViews()
    }

    private fun commitStrokeCandidate(candidate: String) {
        if (isSensitiveField(currentInputEditorInfo)) return
        val connection = currentInputConnection ?: return
        val contextBeforeCommit = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        if (!connection.commitText(outputScript(candidate), 1)) return
        if (OpenLessAndroidPreferences.strokeUsageEnabled(this)) {
            userFrequency.record(currentInputEditorInfo?.packageName.orEmpty(), contextBeforeCommit, candidate)
        }
        confirmedText = (confirmedText + candidate).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
    }

    private fun refreshAssociations() {
        if (!OpenLessAndroidPreferences.strokeAssociationEnabled(this)) {
            strokeCandidates?.removeAllViews()
            return
        }
        val context = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        val query = ++phraseQueryEpoch
        if (context.isEmpty()) return
        val packageName = currentInputEditorInfo?.packageName.orEmpty()
        phraseRepository.searchAsync(context, packageName) { result ->
            if (query != phraseQueryEpoch || inputMode != InputMode.STROKE || confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT) != context) return@searchAsync
            strokeCandidates?.removeAllViews()
            result.forEach { candidate ->
                val matchedPrefix = candidate.matchedPrefix.ifEmpty { context }
                val displayText = outputScript(candidate.text)
                val candidateWidth = dp((displayText.codePointCount(0, displayText.length) * 26 + 20).coerceAtLeast(52))
                strokeCandidates?.addView(keyboardKey(displayText, 1f, action = { commitAssociation(candidate.text, matchedPrefix) }).apply {
                    textSize = 18f
                    setSingleLine(true)
                    maxLines = 1
                }, LinearLayout.LayoutParams(candidateWidth, dp(30)))
            }
        }
    }

    private fun commitAssociation(displayText: String, matchedContext: String) {
        if (isSensitiveField(currentInputEditorInfo) || !displayText.startsWith(matchedContext)) return
        val suffix = displayText.removePrefix(matchedContext)
        val connection = currentInputConnection ?: return
        if (suffix.isNotEmpty() && !connection.commitText(outputScript(suffix), 1)) return
        if (OpenLessAndroidPreferences.strokeUsageEnabled(this)) {
            userFrequency.record(currentInputEditorInfo?.packageName.orEmpty(), matchedContext, displayText)
        }
        confirmedText = (confirmedText + suffix).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
    }

    private fun addKeyboardRow(parent: LinearLayout, keys: List<String>) {
        val row = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER
        }
        keys.forEach { key ->
            row.addView(keyboardKey(
                key,
                1f,
                { handleKeyboardKey(key) },
                repeatOnLongPress = key == "⌫",
                repeatAction = { currentInputConnection?.deleteSurroundingText(1, 0) },
            ))
        }
        parent.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    private fun keyboardKey(
        label: String,
        weight: Float,
        action: () -> Unit = {},
        repeatOnLongPress: Boolean = false,
        repeatAction: (() -> Unit)? = null,
        swipeUpAction: (() -> Unit)? = null,
        swipePreview: String? = null,
        microphoneIcon: Boolean = false,
        strokeIconCode: String? = null,
        graphicCode: String? = null,
        graphicActionCode: String? = null,
    ): TextView {
        val keyView = when {
            microphoneIcon -> MicrophoneKeyView(this)
            strokeIconCode != null -> StrokeKeyView(this, strokeIconCode)
            graphicCode != null -> StrokeGlyphView(this, graphicCode)
            graphicActionCode != null -> StrokeActionView(this, graphicActionCode)
            label == "←" || label == "↵" -> ActionSymbolView(this, label)
            else -> TextView(this)
        }
        return keyView.apply {
            text = if (strokeIconCode != null || graphicCode != null || graphicActionCode != null) {
                ""
            } else if ('\n' in label) {
                android.text.SpannableString(label).apply {
                    setSpan(android.text.style.RelativeSizeSpan(0.55f), 0, 1, android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
                }
            } else label
            textSize = if (microphoneIcon) 10f else if (strokeIconCode != null) 1f else if (label == "return") 17f else 22f
            gravity = if (microphoneIcon) android.view.Gravity.TOP or android.view.Gravity.CENTER_HORIZONTAL else android.view.Gravity.CENTER
            if (microphoneIcon) setPadding(0, dp(2), 0, 0)
            setTextColor(Color.rgb(245, 245, 245))
            background = roundedButton(Color.rgb(52, 52, 54), dp(5))
            // Keep the existing palette and geometry, but give each key a subtle raised surface.
            elevation = dp(5).toFloat()
            translationZ = dp(1).toFloat()
            contentDescription = label.ifBlank { ui("空格", "Space") }
            var suppressNextClick = false
            var downY = 0f
            var swipePopup: android.widget.PopupWindow? = null
            setOnClickListener {
                if (suppressNextClick) {
                    suppressNextClick = false
                } else {
                    action()
                }
            }
            val repeatHandler = if (repeatOnLongPress && repeatAction != null) {
                Handler(Looper.getMainLooper())
            } else null
            val repeatRunnable = if (repeatHandler != null && repeatAction != null) {
                object : Runnable {
                    override fun run() {
                        repeatAction.invoke()
                        repeatHandler.postDelayed(this, keyRepeatIntervalMs())
                    }
                }
            } else null
            if (repeatHandler != null && repeatRunnable != null && repeatAction != null) {
                setOnLongClickListener {
                    repeatAction.invoke()
                    repeatHandler.postDelayed(repeatRunnable, keyRepeatIntervalMs())
                    true
                }
            }
            setOnTouchListener { view, event ->
                when (event.actionMasked) {
                    MotionEvent.ACTION_DOWN -> {
                        downY = event.y
                        view.animate()
                            .scaleX(0.97f)
                            .scaleY(0.97f)
                            .translationZ(dp(3).toFloat())
                            .alpha(0.90f)
                            .setDuration(65L)
                            .start()
                        performKeyHaptic()
                    }
                    MotionEvent.ACTION_MOVE -> {
                        if (swipePopup == null && swipeUpAction != null && swipePreview != null && downY - event.y >= dp(10)) {
                            val preview = TextView(this@OpenLessImeService).apply {
                                text = swipePreview
                                textSize = 22f
                                gravity = android.view.Gravity.CENTER
                                setTextColor(Color.WHITE)
                                background = GradientDrawable().apply {
                                    shape = GradientDrawable.RECTANGLE
                                    cornerRadius = dp(10).toFloat()
                                    setColor(Color.argb(205, 65, 65, 65))
                                    setStroke(dp(1), Color.rgb(105, 105, 105))
                                }
                            }
                            swipePopup = android.widget.PopupWindow(
                                preview,
                                dp(64),
                                dp(40),
                                false,
                            ).apply {
                                isClippingEnabled = false
                                elevation = dp(6).toFloat()
                                showAtLocation(
                                    view.rootView,
                                    android.view.Gravity.TOP or android.view.Gravity.CENTER_HORIZONTAL,
                                    0,
                                    dp(8),
                                )
                            }
                        }
                    }
                    MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                        if (event.actionMasked == MotionEvent.ACTION_UP && swipeUpAction != null && downY - event.y >= dp(10)) {
                            swipeUpAction.invoke()
                            suppressNextClick = true
                        }
                        repeatHandler?.let { handler ->
                            repeatRunnable?.let { handler.removeCallbacks(it) }
                        }
                        swipePopup?.dismiss()
                        swipePopup = null
                        view.animate()
                            .scaleX(1f)
                            .scaleY(1f)
                            .translationZ(0f)
                            .alpha(1f)
                            .setDuration(90L)
                            .start()
                    }
                }
                false
            }
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, weight).apply {
                setMargins(dp(3), dp(3), dp(3), dp(3))
            }
        }
    }

    private fun keyRepeatIntervalMs(): Long {
        return getSharedPreferences("openless_ime_ui", MODE_PRIVATE)
            .getLong("key_haptic_repeat_interval_ms", 60L)
            .coerceIn(30L, 200L)
    }

    private fun performKeyHaptic() {
        val preferences = getSharedPreferences("openless_ime_ui", MODE_PRIVATE)
        if (!preferences.getBoolean("key_haptic_enabled", true)) return
        val durationMs = preferences.getLong("key_haptic_duration_ms", 12L).coerceIn(1L, 100L)
        val amplitude = preferences.getInt("key_haptic_amplitude", 55).coerceIn(1, 255)
        runCatching {
            val vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                (getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as VibratorManager).defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                getSystemService(Context.VIBRATOR_SERVICE) as Vibrator
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                vibrator.vibrate(VibrationEffect.createOneShot(durationMs, amplitude))
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(durationMs)
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
        restoreScriptPreference()
        refreshLanguage()
        strokeNumberMode = false
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

    private fun roundedButton(color: Int, radius: Int): android.graphics.drawable.Drawable {
        val lowerEdge = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = radius.toFloat()
            setColor(mixColor(color, Color.BLACK, 0.72f))
        }
        val face = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = radius.toFloat()
            orientation = GradientDrawable.Orientation.TOP_BOTTOM
            colors = intArrayOf(
                mixColor(color, Color.WHITE, 0.09f),
                color,
                mixColor(color, Color.BLACK, 0.18f),
            )
            setStroke(dp(1), mixColor(color, Color.BLACK, 0.55f))
        }
        return android.graphics.drawable.LayerDrawable(arrayOf(lowerEdge, face)).apply {
            // The exposed lower layer forms the reference keyboard's dark keycap step.
            setLayerInset(0, 0, dp(2), 0, 0)
            setLayerInset(1, 0, 0, 0, dp(3))
        }
    }

    private fun mixColor(first: Int, second: Int, amount: Float): Int {
        val ratio = amount.coerceIn(0f, 1f)
        return Color.rgb(
            (Color.red(first) + (Color.red(second) - Color.red(first)) * ratio).toInt(),
            (Color.green(first) + (Color.green(second) - Color.green(first)) * ratio).toInt(),
            (Color.blue(first) + (Color.blue(second) - Color.blue(first)) * ratio).toInt(),
        )
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
            // Always go through OpenLessBackendWarmupActivity so there is ever only
            // one tracked Tauri host: it reuses the existing instance if one is
            // running, or starts fresh otherwise. Starting the bare MainActivity
            // here would spin up an untracked second host and re-run Tauri/Rust
            // setup from scratch, which is what produced the black window /
            // native crash seen when settings was opened before any warmup host
            // was tracked as running.
            OpenLessBackendWarmupActivity.openSettings(this)
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
            // The warmup Activity can crash the whole process before it finishes
            // warming up the backend (native HWUI teardown race). That kills this
            // in-memory throttle along with it, so a plain instance field lets a
            // crash loop retry every few seconds forever. Persist the attempt time
            // so a fresh process still honors the cooldown.
            val runtimePrefs = getSharedPreferences("openless_runtime", MODE_PRIVATE)
            val wallNow = System.currentTimeMillis()
            val lastAttempt = runtimePrefs.getLong(BACKEND_WARMUP_ATTEMPT_KEY, 0L)
            if (wallNow >= lastAttempt && wallNow - lastAttempt < BACKEND_WARMUP_RETRY_DELAY_MS) return
            lastBackendWarmupAt = now
            runtimePrefs.edit().putLong(BACKEND_WARMUP_ATTEMPT_KEY, wallNow).apply()
            android.util.Log.i("OpenLessImeService", "backend is not ready; launching main process", error)
            android.os.Handler(mainLooper).postDelayed({
                runCatching {
                    startActivity(android.content.Intent(this, OpenLessBackendWarmupActivity::class.java).apply {
                        addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                        addFlags(android.content.Intent.FLAG_ACTIVITY_EXCLUDE_FROM_RECENTS)
                        addFlags(android.content.Intent.FLAG_ACTIVITY_NO_ANIMATION)
                    })
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
        private val onModeSelected: (InputMode) -> Unit,
    ) : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val iconPaint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)

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
            // Fixed-size icons (not text) so the label never changes footprint
            // across UI-language switches, which previously shifted the whole
            // toggle/logo header and read as the panel "jumping".
            drawLabelIcon(canvas, strokeIcon(), segmentWidth * 1.5f, centerY, segmentWidth)
            drawLabelIcon(canvas, enIcon(), segmentWidth * 2.5f, centerY, segmentWidth)
        }

        private fun drawLabelIcon(canvas: Canvas, bitmap: android.graphics.Bitmap, centerX: Float, centerY: Float, segmentWidth: Float) {
            val targetHeight = dp(20)
            val maxWidth = segmentWidth - dp(6)
            val scale = minOf(targetHeight / bitmap.height, maxWidth / bitmap.width)
            val w = bitmap.width * scale
            val h = bitmap.height * scale
            val dst = android.graphics.RectF(centerX - w / 2f, centerY - h / 2f, centerX + w / 2f, centerY + h / 2f)
            canvas.drawBitmap(bitmap, null, dst, iconPaint)
        }

        private fun strokeIcon(): android.graphics.Bitmap {
            strokeBitmap?.let { return it }
            return android.graphics.BitmapFactory.decodeResource(resources, R.drawable.toggle_stroke)
                .also { strokeBitmap = it }
        }

        private fun enIcon(): android.graphics.Bitmap {
            enBitmap?.let { return it }
            return android.graphics.BitmapFactory.decodeResource(resources, R.drawable.toggle_en)
                .also { enBitmap = it }
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

        companion object {
            private var strokeBitmap: android.graphics.Bitmap? = null
            private var enBitmap: android.graphics.Bitmap? = null
        }
    }

    /** Central stroke keys use a canvas glyph so their proportions do not depend on a font. */
    private class StrokeGlyphView(
        context: android.content.Context,
        private val glyphCode: String,
    ) : TextView(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.rgb(232, 232, 232)
            textAlign = Paint.Align.CENTER
            typeface = android.graphics.Typeface.create("sans-serif", android.graphics.Typeface.NORMAL)
        }

        override fun onDraw(canvas: Canvas) {
            val unit = minOf(width, height).coerceAtLeast(1) / 100f
            val x = width / 2f
            paint.style = Paint.Style.FILL
            paint.textSize = 19f * unit
            val text = when (glyphCode) {
                "*" -> "通配"
                " " -> "分词"
                ":" -> ":"
                ";" -> ";"
                "symbols" -> "符号"
                "script" -> "繁"
                else -> glyphCode
            }
            val top = when (glyphCode) {
                "*" -> "6"
                " " -> "7"
                ":" -> "8"
                ";" -> "9"
                else -> ""
            }
            if (top.isNotEmpty()) {
                paint.color = Color.rgb(155, 155, 155)
                paint.textSize = 20f * unit
                // Matches the "0" key's TextView-rendered top-gravity number, which
                // sits lower than this baseline-based canvas position implied.
                canvas.drawText(top, x, 32f * unit, paint)
                paint.color = Color.rgb(232, 232, 232)
            }
            paint.textSize = if (text in listOf("符号", "通配", "分词", "繁")) 33f * unit else 27f * unit
            canvas.drawText(text, x, if (top.isEmpty()) 61f * unit else 76f * unit, paint)
        }
    }

    /** Red actions are also custom-drawn to keep the reference glyph geometry stable. */
    private class StrokeActionView(
        context: android.content.Context,
        private val actionCode: String,
    ) : TextView(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            style = Paint.Style.STROKE
            strokeWidth = 5.2f
            strokeCap = Paint.Cap.SQUARE
            strokeJoin = Paint.Join.MITER
        }
        private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            textAlign = Paint.Align.CENTER
            typeface = android.graphics.Typeface.create("sans-serif", android.graphics.Typeface.NORMAL)
        }

        override fun onDraw(canvas: Canvas) {
            val u = minOf(width, height).coerceAtLeast(1) / 100f
            val cx = width / 2f
            val cy = height / 2f
            paint.strokeWidth = 5.2f * u
            when (actionCode) {
                "←" -> {
                    canvas.drawLine(cx - 23f * u, cy, cx + 23f * u, cy, paint)
                    canvas.drawLine(cx - 23f * u, cy, cx - 8f * u, cy - 12f * u, paint)
                    canvas.drawLine(cx - 23f * u, cy, cx - 8f * u, cy + 12f * u, paint)
                }
                "↵" -> {
                    val path = Path().apply {
                        moveTo(cx + 23f * u, cy - 14f * u)
                        lineTo(cx + 23f * u, cy + 5f * u)
                        quadTo(cx + 23f * u, cy + 15f * u, cx + 13f * u, cy + 15f * u)
                        lineTo(cx - 22f * u, cy + 15f * u)
                    }
                    canvas.drawPath(path, paint)
                    canvas.drawLine(cx - 22f * u, cy + 15f * u, cx - 10f * u, cy + 5f * u, paint)
                    canvas.drawLine(cx - 22f * u, cy + 15f * u, cx - 10f * u, cy + 25f * u, paint)
                }
                else -> {
                    textPaint.textSize = if (actionCode == "清除") 32f * u else 30f * u
                    canvas.drawText(actionCode, cx, cy - (textPaint.ascent() + textPaint.descent()) / 2f, textPaint)
                }
            }
        }
    }

    private class StrokeKeyView(
        context: android.content.Context,
        private val strokeCode: String,
    ) : TextView(context) {
        private val numberPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.rgb(155, 155, 155)
            textAlign = Paint.Align.CENTER
            typeface = android.graphics.Typeface.create("sans-serif", android.graphics.Typeface.NORMAL)
        }
        private val strokePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.rgb(232, 232, 232)
            style = Paint.Style.STROKE
            strokeCap = Paint.Cap.ROUND
            strokeJoin = Paint.Join.ROUND
        }

        override fun onDraw(canvas: Canvas) {
            val unit = minOf(width, height).coerceAtLeast(1) / 100f
            val centerX = width / 2f
            val topNumber = when (strokeCode) {
                "h" -> "1"
                "s" -> "2"
                "p" -> "3"
                "n" -> "4"
                else -> "5"
            }
            numberPaint.textSize = 20f * unit
            // Matches the "0" key's TextView-rendered top-gravity number, which
            // sits lower than this baseline-based canvas position implied.
            canvas.drawText(topNumber, centerX, 32f * unit, numberPaint)

            strokePaint.strokeWidth = 3.2f * unit
            val stroke = Path()
            when (strokeCode) {
                "h" -> {
                    stroke.moveTo(centerX - 20f * unit, 62f * unit)
                    stroke.lineTo(centerX + 20f * unit, 62f * unit)
                }
                "s" -> {
                    stroke.moveTo(centerX, 42f * unit)
                    stroke.lineTo(centerX, 79f * unit)
                }
                "p" -> {
                    stroke.moveTo(centerX + 13f * unit, 43f * unit)
                    stroke.cubicTo(centerX + 10f * unit, 55f * unit, centerX - 2f * unit, 72f * unit, centerX - 16f * unit, 79f * unit)
                }
                "n" -> {
                    stroke.moveTo(centerX - 11f * unit, 48f * unit)
                    stroke.cubicTo(centerX - 4f * unit, 57f * unit, centerX + 3f * unit, 68f * unit, centerX + 11f * unit, 76f * unit)
                }
                else -> {
                    // Draw the reference's折笔 directly: a clean descending stroke
                    // from upper-right to lower-left, then a horizontal finish to the right.
                    stroke.moveTo(centerX + 12f * unit, 44f * unit)
                    stroke.lineTo(centerX - 8f * unit, 75f * unit)
                    stroke.lineTo(centerX + 17f * unit, 75f * unit)
                }
            }
            canvas.drawPath(stroke, strokePaint)
        }
    }

    private class ActionSymbolView(
        context: android.content.Context,
        private val symbol: String,
    ) : TextView(context) {
        private val symbolPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            style = Paint.Style.STROKE
            strokeCap = Paint.Cap.SQUARE
            strokeJoin = Paint.Join.ROUND
        }

        override fun onDraw(canvas: Canvas) {
            val unit = minOf(width, height).coerceAtLeast(1) / 100f
            val centerX = width / 2f
            val centerY = height / 2f
            symbolPaint.strokeWidth = 5.5f * unit
            if (symbol == "←") {
                val left = centerX - 27f * unit
                val right = centerX + 27f * unit
                canvas.drawLine(left + 13f * unit, centerY, right, centerY, symbolPaint)
                val arrow = Path().apply {
                    moveTo(left + 13f * unit, centerY)
                    lineTo(left + 27f * unit, centerY - 12f * unit)
                    moveTo(left + 13f * unit, centerY)
                    lineTo(left + 27f * unit, centerY + 12f * unit)
                }
                canvas.drawPath(arrow, symbolPaint)
            } else {
                val path = Path().apply {
                    moveTo(centerX + 29f * unit, centerY - 14f * unit)
                    lineTo(centerX + 29f * unit, centerY + 8f * unit)
                    lineTo(centerX + 21f * unit, centerY + 18f * unit)
                    lineTo(centerX + 7f * unit, centerY + 18f * unit)
                    lineTo(centerX - 25f * unit, centerY + 18f * unit)
                }
                canvas.drawPath(path, symbolPaint)
                val arrow = Path().apply {
                    moveTo(centerX - 25f * unit, centerY + 18f * unit)
                    lineTo(centerX - 12f * unit, centerY + 8f * unit)
                    moveTo(centerX - 25f * unit, centerY + 18f * unit)
                    lineTo(centerX - 12f * unit, centerY + 28f * unit)
                }
                canvas.drawPath(arrow, symbolPaint)
            }
        }
    }

    private class MicrophoneKeyView(context: android.content.Context) : TextView(context) {
        private val microphonePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.rgb(190, 190, 190)
            strokeCap = Paint.Cap.ROUND
            strokeJoin = Paint.Join.ROUND
            style = Paint.Style.FILL
        }
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            // Keep the same proportions as VoiceButton, but leave room for the small 0 label.
            val unit = minOf(width, height).coerceAtLeast(1) / 160f
            val centerX = width / 2f
            val centerY = height * 0.56f
            val bodyWidth = 11.5f * unit
            val bodyTop = centerY - 24f * unit
            val bodyBottom = centerY + 20f * unit
            canvas.drawRoundRect(
                centerX - bodyWidth,
                bodyTop,
                centerX + bodyWidth,
                bodyBottom,
                bodyWidth,
                bodyWidth,
                microphonePaint,
            )
            microphonePaint.style = Paint.Style.STROKE
            microphonePaint.strokeWidth = 4.5f * unit
            val arc = Path().apply {
                moveTo(centerX - 19f * unit, centerY + 8f * unit)
                cubicTo(
                    centerX - 18f * unit, centerY + 25f * unit,
                    centerX - 9f * unit, centerY + 30f * unit,
                    centerX, centerY + 30f * unit,
                )
                cubicTo(
                    centerX + 9f * unit, centerY + 30f * unit,
                    centerX + 18f * unit, centerY + 25f * unit,
                    centerX + 19f * unit, centerY + 8f * unit,
                )
            }
            canvas.drawPath(arc, microphonePaint)
            canvas.drawLine(
                centerX,
                centerY + 30f * unit,
                centerX,
                centerY + 39f * unit,
                microphonePaint,
            )
            microphonePaint.style = Paint.Style.FILL
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
        private const val BACKEND_WARMUP_ATTEMPT_KEY = "backend_warmup_attempt_wall_time"
        private const val BACKEND_WARMUP_RETRY_DELAY_MS = 30_000L
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
