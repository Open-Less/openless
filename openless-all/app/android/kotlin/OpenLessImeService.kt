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
    private enum class ShiftState { OFF, SHIFT_ONCE, CAPS_LOCK }

    private var sessionEpoch = 0L
    private var recording = false
    private var processing = false
    private var inputMode = InputMode.VOICE
    private var symbolMode = false
    private var strokeNumberMode = false
    private var numberSymbolMode = false
    private var symbolPageIndex = 0
    private val numberPanelSymbolPages = listOf(
        listOf("、", "。", "，", "；", "：", "？", "！", "…", "—", "～", "·", "（"),
        listOf("）", "《", "》", "“", "”", "‘", "’", "【", "】", "「", "」", "￥"),
        listOf("%", "#", "&", "*", "=", "/", "\\", "<", ">", "^", "_", "|"),
    )
    private var punctuationGroupIndex = 0
    private val punctuationGroups = listOf(
        listOf(",", "°", "?", "!", "~"),
        listOf(".", "、", ";", ":", "\""),
        listOf("(", ")", "[", "]", "-"),
        listOf("@", "#", "$", "%", "&"),
        listOf("*", "+", "=", "/", "_"),
    )
    private var traditionalOutput = false
    private var shiftState = ShiftState.OFF
    private var state = "idle"
    private var currentMessage = "点击开始说话"
    private var status: TextView? = null
    private var voiceButton: VoiceButton? = null
    private var englishUi = false
    private val simplifiedToTraditional by lazy { Transliterator.getInstance("Hans-Hant") }
    private val strokeRepository by lazy { StrokeInputRepository(this) }
    private val phraseRepository by lazy { StrokePhraseRepository(this) }
    private val userFrequency by lazy { StrokeUserFrequency(this) }
    private var strokeCode = ""
    private var strokeQueryEpoch = 0L
    // In-memory word-segmentation buffer: characters the user has marked with
    // 分词 while composing a multi-character word. Never touches the actual
    // input connection until the assembled word (or its final character) is
    // committed — see segmentStroke()/commitWord().
    private val wordSegments = mutableListOf<String>()
    private var lastStrokeCandidates: List<String> = emptyList()
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
        val root = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
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
        // @ / return / backspace all use the same keyboardKey() styling as
        // the stroke panel's own keys — rounded corners + elevation shadow —
        // instead of the previous ad-hoc pill backgrounds, so this row reads
        // as consistent with the rest of the keyboard.
        // Flat pill style matching the mic capsule below (Color.rgb(54,54,54),
        // fully rounded, no elevation), not the stroke panel's raised-shadow
        // keys — this row sits directly above the mic and reads oddly if it
        // looks like a separate raised keyboard instead of part of this page.
        val footerButtonHeight = dp(48)
        fun flattenFooterButton(view: TextView) {
            view.elevation = 0f
            view.translationZ = 0f
            // A single flat fill, not roundedButton()'s layered "keycap" look
            // (gradient face + exposed darker step) — genuinely flat, matching
            // the mic capsule's own plain drawRoundRect fill exactly.
            view.background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = (footerButtonHeight / 2).toFloat()
                setColor(Color.rgb(54, 54, 54))
            }
        }
        val atButton = keyboardKey("@", 1f, action = { currentInputConnection?.commitText("@", 1) }).apply {
            textSize = 20f
            contentDescription = ui("输入 @", "Insert at sign")
            layoutParams = LinearLayout.LayoutParams(dp(84), footerButtonHeight)
            flattenFooterButton(this)
        }
        val returnButton = keyboardKey("return", 1f, action = { sendEnterKey() }).apply {
            textSize = 18f
            contentDescription = ui("回车", "Return")
            layoutParams = LinearLayout.LayoutParams(dp(120), footerButtonHeight)
            flattenFooterButton(this)
        }
        val backspaceButton = keyboardKey(
            "⌫",
            1f,
            action = { currentInputConnection?.deleteSurroundingText(1, 0) },
            repeatOnLongPress = true,
            repeatAction = { currentInputConnection?.deleteSurroundingText(1, 0) },
        ).apply {
            textSize = 22f
            contentDescription = ui("退格", "Backspace")
            layoutParams = LinearLayout.LayoutParams(dp(84), footerButtonHeight)
            flattenFooterButton(this)
        }
        val returnHolder = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER
            addView(returnButton)
        }
        footer.addView(atButton)
        footer.addView(returnHolder, LinearLayout.LayoutParams(0, dp(84), 1f))
        footer.addView(backspaceButton)
        root.addView(footer, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            dp(84),
        ))
        return root
    }

    private fun refreshInputView() {
        setInputView(onCreateInputView())
    }

    private fun buildModeToggle(): View = ModeToggle(this, inputMode) { selected -> selectInputMode(selected) }

    private fun selectInputMode(selected: InputMode) {
        if (recording || processing) cancelDictation()
        inputMode = selected
        saveInputMode(selected)
        symbolMode = false
        strokeNumberMode = false
        numberSymbolMode = false
        symbolPageIndex = 0
        punctuationGroupIndex = 0
        shiftState = ShiftState.OFF
        strokeCode = ""
        strokeQueryEpoch++
        confirmedText = ""
        phraseQueryEpoch++
        refreshInputView()
    }

    /**
     * Left/right swipe on any panel steps through the same Voice-Stroke-English
     * order as the toggle switch, clamped at both ends (no wraparound) —
     * swiping left keeps landing on Voice, right keeps landing on English.
     */
    private fun swipeInputMode(direction: Int) {
        val modes = InputMode.entries
        val next = modes[(inputMode.ordinal + direction).coerceIn(0, modes.lastIndex)]
        if (next != inputMode) selectInputMode(next)
    }

    private fun buildKeyboardView(): View {
        val root = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
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
            shiftState = ShiftState.OFF
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
        val root = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
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
        // Swiping the rail up/down cycles through punctuationGroups instead of
        // scrolling — one swipe always advances exactly one group.
        val punctuation = SwipeRail(this) { direction ->
            val count = punctuationGroups.size
            punctuationGroupIndex = ((punctuationGroupIndex + direction) % count + count) % count
            refreshInputView()
        }.apply {
            orientation = LinearLayout.VERTICAL
            gravity = android.view.Gravity.CENTER
            setPadding(dp(2), dp(2), dp(2), dp(2))
            background = roundedButton(Color.rgb(45, 45, 45), dp(4))
        }
        punctuationGroups[punctuationGroupIndex].forEachIndexed { index, mark ->
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
                        "symbols" -> {
                            strokeNumberMode = true
                            numberSymbolMode = true
                            symbolPageIndex = 0
                            refreshInputView()
                        }
                        " " -> segmentStroke()
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
        listOf("←" to { deleteStroke() }, "↵" to { sendEnterKey() }, "清除" to { clearStrokes() }, "123" to {
            strokeNumberMode = true
            numberSymbolMode = false
            symbolPageIndex = 0
            refreshInputView()
        }).forEach { (label, action) ->
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
        val root = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
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

        // Backspace/Enter/Voice/Return/Symbols stay in Chinese regardless of UI
        // language: they're functional keys on the number/symbol panel, not
        // content the user is composing.
        val rows = if (numberSymbolMode) {
            val page = numberPanelSymbolPages[symbolPageIndex.coerceIn(numberPanelSymbolPages.indices)]
            listOf(
                page.subList(0, 4) + "退格",
                page.subList(4, 8) + "回车",
                page.subList(8, 12) + "语音",
                listOf("▲", "${symbolPageIndex + 1}/${numberPanelSymbolPages.size}", "▼", "数字", "返回"),
            )
        } else {
            listOf(
                listOf("@", "1", "2", "3", "退格"),
                listOf(":", "4", "5", "6", "回车"),
                listOf(",", "7", "8", "9", "语音"),
                // "0" sits directly under "8", flanked by +/. — the same
                // layout convention as a phone dial pad's "* 0 #" row.
                listOf("+", "0", ".", "符号", "返回"),
            )
        }
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
                        numberSymbolMode = false
                        symbolPageIndex = 0
                        refreshInputView()
                    })
                    // Symbol-page navigation only replaces row 3's first three
                    // cells (previously "+ - .") while in symbol mode; the
                    // middle cell is just a page indicator, not clickable.
                    rowIndex == 3 && index == 0 && numberSymbolMode -> ({
                        symbolPageIndex = (symbolPageIndex - 1 + numberPanelSymbolPages.size) % numberPanelSymbolPages.size
                        refreshInputView()
                    })
                    rowIndex == 3 && index == 1 && numberSymbolMode -> ({})
                    rowIndex == 3 && index == 2 && numberSymbolMode -> ({
                        symbolPageIndex = (symbolPageIndex + 1) % numberPanelSymbolPages.size
                        refreshInputView()
                    })
                    rowIndex == 3 && index == 3 -> ({
                        numberSymbolMode = !numberSymbolMode
                        symbolPageIndex = 0
                        refreshInputView()
                    })
                    rowIndex == 3 && isAction -> ({
                        strokeNumberMode = false
                        numberSymbolMode = false
                        symbolPageIndex = 0
                        refreshInputView()
                    })
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
            lastStrokeCandidates = emptyList()
            renderCandidateRow(emptyList())
        }
        strokeCode += stroke
        updateStrokePreview()
        refreshStrokeCandidates(strokeCode)
    }

    /** Marks the current character as a word segment without committing it yet. */
    private fun segmentStroke() {
        if (strokeCode.isEmpty()) return
        val candidate = lastStrokeCandidates.firstOrNull() ?: return
        wordSegments.add(candidate)
        strokeCode = ""
        strokeQueryEpoch++
        lastStrokeCandidates = emptyList()
        updateStrokePreview()
        renderCandidateRow(emptyList())
    }

    private fun updateStrokePreview() {
        strokePreview?.text = wordSegments.joinToString("") + displayStrokeCode(strokeCode)
    }

    private fun refreshStrokeCandidates(code: String) {
        val query = ++strokeQueryEpoch
        strokeRepository.searchAsync(code) { result ->
            if (query != strokeQueryEpoch || inputMode != InputMode.STROKE) return@searchAsync
            lastStrokeCandidates = result
            renderCandidateRow(result)
        }
    }

    /**
     * Renders the stroke candidate row: a leading "commit the whole word"
     * button for any segments marked via 分词 (if present), followed by the
     * single-character candidates for the character currently being typed.
     */
    private fun renderCandidateRow(strokeMatches: List<String>) {
        strokeCandidates?.removeAllViews()
        if (wordSegments.isNotEmpty()) {
            val word = wordSegments.joinToString("")
            val displayWord = outputScript(word)
            val wordWidth = dp((displayWord.codePointCount(0, displayWord.length) * 26 + 20).coerceAtLeast(52))
            strokeCandidates?.addView(keyboardKey(displayWord, 1f, action = { commitWord(word) }).apply {
                textSize = 18f
                setSingleLine(true)
                maxLines = 1
            }, LinearLayout.LayoutParams(wordWidth, dp(30)))
        }
        strokeMatches.forEach { candidate ->
            strokeCandidates?.addView(keyboardKey(outputScript(candidate), 1f, action = { commitStrokeCandidate(candidate) }).apply {
                textSize = 18f
                setSingleLine(true)
                maxLines = 1
            }, LinearLayout.LayoutParams(dp(44), dp(30)))
        }
    }

    private fun deleteStroke() {
        if (strokeCode.isNotEmpty()) {
            strokeCode = strokeCode.dropLast(1)
            strokeQueryEpoch++
            updateStrokePreview()
            if (strokeCode.isNotEmpty()) {
                appendStroke("")
            } else {
                lastStrokeCandidates = emptyList()
                renderCandidateRow(emptyList())
            }
        } else if (wordSegments.isNotEmpty()) {
            wordSegments.removeAt(wordSegments.lastIndex)
            updateStrokePreview()
            renderCandidateRow(emptyList())
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
        wordSegments.clear()
        lastStrokeCandidates = emptyList()
        strokePreview?.text = ""
        strokeCandidates?.removeAllViews()
    }

    /** Commits the current character together with any segments already marked via 分词. */
    private fun commitStrokeCandidate(candidate: String) {
        commitWord((wordSegments + candidate).joinToString(""))
    }

    private fun commitWord(word: String) {
        if (word.isEmpty() || isSensitiveField(currentInputEditorInfo)) return
        val connection = currentInputConnection ?: return
        val contextBeforeCommit = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        if (!connection.commitText(outputScript(word), 1)) return
        if (OpenLessAndroidPreferences.strokeUsageEnabled(this)) {
            userFrequency.record(currentInputEditorInfo?.packageName.orEmpty(), contextBeforeCommit, word)
        }
        confirmedText = (confirmedText + word).takeLast(MAX_ASSOCIATION_CONTEXT)
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
            // The key's identity (passed to handleKeyboardKey) always stays
            // lowercase; only the displayed label follows shiftState, so the
            // keyboard visibly shows what tapping it will actually type.
            val displayLabel = if (shiftState != ShiftState.OFF && key.length == 1 && key[0].isLetter()) {
                key.uppercase()
            } else {
                key
            }
            row.addView(keyboardKey(
                displayLabel,
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
            label == "⇧" -> ShiftKeyView(this, shiftState)
            else -> TextView(this)
        }
        return keyView.apply {
            text = if (strokeIconCode != null || graphicCode != null || graphicActionCode != null || label == "⇧") {
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
                // Cycles lowercase -> capitalize-next -> caps-lock -> lowercase.
                shiftState = when (shiftState) {
                    ShiftState.OFF -> ShiftState.SHIFT_ONCE
                    ShiftState.SHIFT_ONCE -> ShiftState.CAPS_LOCK
                    ShiftState.CAPS_LOCK -> ShiftState.OFF
                }
                refreshInputView()
            }
            "ABC" -> {
                symbolMode = false
                shiftState = ShiftState.OFF
                refreshInputView()
            }
            else -> {
                val text = if (shiftState != ShiftState.OFF && key.length == 1) key.uppercase() else key
                currentInputConnection?.commitText(text, 1)
                // Caps-lock stays on for every letter; a one-shot shift only
                // capitalizes the single letter that was just typed.
                if (shiftState == ShiftState.SHIFT_ONCE) {
                    shiftState = ShiftState.OFF
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
        numberSymbolMode = false
        symbolPageIndex = 0
        startRuntimeService()
        sessionEpoch++
        confirmedText = ""
        phraseQueryEpoch++
        strokeCode = ""
        wordSegments.clear()
        lastStrokeCandidates = emptyList()
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

    private fun ensureBackendReady() = OpenLessBackendWarmupActivity.ensureBackendReady(this)

    private fun stopRuntimeService() {
        runCatching { stopService(android.content.Intent(this, OpenLessRuntimeService::class.java)) }
    }

    /**
     * A vertical stack of keys that also recognizes a whole-column swipe:
     * once a drag exceeds touch slop, this claims the gesture from whichever
     * child key the drag started on (via onInterceptTouchEvent) so a single
     * swipe anywhere in the column advances exactly one group, rather than
     * being absorbed as a press/drag on one key.
     */
    private class SwipeRail(
        context: android.content.Context,
        private val onSwipe: (Int) -> Unit,
    ) : LinearLayout(context) {
        private var startY = 0f
        private var intercepting = false
        private val touchSlop = android.view.ViewConfiguration.get(context).scaledTouchSlop

        init {
            excludeFromSystemGestures(this)
        }

        override fun onInterceptTouchEvent(ev: android.view.MotionEvent): Boolean {
            when (ev.actionMasked) {
                android.view.MotionEvent.ACTION_DOWN -> {
                    startY = ev.y
                    intercepting = false
                }
                android.view.MotionEvent.ACTION_MOVE -> {
                    if (!intercepting && kotlin.math.abs(ev.y - startY) > touchSlop) {
                        intercepting = true
                        // Stop the outer SwipeModeContainer (or any other
                        // ancestor) from also trying to claim this gesture
                        // once we've committed to handling it as a vertical
                        // drag — otherwise a still-moving finger can flip
                        // back and forth between the two interceptors.
                        parent?.requestDisallowInterceptTouchEvent(true)
                    }
                }
            }
            return intercepting
        }

        override fun onTouchEvent(event: android.view.MotionEvent): Boolean {
            if (event.actionMasked == android.view.MotionEvent.ACTION_UP ||
                event.actionMasked == android.view.MotionEvent.ACTION_CANCEL
            ) {
                val dy = startY - event.y
                // Swiping up (finger moves toward the top, dy > 0) advances to
                // the next group; swiping down goes back to the previous one.
                // Posted rather than called inline: onSwipe rebuilds the whole
                // input view, and doing that synchronously while this touch
                // gesture is still unwinding through the view we're about to
                // replace is what caused the panel to visibly flicker
                // closed/open under the finger.
                if (kotlin.math.abs(dy) > touchSlop) post { onSwipe(if (dy > 0) 1 else -1) }
                intercepting = false
            }
            return true
        }
    }

    /**
     * Horizontal swipe-to-switch-mode, applied to every panel's root
     * container: a left/right drag anywhere that isn't already claimed by a
     * vertical gesture (like SwipeRail) steps the input mode toward
     * Voice/English, matching the toggle switch's order.
     */
    private class SwipeModeContainer(
        context: android.content.Context,
        private val onSwipe: (Int) -> Unit,
    ) : LinearLayout(context) {
        private var startX = 0f
        private var startY = 0f
        private var intercepting = false
        private val touchSlop = android.view.ViewConfiguration.get(context).scaledTouchSlop

        init {
            excludeFromSystemGestures(this)
        }

        override fun onInterceptTouchEvent(ev: android.view.MotionEvent): Boolean {
            when (ev.actionMasked) {
                android.view.MotionEvent.ACTION_DOWN -> {
                    startX = ev.x
                    startY = ev.y
                    intercepting = false
                }
                android.view.MotionEvent.ACTION_MOVE -> {
                    val dx = ev.x - startX
                    val dy = ev.y - startY
                    // Require a clearly horizontal drag so this never steals a
                    // vertical gesture meant for a nested SwipeRail.
                    if (!intercepting && kotlin.math.abs(dx) > touchSlop && kotlin.math.abs(dx) > kotlin.math.abs(dy) * 1.5f) {
                        intercepting = true
                        parent?.requestDisallowInterceptTouchEvent(true)
                    }
                }
            }
            return intercepting
        }

        override fun onTouchEvent(event: android.view.MotionEvent): Boolean {
            if (event.actionMasked == android.view.MotionEvent.ACTION_UP ||
                event.actionMasked == android.view.MotionEvent.ACTION_CANCEL
            ) {
                val dx = event.x - startX
                // Swiping left (finger moves toward the start, dx < 0) steps
                // toward Voice; swiping right steps toward English. (This is
                // inverted from the raw dx sign — on-device testing showed
                // dx < 0 landing on English, so the mapping below matches what
                // actually happens rather than the "obvious" sign.)
                // Posted for the same reason as SwipeRail: avoid rebuilding
                // the input view synchronously mid-gesture.
                if (kotlin.math.abs(dx) > touchSlop) post { onSwipe(if (dx < 0) 1 else -1) }
                intercepting = false
            }
            return true
        }
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
                    // Traced from the reference glyph: straight down for most
                    // of the stroke, hooking left only at the very end — a "J"
                    // shape, not a curve along its whole length.
                    stroke.moveTo(centerX - 2f * unit, 38f * unit)
                    stroke.cubicTo(
                        centerX - 2f * unit, 55f * unit,
                        centerX - 5f * unit, 68f * unit,
                        centerX - 16f * unit, 77f * unit,
                    )
                }
                "n" -> {
                    // Traced from the reference glyph: a short, straight tick,
                    // not a long curve — 丶 is a dot/dian, not a swooping line.
                    stroke.moveTo(centerX - 7f * unit, 50f * unit)
                    stroke.lineTo(centerX + 6f * unit, 67f * unit)
                }
                else -> {
                    // Traced from the reference glyph: a diagonal down-left to
                    // a corner, then a horizontal finish to the right (乙/横折).
                    stroke.moveTo(centerX + 1f * unit, 42f * unit)
                    stroke.lineTo(centerX - 13f * unit, 71f * unit)
                    stroke.lineTo(centerX + 15f * unit, 71f * unit)
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

    /**
     * The English keyboard's shift key draws one of three distinct icons so
     * its state is visible at a glance: a hollow arrow (lowercase), a solid
     * arrow (capitalize the next letter only), or a solid arrow with an
     * underline bar (caps lock — every letter is capitalized until toggled
     * off again).
     */
    private class ShiftKeyView(
        context: android.content.Context,
        private val state: ShiftState,
    ) : TextView(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            strokeJoin = Paint.Join.ROUND
            strokeCap = Paint.Cap.ROUND
        }

        override fun onDraw(canvas: Canvas) {
            val unit = minOf(width, height).coerceAtLeast(1) / 100f
            val cx = width / 2f
            val cy = height / 2f - 6f * unit
            val arrow = Path().apply {
                moveTo(cx, cy - 20f * unit)
                lineTo(cx + 17f * unit, cy - 2f * unit)
                lineTo(cx + 8f * unit, cy - 2f * unit)
                lineTo(cx + 8f * unit, cy + 15f * unit)
                lineTo(cx - 8f * unit, cy + 15f * unit)
                lineTo(cx - 8f * unit, cy - 2f * unit)
                lineTo(cx - 17f * unit, cy - 2f * unit)
                close()
            }
            when (state) {
                ShiftState.OFF -> {
                    paint.style = Paint.Style.STROKE
                    paint.strokeWidth = 3f * unit
                    paint.color = Color.rgb(190, 190, 190)
                }
                ShiftState.SHIFT_ONCE, ShiftState.CAPS_LOCK -> {
                    paint.style = Paint.Style.FILL
                    paint.color = Color.rgb(245, 245, 245)
                }
            }
            canvas.drawPath(arrow, paint)
            if (state == ShiftState.CAPS_LOCK) {
                paint.style = Paint.Style.FILL
                paint.color = Color.rgb(245, 245, 245)
                canvas.drawRoundRect(
                    cx - 17f * unit, cy + 22f * unit, cx + 17f * unit, cy + 28f * unit,
                    3f * unit, 3f * unit, paint,
                )
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
                // Longer bar group (17 vs the original 9) with a genuine
                // left-flowing traveling wave — as `phase` advances, the sine
                // term's peak visibly drifts from higher to lower bar
                // indices, i.e. right to left — layered on top of the same
                // bell-shaped envelope (tallest in the middle) as before.
                // Gap is derived from bar count so the group always spans the
                // same ~86% of the view width regardless of how many bars.
                val live = (audioLevel * 1.15f).coerceIn(0f, 1f)
                val barCount = 17
                val envelopeCenter = (barCount - 1) / 2f
                val gap = width * 0.86f / (barCount - 1)
                val startX = centerX - gap * (barCount - 1) / 2f
                for (index in 0 until barCount) {
                    val x = startX + index * gap
                    val distanceFromCenter = kotlin.math.abs(index - envelopeCenter) / envelopeCenter
                    val envelope = 1f - distanceFromCenter * distanceFromCenter * 0.75f
                    val flow = 0.55f + 0.45f * kotlin.math.sin(
                        (phase * 2.2f + index * 0.9f).toDouble(),
                    ).toFloat()
                    val halfHeight = minOf(height * 0.95f, dp(66).toFloat()) *
                        (0.035f + live * 0.965f) * envelope * flow
                    paint.color = Color.rgb(222, 222, 222)
                    paint.strokeWidth = dp(3).toFloat()
                    canvas.drawLine(x, centerY - halfHeight, x, centerY + halfHeight, paint)
                }
            } else if (isProcessing) {
                // Analysis state uses the same restrained monochrome palette;
                // the ring of dots keeps rotating exactly as before, and on
                // top of that the whole ring's radius now breathes — growing
                // then shrinking together as one — rather than each dot
                // sizing itself independently off its own angle.
                val colors = intArrayOf(
                    Color.rgb(245, 245, 245), Color.rgb(205, 205, 205),
                    Color.rgb(170, 170, 170), Color.rgb(235, 235, 235),
                    Color.rgb(190, 190, 190), Color.rgb(220, 220, 220),
                )
                val baseOrbit = minOf(width * 0.28f, height * 0.52f)
                val baseDotRadius = minOf(width * 0.055f, height * 0.15f)
                val breathe = 0.6f + 0.4f * ((1f + kotlin.math.sin((phase * 0.5f).toDouble()).toFloat()) / 2f)
                val orbit = baseOrbit * breathe
                val dotRadius = baseDotRadius * breathe
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

        /**
         * Opts a view out of Android's system gesture navigation (back/home
         * edge swipes) so our own in-keyboard swipe gestures aren't preempted
         * by the OS, which otherwise reads as the IME window flickering
         * closed and reopening under the user's finger.
         */
        fun excludeFromSystemGestures(view: View) {
            if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.Q) return
            view.addOnLayoutChangeListener { v, left, top, right, bottom, _, _, _, _ ->
                if (right > left && bottom > top) {
                    v.systemGestureExclusionRects = listOf(android.graphics.Rect(0, 0, right - left, bottom - top))
                }
            }
        }

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
