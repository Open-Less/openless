package com.openless.app

import android.Manifest
import android.content.Context
import android.content.res.Configuration
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
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/** Minimal system IME surface. Voice transport is intentionally added in a later phase. */
class OpenLessImeService : InputMethodService(), OpenLessOverlayBridge.OverlayStateListener {
    private enum class InputMode { VOICE, STROKE, CLIPBOARD, ENGLISH }
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
    // Clipboard panel state: whether ← → ↑ ↓ extend the selection (like
    // holding Shift on a physical keyboard) instead of just moving the
    // cursor, whether the history browser sub-panel is showing instead of
    // the direction-pad grid, and which category tab is selected there.
    private var clipboardSelectionMode = false
    // The fixed end and the moving end of the in-progress selection, set the
    // first time an arrow key is pressed after "选择" turns on; cleared
    // whenever selection mode turns off so the next selection starts fresh
    // from wherever the cursor happens to be then.
    private var clipboardSelectionAnchor = -1
    private var clipboardSelectionActive = -1
    private var clipboardHistoryMode = false
    private var clipboardHistoryCategory = OpenLessClipboardHistory.Category.ALL
    private var shiftState = ShiftState.OFF
    private var state = "idle"
    private var currentMessage = "点击开始说话"
    private var status: TextView? = null
    private var voiceButton: VoiceButton? = null
    // Silence-detection for the main voice panel: if the mic capture never
    // reports a meaningful level for a while after recording starts, the
    // audio link is probably broken upstream (muted mic, dead capture
    // session, etc.) even though the UI otherwise looks like it's recording.
    private var voiceLinkWarning: TextView? = null
    private var recordingStartedAtMs = 0L
    private var maxObservedLevelThisSession = 0f
    // The exact text this dictation session committed, so the undo/redo
    // toggle can remove/restore precisely that span rather than guessing.
    private var lastDictationText: String? = null
    private var lastDictationEpoch: Long = -1
    private var dictationTextUndone = false
    private var editingDictationResult = false
    // True from the moment the edit mic starts recording until its result
    // (or a cancel) resolves — independent of editingDictationResult, which
    // only tracks which PANEL is currently shown. Stopping the edit mic
    // switches back to the main voice panel immediately (so the "thinking"/
    // polish animation plays there, not on the compact edit view), while
    // this flag keeps commitImeText() routing the eventual result to
    // finishEditWithSpokenReplacement() instead of a normal commit.
    private var awaitingEditReplacement = false
    // The exact span being replaced by the edit flow's "speak the correct
    // word" mic: either whatever the user selected in the real input field,
    // or (if nothing was selected) the whole last dictation result.
    private var editingOriginalText: String? = null
    private var editingReplacesWholeResult = false
    // True only for the clipboard swipe-left "add correction" flow: the
    // spoken result never touches currentInputConnection, it only becomes a
    // correction rule. False for the normal "edit dictation result" flow.
    private var editingForClipboardCorrection = false
    // Whether finishEditWithSpokenReplacement() should record a correction
    // rule at all. Defaults true (matches the original always-record
    // behavior); the edit panel's checkbox lets the user opt out per-edit
    // for edits that are just rewording, not an actual misrecognition worth
    // remembering — otherwise every edit silently accumulates a rule, which
    // was the reported problem (too many unwanted rules piling up). Always
    // true and hidden for the clipboard-correction flow, where recording the
    // rule is the entire point of the action.
    private var addCorrectionRuleForEdit = true
    private var undoRedoButton: TextView? = null
    private var editResultButton: TextView? = null
    // The row holding undoRedoButton/editResultButton, toggled as a whole so
    // it stays visible any time there's a result to act on (not just while
    // state == "done"), directly above the @/backspace footer row.
    private var dictationResultRow: LinearLayout? = null
    private var englishUi = false
    private val simplifiedToTraditional by lazy { Transliterator.getInstance("Hans-Hant") }
    // Packaged as an asset (not a drawable resource) so it survives the
    // gen/android scaffolding copy step the same way the stroke dictionaries do.
    private val brandLogoBitmap: android.graphics.Bitmap? by lazy {
        try {
            assets.open("openless_wordmark.png").use { android.graphics.BitmapFactory.decodeStream(it) }
        } catch (error: Exception) {
            android.util.Log.w("OpenLessIme", "failed to load brand logo asset", error)
            null
        }
    }
    private val strokeRepository by lazy { StrokeInputRepository(this) }
    private val phraseRepository by lazy { StrokePhraseRepository(this) }
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

    // Single source of truth for the encode row's blue text, reused as-is
    // (not a new similar blue) for the selected/first candidate. A property,
    // not a val, since it must track the live system theme, not whatever it
    // resolved to when the service was first created.
    private val strokeEncodeAccentColor: Int
        get() = tone(Color.rgb(120, 190, 255), Color.rgb(20, 110, 220))

    /**
     * True when the keyboard should render its dark palette — follows the
     * OpenLess app's OWN Settings > Appearance choice (mirrored into
     * `openless_ime_ui`'s "theme_mode" by OpenLessApplication's WebView
     * poll), not the raw OS dark-mode setting. Falls back to the OS setting
     * only if the app's WebView has never resolved a theme yet (fresh
     * install before Settings was opened once) — the app's own default
     * preference is itself "system", so this fallback agrees with it.
     * Re-read on every call, never cached, so a change is picked up on the
     * next panel rebuild.
     */
    private val isDarkTheme: Boolean
        get() {
            val mirrored = getSharedPreferences("openless_ime_ui", MODE_PRIVATE).getString("theme_mode", null)
            return when (mirrored) {
                "light" -> false
                "dark" -> true
                else -> (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) != Configuration.UI_MODE_NIGHT_NO
            }
        }

    /** Picks `dark` or `light` for the current system theme — the one place every themed color in this file goes through. */
    private fun tone(dark: Int, light: Int): Int = if (isDarkTheme) dark else light
    // Mirrors whatever's currently in strokeCandidates (word/stroke matches
    // or phrase associations) as plain (label, action) pairs, so the "show
    // more" overlay can replay the exact same set without duplicating the
    // stroke-match vs. association branching logic.
    private var candidateOverlayEntries: List<Pair<String, () -> Unit>> = emptyList()

    private fun ui(zh: String, en: String) = if (englishUi) en else zh

    private fun outputScript(text: String): String {
        if (!traditionalOutput) return text
        return runCatching { simplifiedToTraditional.transliterate(text) }.getOrDefault(text)
    }

    // The 5th stroke has no plain-text glyph in the encode preview — it's
    // drawn as the same shape as the "5" key's own icon (an ImageSpan), so
    // the preview and the key read as the same stroke instead of the bare
    // "乙" character.
    private fun displayStrokeCode(code: String): CharSequence {
        val builder = android.text.SpannableStringBuilder()
        code.forEach { stroke ->
            when (stroke) {
                'h' -> builder.append('一')
                's' -> builder.append('丨')
                'p' -> builder.append('丿')
                'n' -> builder.append('丶')
                'z' -> {
                    val start = builder.length
                    builder.append(' ')
                    builder.setSpan(
                        android.text.style.ImageSpan(strokeFifthGlyphDrawable(), android.text.style.ImageSpan.ALIGN_BASELINE),
                        start,
                        builder.length,
                        android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE,
                    )
                }
                '*' -> builder.append('＊')
                else -> builder.append(stroke)
            }
        }
        return builder
    }

    /** Same diagonal-then-horizontal shape as the "5" key's own icon, sized and colored to sit inline in strokePreview's text. */
    private fun strokeFifthGlyphDrawable(): android.graphics.drawable.Drawable {
        val sizePx = (16.5f * resources.displayMetrics.scaledDensity).toInt().coerceAtLeast(dp(14))
        val glyphPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = strokeEncodeAccentColor
            style = Paint.Style.STROKE
            strokeCap = Paint.Cap.ROUND
            strokeJoin = Paint.Join.ROUND
            strokeWidth = sizePx * 0.09f
        }
        return object : android.graphics.drawable.Drawable() {
            override fun draw(canvas: Canvas) {
                val unit = minOf(bounds.width(), bounds.height()) / 100f
                val cx = bounds.left + bounds.width() / 2f
                val top = bounds.top.toFloat()
                // Mirrors the "5" key's own icon shape, including its top
                // trimmed by 1/6 (bottom unchanged): 28 + (68-28)/6 ≈ 34.67.
                val path = Path().apply {
                    moveTo(cx + 1f * unit, top + 34.67f * unit)
                    lineTo(cx - 14f * unit, top + 68f * unit)
                    lineTo(cx + 16f * unit, top + 68f * unit)
                }
                canvas.drawPath(path, glyphPaint)
            }
            override fun setAlpha(alpha: Int) { glyphPaint.alpha = alpha }
            override fun setColorFilter(colorFilter: android.graphics.ColorFilter?) { glyphPaint.colorFilter = colorFilter }
            @Deprecated("Deprecated in Java", ReplaceWith("PixelFormat.TRANSLUCENT", "android.graphics.PixelFormat"))
            override fun getOpacity(): Int = android.graphics.PixelFormat.TRANSLUCENT
        }.apply { setBounds(0, 0, sizePx, sizePx) }
    }

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
            "clipboard" -> InputMode.CLIPBOARD
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

    private val clipboardManager by lazy { getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager }
    private val clipboardHistoryListener = android.content.ClipboardManager.OnPrimaryClipChangedListener {
        val text = clipboardManager.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)
            ?.coerceToText(this)?.toString()
        if (!text.isNullOrBlank()) {
            OpenLessClipboardHistory.recordCopy(this, text)
        }
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
        clipboardManager.addPrimaryClipChangedListener(clipboardHistoryListener)
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
        clipboardManager.removePrimaryClipChangedListener(clipboardHistoryListener)
        stopRuntimeService()
        strokeRepository.shutdown()
        phraseRepository.shutdown()
        super.onDestroy()
    }

    // Every panel is a fixed 300dp height by design; keep Android's own
    // fullscreen-extract heuristic from ever engaging regardless of host
    // app/orientation quirks (the actual measured-height bug turned out to
    // be unrelated — see SwipeModeContainer.onMeasure — but this is still
    // the standard, harmless precaution most custom keyboards apply).
    override fun onEvaluateFullscreenMode(): Boolean = false

    override fun onCreateInputView(): View {
        refreshLanguage()
        startRuntimeService()
        // Checked before any inputMode branch: the clipboard swipe-left
        // correction flow opens this panel while inputMode is still
        // InputMode.CLIPBOARD (never changed), so if the CLIPBOARD branch
        // below ran first, it would win and rebuild the clipboard history
        // list instead — exactly the "swipe left does nothing visible" bug
        // this ordering fixes. The original edit-result flow only ever
        // triggered from Voice mode, so this ordering issue never showed up
        // before the clipboard flow started reusing the same panel.
        if (editingDictationResult) return buildEditPanel()
        if (inputMode == InputMode.ENGLISH) return buildKeyboardView()
        if (inputMode == InputMode.STROKE) return if (strokeNumberMode) buildStrokeNumberView() else buildStrokeView()
        if (inputMode == InputMode.CLIPBOARD) return if (clipboardHistoryMode) buildClipboardHistoryView() else buildClipboardView()
        val panel = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(16), dp(8), dp(16), dp(4))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
            clipChildren = false
            clipToPadding = false
        }
        panel.addView(buildVoiceHeader(), LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            dp(38),
        ))
        status = TextView(this).apply {
            text = displayStatus(currentMessage)
            textSize = 16f
            gravity = android.view.Gravity.CENTER
            setTextColor(tone(Color.rgb(190, 190, 190), Color.rgb(110, 110, 115)))
            setPadding(0, dp(6), 0, dp(4))
        }
        panel.addView(status, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            dp(38),
        ))

        voiceButton = VoiceButton(this, isDarkTheme).apply {
            isClickable = true
            setOnClickListener { toggleDictation() }
            contentDescription = ui("OpenLess 语音听写", "OpenLess dictation")
        }
        val buttonHolder = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = android.view.Gravity.CENTER
            setBackgroundColor(Color.TRANSPARENT)
            clipChildren = false
            clipToPadding = false
        }
        buttonHolder.addView(voiceButton!!, LinearLayout.LayoutParams(dp(176), dp(72)))
        voiceLinkWarning = TextView(this).apply {
            text = ui("检测到麦克风无声音，点击重启应用", "No mic audio detected — tap to restart the app")
            textSize = 16f
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            gravity = android.view.Gravity.CENTER
            setTextColor(Color.rgb(255, 90, 90))
            setPadding(dp(12), dp(8), dp(12), 0)
            visibility = View.GONE
            isClickable = true
            setOnClickListener { restartApp() }
        }
        buttonHolder.addView(voiceLinkWarning, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        panel.addView(buttonHolder, LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            0,
            1f,
        ))

        panel.addView(buildVoiceFooter(), LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            dp(84),
        ))

        // The undo/redo/edit controls float over the panel instead of taking
        // a reserved slot in its own layout — that way showing or hiding
        // them can never nudge the mic (or anything else) out of its
        // original centered position, regardless of visibility state.
        val overlayHost = FrameLayout(this).apply {
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
        }
        overlayHost.addView(panel, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        overlayHost.addView(buildDictationResultOverlay(), FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT,
            android.view.Gravity.BOTTOM,
        ).apply {
            marginStart = dp(16)
            marginEnd = dp(16)
            // Panel's own bottom padding (4dp) + footer height (84dp) + a
            // small gap (6dp), so this lands just above @ and backspace.
            bottomMargin = dp(4) + dp(84) + dp(6)
        })
        return overlayHost
    }

    private fun buildVoiceHeader(): LinearLayout {
        val header = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
        }
        header.addView(buildBrandView(), LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(165), dp(38)))
        return header
    }

    /** Flat pill fill (no elevation) shared by every footer-row button — @, return, backspace, undo/redo, edit. */
    private fun flattenFooterButton(view: TextView, height: Int) {
        view.elevation = 0f
        view.translationZ = 0f
        // A single flat fill, not roundedButton()'s layered "keycap" look
        // (gradient face + exposed darker step) — genuinely flat, matching
        // the mic capsule's own plain drawRoundRect fill exactly.
        view.background = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = (height / 2).toFloat()
            setColor(tone(Color.rgb(54, 54, 54), Color.rgb(225, 225, 228)))
        }
    }

    /** @ / return / backspace — the plain footer row, unaffected by the undo/redo/edit overlay above it. */
    private fun buildVoiceFooter(): LinearLayout {
        val footerButtonHeight = dp(48)
        val footer = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
            clipChildren = false
            clipToPadding = false
        }
        val atButton = keyboardKey("@", 1f, action = { currentInputConnection?.commitText("@", 1) }).apply {
            textSize = 20f
            contentDescription = ui("输入 @", "Insert at sign")
            layoutParams = LinearLayout.LayoutParams(dp(84), footerButtonHeight)
            flattenFooterButton(this, footerButtonHeight)
        }
        // Chinese UI shows "换行" (line break) instead of the English word
        // "Return" — a real Chinese label reads better here than the
        // borrowed English term, so it gets its own larger, bold treatment
        // rather than reusing "Return"'s smaller size.
        val returnLabel = if (englishUi) "Return" else "换行"
        val returnButton = keyboardKey(returnLabel, 1f, action = { sendEnterKey() }).apply {
            textSize = if (englishUi) 18f else 20f
            if (!englishUi) setTypeface(typeface, android.graphics.Typeface.BOLD)
            contentDescription = ui("回车", "Return")
            layoutParams = LinearLayout.LayoutParams(dp(120), footerButtonHeight)
            flattenFooterButton(this, footerButtonHeight)
        }
        val backspaceButton = keyboardKey(
            "⌫",
            1f,
            action = {
                currentInputConnection?.deleteSurroundingText(1, 0)
                invalidateDictationResultIfTextChanged()
            },
            repeatOnLongPress = true,
            repeatAction = {
                currentInputConnection?.deleteSurroundingText(1, 0)
                invalidateDictationResultIfTextChanged()
            },
        ).apply {
            textSize = 22f
            contentDescription = ui("退格", "Backspace")
            layoutParams = LinearLayout.LayoutParams(dp(84), footerButtonHeight)
            flattenFooterButton(this, footerButtonHeight)
        }
        val returnHolder = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER
            addView(returnButton)
        }
        footer.addView(atButton)
        footer.addView(returnHolder, LinearLayout.LayoutParams(0, dp(84), 1f))
        footer.addView(backspaceButton)
        return footer
    }

    /**
     * Undo/redo + edit controls for the last dictation result, same size and
     * style as the footer's @/backspace keys. Built standalone (not part of
     * buildVoiceFooter()'s layout flow) so it can be positioned as a
     * FrameLayout overlay above the footer — showing or hiding it can never
     * shift the mic or footer since it never occupies space in their own
     * LinearLayout.
     */
    private fun buildDictationResultOverlay(): View {
        val footerButtonHeight = dp(48)
        undoRedoButton = keyboardKey("✕", 1f, action = { toggleUndoRedoDictation() }).apply {
            textSize = 20f
            contentDescription = ui("撤销本次听写结果", "Undo this dictation")
            layoutParams = LinearLayout.LayoutParams(dp(84), footerButtonHeight)
            flattenFooterButton(this, footerButtonHeight)
        }
        editResultButton = keyboardKey("✎", 1f, action = { openEditDictationResult() }).apply {
            textSize = 20f
            contentDescription = ui("编辑听写结果", "Edit dictation result")
            layoutParams = LinearLayout.LayoutParams(dp(84), footerButtonHeight)
            flattenFooterButton(this, footerButtonHeight)
        }
        val row = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        row.addView(undoRedoButton!!, LinearLayout.LayoutParams(dp(84), footerButtonHeight))
        row.addView(View(this), LinearLayout.LayoutParams(0, footerButtonHeight, 1f))
        row.addView(editResultButton!!, LinearLayout.LayoutParams(dp(84), footerButtonHeight))
        dictationResultRow = row
        updateDictationResultControls()
        return row
    }

    private fun refreshInputView() {
        setInputView(onCreateInputView())
    }

    /**
     * Same as refreshInputView(), but slides the freshly built panel in from
     * the side matching [slideDirection] (+1 = from the right, -1 = from the
     * left) — used for swipe-triggered mode switches so the transition
     * reads as a continuation of the finger's drag, not an instant swap.
     * The starting offset uses the screen width rather than the new view's
     * own (not yet measured at this point) width.
     */
    private fun refreshInputView(slideDirection: Int) {
        val newView = onCreateInputView()
        setInputView(newView)
        newView.translationX = resources.displayMetrics.widthPixels.toFloat() * slideDirection
        newView.animate()
            .translationX(0f)
            .setDuration(180L)
            .setInterpolator(android.view.animation.DecelerateInterpolator())
            .start()
    }

    /**
     * "Speak to fix a word" panel, styled after the Typeless reference: same
     * brand+toggle header as every other panel, a divider, the selected-text
     * preview (with its own cancel button), a second divider, then a wide
     * mic + status row along the bottom. Like every other input mode this
     * replaces the whole panel content rather than floating on top of it —
     * no card background, just the same panel surface split by
     * hairline dividers.
     */
    private fun buildEditPanel(): View {
        // Same SwipeModeContainer + exact 300dp constraint as every other
        // panel (voice/stroke/English) — not a plain LinearLayout — so this
        // is structurally identical to the main voice panel instead of
        // risking a taller/"fullscreen"-looking measurement.
        val root = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(16), dp(8), dp(16), dp(10))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
            clipChildren = false
            clipToPadding = false
        }

        // Same brand logo + mode toggle as the normal voice panel — not a
        // custom header with the toggle swapped for a close button — so the
        // IME's own identity/state stays visibly unchanged while editing,
        // exactly like the number panel keeps the same header as the letter
        // panel underneath it.
        root.addView(buildVoiceHeader(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)).apply {
            topMargin = dp(6)
        })

        root.addView(buildDivider(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)).apply {
            topMargin = dp(6)
        })

        val chipRow = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        val chip = TextView(this).apply {
            // Plain text, no button/pill background — this is a preview of
            // text already sitting in the real field, not a tappable control.
            text = editingOriginalText.orEmpty()
            textSize = 15f
            setTextColor(strokeEncodeAccentColor)
            maxLines = 2
            ellipsize = android.text.TextUtils.TruncateAt.END
        }
        chipRow.addView(chip, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        // Cancel-editing control lives here instead of the header, since the
        // header is now identical to every other panel's.
        chipRow.addView(flatCircleButton("✕", 34, ui("取消编辑", "Cancel editing")) {
            closeEditDictationResult()
        }, LinearLayout.LayoutParams(dp(38), dp(38)))
        root.addView(chipRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
            topMargin = dp(14)
        })

        // Hidden for the clipboard swipe-left flow, where recording a
        // correction rule is the entire point of the action, not an
        // optional side effect of fixing dictated text in place. Shown for
        // the normal edit flow so an edit that's just rewording (not an
        // actual misrecognition) doesn't silently pile up an unwanted rule.
        if (!editingForClipboardCorrection) {
            val correctionToggleRow = LinearLayout(this).apply {
                gravity = android.view.Gravity.CENTER_VERTICAL
                isClickable = true
                setOnClickListener {
                    addCorrectionRuleForEdit = !addCorrectionRuleForEdit
                    refreshInputView()
                }
            }
            correctionToggleRow.addView(
                TextView(this).apply {
                    text = if (addCorrectionRuleForEdit) "☑" else "☐"
                    textSize = 16f
                    setTextColor(if (addCorrectionRuleForEdit) strokeEncodeAccentColor else tone(Color.rgb(140, 140, 140), Color.rgb(150, 150, 154)))
                },
                LinearLayout.LayoutParams(dp(22), ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = dp(6) },
            )
            correctionToggleRow.addView(
                TextView(this).apply {
                    text = ui("同时加入纠错规则（下次自动改正）", "Also add as a correction rule")
                    textSize = 12f
                    setTextColor(tone(Color.rgb(180, 180, 180), Color.rgb(120, 120, 125)))
                },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
            root.addView(
                correctionToggleRow,
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = dp(8) },
            )
        }

        // Flexible filler mirrors the empty middle area in the reference,
        // pushing the divider + mic row down to the bottom of the panel.
        root.addView(View(this), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))

        root.addView(buildDivider(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)).apply {
            bottomMargin = dp(10)
        })

        val bottomRow = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        voiceButton = VoiceButton(this, isDarkTheme).apply {
            isClickable = true
            setOnClickListener { toggleDictation() }
            contentDescription = ui("说出正确的词", "Speak the correct word")
        }
        // Noticeably wider than a plain circular icon so the recording
        // waveform animation has real room to play.
        bottomRow.addView(voiceButton!!, LinearLayout.LayoutParams(dp(170), dp(46)))

        val textStack = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val title = TextView(this).apply {
            text = ui("说出正确的词", "Speak to edit")
            textSize = 15f
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
        }
        status = TextView(this).apply {
            text = displayStatus(currentMessage)
            textSize = 12f
            setTextColor(tone(Color.rgb(160, 160, 160), Color.rgb(120, 120, 125)))
            setPadding(0, dp(2), 0, 0)
        }
        textStack.addView(title, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        textStack.addView(status, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        bottomRow.addView(textStack, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply {
            marginStart = dp(12)
        })
        root.addView(bottomRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        return root
    }

    private fun buildDivider(): View = View(this).apply { setBackgroundColor(tone(Color.rgb(68, 68, 68), Color.rgb(215, 215, 218))) }

    private fun buildModeToggle(): View = ModeToggle(this, inputMode, isDarkTheme) { selected -> selectInputMode(selected) }

    /**
     * White-on-transparent wordmark used in every panel header, replacing
     * the old text label. Wrapped in a FrameLayout so it can be rendered at
     * a fixed half-size intrinsic box (start-aligned, vertically centered)
     * regardless of how tall the header row around it is — the existing
     * call sites all pass their own LinearLayout.LayoutParams for that row.
     */
    private fun buildBrandView(): View {
        val bitmap = brandLogoBitmap
        val wrapper = FrameLayout(this).apply {
            isClickable = true
            contentDescription = ui("打开 OpenLess 设置", "Open OpenLess settings")
            setOnClickListener { openSettings() }
            // Long-press opens a full-screen native settings window for
            // keyboard-only preferences (starting with vibration) that don't
            // need the full OpenLess app — separate from the short-tap,
            // which still opens the main app's own settings.
            setOnLongClickListener {
                openKeyboardSettings()
                true
            }
        }
        if (bitmap != null) {
            // Half the wordmark's previous rendered size — at full size it
            // crowded the header row next to the mode toggle/close button.
            val heightPx = dp(19)
            val widthPx = (heightPx.toFloat() * bitmap.width / bitmap.height).toInt()
            wrapper.addView(android.widget.ImageView(this).apply {
                setImageBitmap(bitmap)
                scaleType = android.widget.ImageView.ScaleType.FIT_XY
                // The wordmark asset is white-on-transparent; on the light
                // theme's light panel that would be invisible, so it's
                // recolored dark via a tint rather than shipping a second
                // asset.
                if (!isDarkTheme) {
                    colorFilter = android.graphics.PorterDuffColorFilter(Color.rgb(30, 30, 34), android.graphics.PorterDuff.Mode.SRC_IN)
                }
            }, FrameLayout.LayoutParams(widthPx, heightPx, android.view.Gravity.START or android.view.Gravity.CENTER_VERTICAL))
        } else {
            wrapper.addView(TextView(this).apply {
                text = "OpenLess"
                textSize = 18f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
            }, FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                android.view.Gravity.START or android.view.Gravity.CENTER_VERTICAL,
            ))
        }
        return wrapper
    }

    private fun selectInputMode(selected: InputMode, slideDirection: Int? = null) {
        if (recording || processing) cancelDictation()
        // Leaving Voice mode mid-edit would otherwise leave editingDictationResult
        // stuck true, so switching back to Voice later would wrongly reopen the
        // edit sub-view instead of the normal panel.
        editingDictationResult = false
        awaitingEditReplacement = false
        editingOriginalText = null
        inputMode = selected
        saveInputMode(selected)
        symbolMode = false
        strokeNumberMode = false
        numberSymbolMode = false
        symbolPageIndex = 0
        punctuationGroupIndex = 0
        clipboardSelectionMode = false
        clipboardSelectionAnchor = -1
        clipboardSelectionActive = -1
        clipboardHistoryMode = false
        clipboardHistoryCategory = OpenLessClipboardHistory.Category.ALL
        shiftState = ShiftState.OFF
        strokeCode = ""
        strokeQueryEpoch++
        confirmedText = ""
        phraseQueryEpoch++
        if (slideDirection != null) refreshInputView(slideDirection) else refreshInputView()
    }

    /**
     * Left/right swipe on any panel steps through the same Voice-Stroke-English
     * order as the toggle switch, clamped at both ends (no wraparound) —
     * swiping left keeps landing on Voice, right keeps landing on English.
     * The new panel slides in from the side matching the ordinal direction
     * (not necessarily the raw finger direction — see SwipeModeContainer).
     */
    private fun swipeInputMode(direction: Int) {
        val modes = InputMode.entries
        val next = modes[(inputMode.ordinal + direction).coerceIn(0, modes.lastIndex)]
        if (next != inputMode) selectInputMode(next, slideDirection = direction)
    }

    private fun buildKeyboardView(): View {
        val root = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(8), dp(8), dp(8), dp(8))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
        }
        val header = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
        }
        header.addView(buildBrandView(), LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(165), dp(38)))
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
        val returnButton = keyboardKey("Return", 1.35f, action = {
            sendEnterKey()
        })
        bottom.addView(modeButton)
        bottom.addView(spaceButton)
        bottom.addView(returnButton)
        root.addView(bottom, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        return root
    }

    private fun buildStrokeView(): View {
        // Matches the punctuation rail's own 0.16f width share below (body's
        // "0.16f/0.65f/0.19f" split) so a downward drag anywhere on the rail
        // is excluded from swipe-to-dismiss and left entirely to SwipeRail.
        val root = SwipeModeContainer(this, verticalDismissExclusionRatio = 0.16f) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(4), dp(3), dp(4), dp(3))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
        }
        val header = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        header.addView(buildBrandView(), LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(165), dp(38)))
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
        // Encode + candidate rows are fixed-height (24dp + 36dp = 60dp, same
        // total as before this pass) and never resize with content — only
        // the candidate list scrolls horizontally — so the stroke keys below
        // never move. Both rows share one rounded background (an existing
        // panel color, not a new one) so they read as a single continuous
        // strip rather than two separate cards.
        val top = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = buildEncodeAreaBackground()
        }
        val strokeRow = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        strokePreview = TextView(this).apply {
            text = ""
            textSize = 16.5f
            setTextColor(strokeEncodeAccentColor)
            gravity = android.view.Gravity.CENTER_VERTICAL
            setSingleLine(true)
            setPadding(dp(10), 0, 0, 0)
        }
        strokeRow.addView(strokePreview, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))
        // Clear-code button: a 40x30dp hit target with a small glyph, not a
        // heavy independent button — tapping it is the same clearStrokes()
        // already wired to the action rail's "清除" key.
        strokeRow.addView(
            TextView(this).apply {
                text = "✕"
                textSize = 13f
                gravity = android.view.Gravity.CENTER
                setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(130, 130, 135)))
                contentDescription = ui("清除笔画编码", "Clear stroke code")
                setOnClickListener { clearStrokes() }
            },
            LinearLayout.LayoutParams(dp(40), ViewGroup.LayoutParams.MATCH_PARENT),
        )
        top.addView(strokeRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(24)))

        val candidateRow = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        // A plain setOnTouchListener on the ScrollView never actually fires
        // here: each candidate is its own clickable keyboardKey() view, so
        // it claims ACTION_DOWN before the ScrollView's own onTouchEvent
        // ever runs. Overriding onInterceptTouchEvent instead runs at the
        // right point in the dispatch chain — before any child gets a
        // chance to claim the touch — so it reliably blocks
        // SwipeModeContainer's mode-switch gesture from stealing a drag
        // that starts on top of a candidate button.
        val candidatesScroll = object : android.widget.HorizontalScrollView(this) {
            override fun onInterceptTouchEvent(ev: android.view.MotionEvent): Boolean {
                if (ev.actionMasked == android.view.MotionEvent.ACTION_DOWN) {
                    parent?.requestDisallowInterceptTouchEvent(true)
                }
                return super.onInterceptTouchEvent(ev)
            }
        }.apply {
            isHorizontalScrollBarEnabled = false
            isFillViewport = false
            overScrollMode = View.OVER_SCROLL_NEVER
            strokeCandidates = LinearLayout(context).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
            strokeCandidates?.orientation = LinearLayout.HORIZONTAL
            addView(strokeCandidates, ViewGroup.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        candidateRow.addView(candidatesScroll, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))
        // "Show more" — opens the full candidate/association list in a
        // floating overlay instead of growing this row or the panel height.
        val expandCandidatesButton = TextView(this).apply {
            text = "▾"
            textSize = 14f
            gravity = android.view.Gravity.CENTER
            setTextColor(tone(Color.rgb(180, 180, 180), Color.rgb(130, 130, 135)))
            contentDescription = ui("展开更多候选", "Show more candidates")
        }
        expandCandidatesButton.setOnClickListener { showCandidateOverlay(expandCandidatesButton) }
        candidateRow.addView(expandCandidatesButton, LinearLayout.LayoutParams(dp(28), ViewGroup.LayoutParams.MATCH_PARENT))
        top.addView(candidateRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(36)))
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
            // Zero vertical padding so the rail's own top/bottom edges land
            // exactly on the grid/actions columns' top/bottom edges (all
            // three share the same MATCH_PARENT body height) — horizontal
            // padding is kept since it only insets key width, not row
            // position.
            setPadding(dp(2), 0, dp(2), 0)
            background = roundedButton(tone(Color.rgb(45, 45, 45), Color.rgb(230, 230, 234)), dp(4))
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
                    setBackgroundColor(tone(Color.rgb(28, 28, 28), Color.rgb(205, 205, 210)))
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
                        background = roundedButton(tone(Color.rgb(112, 78, 92), Color.rgb(232, 205, 213)), dp(5))
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
                // dp(1) on every side gives a uniform ~2dp gap both ways
                // (keys stay clearly separated) and puts as much of the
                // reclaimed margin as possible into visible key size, not
                // new padding. The action rail's vertical margin is kept at
                // the same dp(1)/dp(1) below so row top/bottom edges still
                // land exactly together across both columns.
                key.layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
                }
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
                // Vertical margin matches the grid keys' dp(1)/dp(1) exactly
                // so every row's top/bottom edge lines up across both
                // columns; horizontal margin is independent (single column).
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
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
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
        }
        val header = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        header.addView(buildBrandView().apply { setPadding(dp(8), 0, 0, 0) }, LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(165), dp(38)))
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
        // Plain "+" concatenation on a CharSequence would call toString() and
        // drop the ImageSpan the 5th stroke relies on — TextUtils.concat()
        // preserves spans across both pieces.
        strokePreview?.text = android.text.TextUtils.concat(wordSegments.joinToString(""), displayStrokeCode(strokeCode))
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
        val overlayEntries = mutableListOf<Pair<String, () -> Unit>>()
        // The very first candidate shown — whichever one that is — is
        // highlighted in the same red as the right-hand action rail, since
        // it's what a bare space/enter would commit.
        var firstCandidate = true
        if (wordSegments.isNotEmpty()) {
            val word = wordSegments.joinToString("")
            val displayWord = outputScript(word)
            val wordWidth = dp((displayWord.codePointCount(0, displayWord.length) * 22 + 16).coerceAtLeast(46))
            strokeCandidates?.addView(candidateItemView(displayWord, firstCandidate) { commitWord(word) }, LinearLayout.LayoutParams(wordWidth, ViewGroup.LayoutParams.MATCH_PARENT))
            overlayEntries.add(displayWord to { commitWord(word) })
            firstCandidate = false
        }
        strokeMatches.forEach { candidate ->
            val displayCandidate = outputScript(candidate)
            strokeCandidates?.addView(candidateItemView(displayCandidate, firstCandidate) { commitStrokeCandidate(candidate) }, LinearLayout.LayoutParams(dp(38), ViewGroup.LayoutParams.MATCH_PARENT))
            overlayEntries.add(displayCandidate to { commitStrokeCandidate(candidate) })
            firstCandidate = false
        }
        candidateOverlayEntries = overlayEntries
    }

    /**
     * Plain-text candidate item — no independent keycap background, just the
     * label, matching a stroke candidate bar rather than a row of separate
     * buttons. Height always comes from the parent row (MATCH_PARENT) so it
     * can never itself grow the fixed 36dp candidate row. The selected/first
     * candidate is marked by color+weight only (the encode row's own light
     * blue, bold) — no size, background, border or shadow change, so it
     * can't shift candidate width/spacing or row height.
     */
    private fun candidateItemView(label: String, isFirst: Boolean, action: () -> Unit): TextView {
        return keyboardKey(label, 1f, action = action).apply {
            textSize = 20f
            setSingleLine(true)
            maxLines = 1
            background = null
            elevation = 0f
            translationZ = 0f
            setPadding(dp(9), 0, dp(9), 0)
            if (isFirst) {
                setTextColor(strokeEncodeAccentColor)
                setTypeface(typeface, android.graphics.Typeface.BOLD)
            }
        }
    }

    /**
     * "Show more candidates" overlay — a PopupWindow anchored below the
     * candidate row, wrapping the current full candidate/association list
     * (whichever is showing) into a flow of rows. A PopupWindow floats over
     * the existing panel without resizing or displacing it, matching "不允许
     * 推动下方按键或改变键盘高度".
     */
    private fun showCandidateOverlay(anchor: View) {
        if (candidateOverlayEntries.isEmpty()) return
        var activePopup: android.widget.PopupWindow? = null
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(8), dp(8), dp(8), dp(8))
        }
        var currentRow: LinearLayout? = null
        var usedWidth = 0
        val maxRowWidth = resources.displayMetrics.widthPixels - dp(32)
        candidateOverlayEntries.forEach { (label, action) ->
            val itemWidth = dp((label.codePointCount(0, label.length) * 24 + 20).coerceAtLeast(52))
            if (currentRow == null || usedWidth + itemWidth > maxRowWidth) {
                currentRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
                content.addView(currentRow)
                usedWidth = 0
            }
            currentRow?.addView(
                TextView(this).apply {
                    text = label
                    textSize = 21f
                    setSingleLine(true)
                    gravity = android.view.Gravity.CENTER
                    setTextColor(tone(Color.rgb(245, 245, 245), Color.rgb(30, 30, 34)))
                    setPadding(dp(10), dp(10), dp(10), dp(10))
                    setOnClickListener {
                        action()
                        activePopup?.dismiss()
                    }
                },
                LinearLayout.LayoutParams(itemWidth, dp(46)),
            )
            usedWidth += itemWidth
        }
        val scroll = android.widget.ScrollView(this).apply {
            isVerticalScrollBarEnabled = false
            addView(content, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        val card = LinearLayout(this).apply {
            addView(scroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            background = roundedButton(tone(Color.rgb(45, 45, 45), Color.rgb(238, 238, 241)), dp(8))
        }
        val popup = android.widget.PopupWindow(
            card,
            resources.displayMetrics.widthPixels,
            dp(150),
            true,
        )
        activePopup = popup
        popup.isOutsideTouchable = true
        popup.elevation = dp(8).toFloat()
        popup.showAsDropDown(anchor, -anchor.left, dp(2))
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

    /**
     * Sends a cursor-movement key. Left/right while clipboardSelectionMode
     * is on extend the selection directly via InputConnection.setSelection()
     * instead of a synthetic Shift+Arrow KeyEvent — many host apps (e.g.
     * WeCom) use a custom edit widget that never wires KeyEvent-driven
     * selection extension up to Android's built-in ArrowKeyMovementMethod,
     * so the Shift+Arrow approach silently did nothing there. setSelection()
     * talks to the same InputConnection API the rest of this IME already
     * relies on for text manipulation, so it works regardless of how the
     * host widget itself handles key events.
     */
    private fun sendClipboardCursorKey(code: Int) {
        val connection = currentInputConnection ?: return
        val isLeft = code == android.view.KeyEvent.KEYCODE_DPAD_LEFT
        val isRight = code == android.view.KeyEvent.KEYCODE_DPAD_RIGHT
        if (clipboardSelectionMode && (isLeft || isRight)) {
            if (clipboardSelectionAnchor < 0) {
                val extracted = connection.getExtractedText(android.view.inputmethod.ExtractedTextRequest(), 0)
                val start = extracted?.selectionStart?.coerceAtLeast(0) ?: 0
                clipboardSelectionAnchor = start
                clipboardSelectionActive = start
            }
            clipboardSelectionActive = (clipboardSelectionActive + if (isRight) 1 else -1).coerceAtLeast(0)
            connection.setSelection(
                minOf(clipboardSelectionAnchor, clipboardSelectionActive),
                maxOf(clipboardSelectionAnchor, clipboardSelectionActive),
            )
            return
        }
        val now = android.os.SystemClock.uptimeMillis()
        connection.sendKeyEvent(android.view.KeyEvent(now, now, android.view.KeyEvent.ACTION_DOWN, code, 0))
        connection.sendKeyEvent(android.view.KeyEvent(now, now, android.view.KeyEvent.ACTION_UP, code, 0))
    }

    /**
     * Clipboard panel: a direction pad (arrow keys double as selection
     * extension when "选择" is toggled on) plus select-all/copy/paste and a
     * button into the clipboard history browser. Styled like the stroke
     * panel's own key grid.
     */
    private fun buildClipboardView(): View {
        val root = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(8), dp(8), dp(8), dp(8))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
        }
        root.addView(buildVoiceHeader(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)).apply {
            marginStart = dp(8)
            marginEnd = dp(8)
        })

        // Arrow keys and the Select toggle work as one unit: while selection
        // mode is on, the arrows extend the selection instead of just moving
        // the cursor, so they share the same active-highlight background as
        // the Select button itself — and lose it the moment it's toggled off.
        val gap = dp(6)
        val mediumTypeface = android.graphics.Typeface.create("sans-serif-medium", android.graphics.Typeface.NORMAL)
        val softWhite = tone(Color.rgb(224, 224, 224), Color.rgb(40, 40, 44))

        // Same keycap background as the stroke panel's own keys (plain
        // keyboardKey() default background/elevation for the normal state,
        // roundedButton() with the same rose highlight the stroke panel's
        // own "繁" toggle uses when active) — only the label/icon size and
        // grid spacing are new, not the button's edge style or palette.
        fun quickActionLabel(label: String, action: () -> Unit, textSizeSp: Float, highlighted: Boolean = false) =
            keyboardKey(label, 1f, action = action).apply {
                textSize = textSizeSp
                typeface = mediumTypeface
                setTextColor(softWhite)
                if (highlighted) background = roundedButton(tone(Color.rgb(112, 78, 92), Color.rgb(232, 205, 213)), dp(5))
                attachPressScale(this)
            }

        fun quickActionIcon(
            graphicActionCode: String,
            action: () -> Unit,
            highlighted: Boolean = false,
            rotationDegrees: Float = 0f,
            repeatOnLongPress: Boolean = false,
        ) = keyboardKey(
            "",
            1f,
            action = action,
            repeatOnLongPress = repeatOnLongPress,
            repeatAction = if (repeatOnLongPress) action else null,
            graphicActionCode = graphicActionCode,
            // Rotates only the icon drawn on the canvas, not the button
            // View itself — rotating the whole View would also distort its
            // rectangular background in a non-square cell.
            graphicRotation = rotationDegrees,
            // Unlike the stroke panel's always-dark-red action keys, these
            // icons sit on a normal or rose key that flips with the theme.
            graphicIconColor = tone(Color.WHITE, Color.rgb(30, 30, 34)),
        ).apply {
            if (highlighted) background = roundedButton(tone(Color.rgb(112, 78, 92), Color.rgb(232, 205, 213)), dp(5))
            attachPressScale(this)
        }

        fun cell(): LinearLayout.LayoutParams {
            val params = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f)
            params.marginStart = gap / 2
            params.marginEnd = gap / 2
            return params
        }

        val grid = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val row1 = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
        val selectKey = quickActionLabel(ui("选择", "Select"), {
            clipboardSelectionMode = !clipboardSelectionMode
            clipboardSelectionAnchor = -1
            clipboardSelectionActive = -1
            refreshInputView()
        }, 20f, highlighted = clipboardSelectionMode)
        val leftKey = quickActionIcon("dir-up", { sendClipboardCursorKey(android.view.KeyEvent.KEYCODE_DPAD_LEFT) }, clipboardSelectionMode, rotationDegrees = 270f)
        val rightKey = quickActionIcon("dir-up", { sendClipboardCursorKey(android.view.KeyEvent.KEYCODE_DPAD_RIGHT) }, clipboardSelectionMode, rotationDegrees = 90f)
        val upKey = quickActionIcon("dir-up", { sendClipboardCursorKey(android.view.KeyEvent.KEYCODE_DPAD_UP) }, clipboardSelectionMode, rotationDegrees = 0f)
        val downKey = quickActionIcon("dir-up", { sendClipboardCursorKey(android.view.KeyEvent.KEYCODE_DPAD_DOWN) }, clipboardSelectionMode, rotationDegrees = 180f)
        for (key in listOf(selectKey, leftKey, rightKey, upKey, downKey)) {
            row1.addView(key, cell())
        }
        grid.addView(row1, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
            bottomMargin = gap / 2
            topMargin = gap / 2
        })

        val row2 = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
        val selectAllKey = quickActionLabel(ui("全选", "Select All"), {
            currentInputConnection?.performContextMenuAction(android.R.id.selectAll)
        }, 20f)
        val copyKey = quickActionLabel(ui("复制", "Copy"), {
            currentInputConnection?.performContextMenuAction(android.R.id.copy)
        }, 20f)
        val pasteKey = quickActionLabel(ui("粘贴", "Paste"), {
            currentInputConnection?.performContextMenuAction(android.R.id.paste)
        }, 20f)
        // "粘贴板" is three characters where the others are two, so it gets a
        // slightly smaller size to avoid crowding/clipping in the same cell width.
        val clipboardKey = quickActionLabel(ui("粘贴板", "Clipboard"), {
            clipboardHistoryMode = true
            refreshInputView()
        }, 18f)
        val backspaceKey = quickActionIcon(
            "backspace-icon",
            { currentInputConnection?.deleteSurroundingText(1, 0) },
            repeatOnLongPress = true,
        )
        for (key in listOf(selectAllKey, copyKey, pasteKey, clipboardKey, backspaceKey)) {
            row2.addView(key, cell())
        }
        grid.addView(row2, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
            topMargin = gap / 2
        })

        root.addView(grid, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
            topMargin = dp(8)
        })
        return root
    }

    /**
     * Clipboard history browser: category tabs (全部/最近/文本/数字/链接) over a
     * scrollable list of past clips. Tapping an entry commits it to the field
     * and bumps it back to the front of the history, then returns to the
     * direction-pad panel.
     */
    private fun buildClipboardHistoryView(): View {
        // Rows handle their own left/right swipe (favorite / correction
        // rule); the panel-switch swipe would otherwise compete for the
        // exact same gesture.
        val root = SwipeModeContainer(this, horizontalSwipeEnabled = false) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(300))
            minimumHeight = dp(300)
            setPadding(dp(16), dp(8), dp(16), dp(8))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
            clipChildren = false
            clipToPadding = false
        }
        root.addView(buildVoiceHeader(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)))
        root.addView(buildDivider(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)).apply {
            topMargin = dp(6)
        })

        val tabsRow = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        val tabs = listOf(
            OpenLessClipboardHistory.Category.ALL to ui("全部", "All"),
            OpenLessClipboardHistory.Category.RECENT to ui("最近", "Recent"),
            OpenLessClipboardHistory.Category.TEXT to ui("文本", "Text"),
            OpenLessClipboardHistory.Category.NUMBER to ui("数字", "Number"),
            OpenLessClipboardHistory.Category.LINK to ui("链接", "Link"),
        )
        tabs.forEach { (category, label) ->
            val selected = category == clipboardHistoryCategory
            tabsRow.addView(TextView(this).apply {
                text = label
                textSize = 14f
                gravity = android.view.Gravity.CENTER
                setTextColor(if (selected) Color.rgb(153, 26, 40) else tone(Color.rgb(190, 190, 190), Color.rgb(140, 140, 145)))
                setTypeface(typeface, if (selected) android.graphics.Typeface.BOLD else android.graphics.Typeface.NORMAL)
                setOnClickListener {
                    clipboardHistoryCategory = category
                    refreshInputView()
                }
            }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))
        }
        tabsRow.addView(flatCircleButton("✕", 26, ui("关闭粘贴板", "Close clipboard")) {
            clipboardHistoryMode = false
            refreshInputView()
        }, LinearLayout.LayoutParams(dp(30), dp(30)))
        root.addView(tabsRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(34)).apply {
            topMargin = dp(8)
        })

        root.addView(buildDivider(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)).apply {
            topMargin = dp(6)
            bottomMargin = dp(6)
        })

        val listContainer = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val entries = OpenLessClipboardHistory.filter(OpenLessClipboardHistory.load(this), clipboardHistoryCategory)
        // Correction rules live in the Rust backend (CORE_BACKEND), which is
        // only registered once mobile_runtime::run()'s setup() has actually
        // executed — not guaranteed just because the IME is showing. Without
        // this, nativeCorrectionRulePatterns()/nativeAddCorrectionRule() below
        // silently no-op against an unregistered backend: the star-like icon
        // never appears and Save in the correction-rule Activity does nothing,
        // with no visible error either place.
        ensureBackendReady()
        // Fetched once per panel build, not per row: a native round trip
        // per row would be wasted work when a single JSON snapshot already
        // answers "does this text have a rule" for all of them.
        val correctionPatterns: Set<String> = try {
            val array = org.json.JSONArray(OpenLessNative.nativeCorrectionRulePatterns())
            (0 until array.length()).mapTo(mutableSetOf()) { array.getString(it) }
        } catch (error: Exception) {
            emptySet()
        }
        if (entries.isEmpty()) {
            listContainer.addView(TextView(this).apply {
                text = ui("暂无粘贴板记录", "No clipboard history yet")
                textSize = 14f
                gravity = android.view.Gravity.CENTER
                setTextColor(tone(Color.rgb(140, 140, 140), Color.rgb(140, 140, 145)))
                setPadding(0, dp(20), 0, 0)
            })
        } else {
            entries.forEachIndexed { index, entry ->
                // Two fixed-width zones behind the row, revealed left-to-right
                // as it's dragged right: favorite first, delete beyond it. A
                // third zone behind the row's end, revealed right-to-left as
                // it's dragged left, toggles a correction rule for this text.
                val entryWrapper = FrameLayout(this)
                val zoneWidth = dp(88)
                val revealRow = LinearLayout(this)
                val favoriteZone = TextView(this).apply {
                    textSize = 13f
                    setTypeface(typeface, android.graphics.Typeface.BOLD)
                    gravity = android.view.Gravity.CENTER
                    setTextColor(Color.WHITE)
                    setBackgroundColor(Color.rgb(181, 136, 32))
                    setPadding(dp(6), 0, dp(6), 0)
                }
                val deleteZone = TextView(this).apply {
                    text = ui("删除", "Delete")
                    textSize = 13f
                    setTypeface(typeface, android.graphics.Typeface.BOLD)
                    gravity = android.view.Gravity.CENTER
                    setTextColor(Color.WHITE)
                    setBackgroundColor(Color.rgb(153, 26, 40))
                    setPadding(dp(6), 0, dp(6), 0)
                }
                revealRow.addView(favoriteZone, LinearLayout.LayoutParams(zoneWidth, ViewGroup.LayoutParams.MATCH_PARENT))
                revealRow.addView(deleteZone, LinearLayout.LayoutParams(zoneWidth, ViewGroup.LayoutParams.MATCH_PARENT))
                entryWrapper.addView(revealRow, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT))
                val correctionZone = TextView(this).apply {
                    textSize = 13f
                    setTypeface(typeface, android.graphics.Typeface.BOLD)
                    gravity = android.view.Gravity.CENTER
                    setTextColor(Color.WHITE)
                    setBackgroundColor(Color.rgb(46, 108, 168))
                    setPadding(dp(6), 0, dp(6), 0)
                }
                entryWrapper.addView(
                    correctionZone,
                    FrameLayout.LayoutParams(zoneWidth, ViewGroup.LayoutParams.MATCH_PARENT, android.view.Gravity.END),
                )

                val row = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
                val star = TextView(this).apply {
                    text = if (entry.favorite) "★" else "☆"
                    textSize = 14f
                    setTextColor(
                        if (entry.favorite) Color.rgb(153, 26, 40) else tone(Color.rgb(120, 120, 120), Color.rgb(180, 180, 184)),
                    )
                }
                row.addView(star, LinearLayout.LayoutParams(dp(22), ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = dp(4) })
                row.addView(
                    TextView(this).apply {
                        text = entry.text
                        textSize = 14f
                        setTextColor(tone(Color.rgb(230, 230, 230), Color.rgb(30, 30, 34)))
                        maxLines = 2
                        ellipsize = android.text.TextUtils.TruncateAt.END
                    },
                    LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
                )
                // After the weighted text view, not before it, so this lands
                // at the row's far right edge instead of crowding the star.
                if (correctionPatterns.contains(entry.text)) {
                    row.addView(
                        TextView(this).apply {
                            text = "✎"
                            textSize = 13f
                            setTextColor(Color.rgb(90, 156, 224))
                        },
                        LinearLayout.LayoutParams(dp(16), ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginStart = dp(4) },
                    )
                }
                // Plain flat row — no keycap-style background/press effect —
                // but still opaque, so it fully hides the reveal zones until dragged.
                row.setPadding(dp(12), dp(10), dp(12), dp(10))
                row.setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
                entryWrapper.addView(row, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

                var suppressRowClick = false
                row.setOnClickListener {
                    if (suppressRowClick) {
                        suppressRowClick = false
                        return@setOnClickListener
                    }
                    currentInputConnection?.commitText(entry.text, 1)
                    OpenLessClipboardHistory.recordCopy(this@OpenLessImeService, entry.text)
                    clipboardHistoryMode = false
                    refreshInputView()
                }
                attachClipboardRowSwipe(
                    row = row,
                    zoneWidth = zoneWidth,
                    favoriteZone = favoriteZone,
                    correctionZone = correctionZone,
                    isFavorite = { entry.favorite },
                    hasCorrectionRule = { correctionPatterns.contains(entry.text) },
                    addLabel = ui("加入收藏", "Add"),
                    removeLabel = ui("取消收藏", "Remove"),
                    addCorrectionLabel = ui("加入纠错规则", "Add correction"),
                    removeCorrectionLabel = ui("移除纠错规则", "Remove correction"),
                    onSuppressClick = { suppressRowClick = true },
                    onToggleFavorite = {
                        OpenLessClipboardHistory.toggleFavorite(this, entry.text)
                        refreshInputView()
                    },
                    onDelete = {
                        OpenLessClipboardHistory.delete(this, entry.text)
                        refreshInputView()
                    },
                    onAddCorrection = { openCorrectionRuleViaVoice(entry.text) },
                    onRemoveCorrection = {
                        OpenLessNative.nativeRemoveCorrectionRule(entry.text)
                        refreshInputView()
                    },
                )
                listContainer.addView(entryWrapper, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
                if (index < entries.lastIndex) {
                    listContainer.addView(buildDivider(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)))
                }
            }
        }
        val scroll = android.widget.ScrollView(this).apply {
            isVerticalScrollBarEnabled = false
            overScrollMode = View.OVER_SCROLL_NEVER
            addView(listContainer, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        root.addView(scroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        return root
    }

    /**
     * Left/right swipe on one clipboard history row: the row itself tracks
     * the finger 1:1 via translationX, revealing fixed-width zones behind it
     * — favorite then delete to the right as it's dragged right, a single
     * correction-rule zone to the left as it's dragged left — each clamped
     * so the drag can't pull the row past its own side's zone width(s).
     * Releasing inside a zone commits that zone's action; releasing short
     * of it just springs the row back with no effect, on either side.
     *
     * Uses rawX/rawY, not the view-local x/y: once translationX starts
     * moving the row mid-gesture, view-local coordinates from the same
     * ongoing touch stream are reported relative to the view's shifting
     * transform and drift, while raw screen coordinates stay stable for the
     * whole gesture.
     *
     * onSuppressClick runs synchronously inside this same ACTION_UP handling
     * — not deferred — because the row's own click (paste + close panel)
     * gets evaluated by the View's default onTouchEvent immediately after
     * this listener returns, within the very same event. Deferring the
     * suppress flag (as a first cut of this did, bundled inside the action
     * callback) let that stale click fire first: it pasted the row's text,
     * reordered it to the front via recordCopy(), and closed back to the
     * main clipboard view — exactly the "jumps back / reorders" symptom.
     * onToggleFavorite/onDelete/onAddCorrection/onRemoveCorrection can still
     * be posted, since only *those* (which rebuild the whole panel) need to
     * avoid the panel flickering mid-gesture.
     *
     * The panel's own SwipeModeContainer has `horizontalSwipeEnabled = false`
     * for this sub-panel specifically, so there is no competing ancestor
     * gesture to race against here — recognizing the drag on ACTION_MOVE and
     * calling requestDisallowInterceptTouchEvent is enough to keep the
     * ScrollView from also treating it as a scroll attempt.
     */
    private fun attachClipboardRowSwipe(
        row: View,
        zoneWidth: Int,
        favoriteZone: TextView,
        correctionZone: TextView,
        isFavorite: () -> Boolean,
        hasCorrectionRule: () -> Boolean,
        addLabel: String,
        removeLabel: String,
        addCorrectionLabel: String,
        removeCorrectionLabel: String,
        onSuppressClick: () -> Unit,
        onToggleFavorite: () -> Unit,
        onDelete: () -> Unit,
        onAddCorrection: () -> Unit,
        onRemoveCorrection: () -> Unit,
    ) {
        val touchSlop = android.view.ViewConfiguration.get(this).scaledTouchSlop
        val maxDragRight = zoneWidth * 2f
        val maxDragLeft = zoneWidth.toFloat()
        var startRawX = 0f
        var startRawY = 0f
        var horizontalDrag = false
        row.setOnTouchListener { view, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    startRawX = event.rawX
                    startRawY = event.rawY
                    horizontalDrag = false
                    favoriteZone.text = if (isFavorite()) removeLabel else addLabel
                    correctionZone.text = if (hasCorrectionRule()) removeCorrectionLabel else addCorrectionLabel
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = event.rawX - startRawX
                    val dy = event.rawY - startRawY
                    if (!horizontalDrag && kotlin.math.abs(dx) > touchSlop && kotlin.math.abs(dx) > kotlin.math.abs(dy) * 1.5f) {
                        horizontalDrag = true
                        view.parent?.requestDisallowInterceptTouchEvent(true)
                    }
                    if (horizontalDrag) view.translationX = dx.coerceIn(-maxDragLeft, maxDragRight)
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    if (horizontalDrag) {
                        val dx = event.rawX - startRawX
                        if (dx > 0f) {
                            val clamped = dx.coerceIn(0f, maxDragRight)
                            when {
                                clamped >= maxDragRight -> {
                                    onSuppressClick()
                                    view.post { onDelete() }
                                }
                                clamped >= zoneWidth -> {
                                    onSuppressClick()
                                    view.post { onToggleFavorite() }
                                }
                            }
                        } else if (dx < 0f) {
                            val clamped = dx.coerceIn(-maxDragLeft, 0f)
                            // Only past the full zone width — the label is
                            // fully revealed by then — does releasing commit
                            // anything; short of that just springs back.
                            if (clamped <= -zoneWidth) {
                                onSuppressClick()
                                if (hasCorrectionRule()) {
                                    view.post { onRemoveCorrection() }
                                } else {
                                    view.post { onAddCorrection() }
                                }
                            }
                        }
                        view.animate().translationX(0f).setDuration(150L).start()
                    }
                    horizontalDrag = false
                }
            }
            // Never consumed: a plain tap (no horizontal drag recognized)
            // still needs to fall through to the row's own click listener.
            false
        }
    }

    /**
     * Clipboard swipe-left "add correction" action, only reached when this
     * text has no rule yet (once one exists, the same swipe instead removes
     * it directly via onRemoveCorrection, no prompt). Reuses the dictation
     * "edit result" mechanism's speak-to-replace mic (openEditDictationResult
     * / finishEditWithSpokenReplacement) instead of any separate window.
     *
     * A dedicated Activity was tried for the "type the correct wording" step
     * first. On a real device it reproduced a native crash: SIGSEGV in
     * HWUI's RenderThread (WebViewFunctorManager::destroyFunctor, abort
     * message "FORTIFY: pthread_mutex_lock called on a destroyed mutex"),
     * whenever that Activity's window appeared while
     * OpenLessBackendWarmupActivity's WebView-hosting window was mid
     * teardown in the background — confirmed via on-device tombstones on two
     * different phones, not timing-tunable away since Android decides when
     * to reclaim a backgrounded WebView's hardware layer, not app code. This
     * panel never opens a second window at all, so that whole crash class
     * does not apply, and it reuses machinery already proven here for
     * exactly this purpose (recording a correction rule from spoken text).
     *
     * editingReplacesWholeResult stays false and editingForClipboardCorrection
     * is set so finishEditWithSpokenReplacement() skips touching
     * currentInputConnection entirely — wrongText is an arbitrary clipboard
     * entry, not necessarily anything currently focused in any app.
     */
    private fun openCorrectionRuleViaVoice(wrongText: String) {
        editingOriginalText = wrongText
        editingReplacesWholeResult = false
        editingForClipboardCorrection = true
        addCorrectionRuleForEdit = true
        editingDictationResult = true
        awaitingEditReplacement = true
        refreshInputView()
        toggleDictation()
    }

    /** Commits the current character together with any segments already marked via 分词. */
    private fun commitStrokeCandidate(candidate: String) {
        // Picking anything other than the top-ranked result is a correction
        // — learn it, so this code favors `candidate` from now on. Picking
        // the top result needs no recording: it's already where it should be.
        if (OpenLessAndroidPreferences.strokeUsageEnabled(this) &&
            strokeCode.isNotEmpty() && candidate != lastStrokeCandidates.firstOrNull()
        ) {
            strokeRepository.recordPersonalPick(strokeCode, candidate)
        }
        commitWord((wordSegments + candidate).joinToString(""))
    }

    private fun commitWord(word: String) {
        if (word.isEmpty() || isSensitiveField(currentInputEditorInfo)) return
        val connection = currentInputConnection ?: return
        val contextBeforeCommit = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        if (!connection.commitText(outputScript(word), 1)) return
        if (OpenLessAndroidPreferences.strokeUsageEnabled(this)) {
            phraseRepository.recordUsage(contextBeforeCommit, word)
        }
        confirmedText = (confirmedText + word).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
    }

    private fun refreshAssociations() {
        if (!OpenLessAndroidPreferences.strokeAssociationEnabled(this)) {
            strokeCandidates?.removeAllViews()
            candidateOverlayEntries = emptyList()
            return
        }
        val context = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        val query = ++phraseQueryEpoch
        if (context.isEmpty()) return
        phraseRepository.searchAsync(context) { result ->
            if (query != phraseQueryEpoch || inputMode != InputMode.STROKE || confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT) != context) return@searchAsync
            strokeCandidates?.removeAllViews()
            val overlayEntries = mutableListOf<Pair<String, () -> Unit>>()
            result.forEachIndexed { index, candidate ->
                val matchedPrefix = candidate.matchedPrefix.ifEmpty { context }
                val displayText = outputScript(candidate.text)
                val candidateWidth = dp((displayText.codePointCount(0, displayText.length) * 22 + 16).coerceAtLeast(46))
                val commit = { commitAssociation(candidate.text, matchedPrefix) }
                strokeCandidates?.addView(candidateItemView(displayText, index == 0, commit), LinearLayout.LayoutParams(candidateWidth, ViewGroup.LayoutParams.MATCH_PARENT))
                overlayEntries.add(displayText to commit)
            }
            candidateOverlayEntries = overlayEntries
        }
    }

    private fun commitAssociation(displayText: String, matchedContext: String) {
        if (isSensitiveField(currentInputEditorInfo) || !displayText.startsWith(matchedContext)) return
        val suffix = displayText.removePrefix(matchedContext)
        val connection = currentInputConnection ?: return
        if (suffix.isNotEmpty() && !connection.commitText(outputScript(suffix), 1)) return
        if (OpenLessAndroidPreferences.strokeUsageEnabled(this)) {
            phraseRepository.recordUsage(matchedContext, displayText)
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
        // Rotates only the icon drawn inside StrokeActionView's canvas, not
        // the whole button (which would also rotate — and for a non-square
        // cell, distort — its background).
        graphicRotation: Float = 0f,
        // Only meaningful with graphicActionCode: StrokeActionView's icon
        // color. Left null for the always-dark-red action keys (white in
        // both themes, StrokeActionView's own default); the clipboard
        // panel's direction/undo icons — which sit on a normal or rose key,
        // not red — pass the theme-appropriate color explicitly.
        graphicIconColor: Int? = null,
    ): TextView {
        val keyView = when {
            microphoneIcon -> MicrophoneKeyView(this, isDarkTheme)
            strokeIconCode != null -> StrokeKeyView(this, strokeIconCode, isDarkTheme)
            graphicCode != null -> StrokeGlyphView(this, graphicCode, isDarkTheme)
            graphicActionCode != null -> if (graphicIconColor != null) {
                StrokeActionView(this, graphicActionCode, graphicRotation, graphicIconColor)
            } else {
                StrokeActionView(this, graphicActionCode, graphicRotation)
            }
            label == "←" || label == "↵" -> ActionSymbolView(this, label, isDarkTheme)
            label == "⇧" -> ShiftKeyView(this, shiftState, isDarkTheme)
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
            textSize = if (microphoneIcon) 10f else if (strokeIconCode != null) 1f else if (label == "Return") 17f else 22f
            gravity = if (microphoneIcon) android.view.Gravity.TOP or android.view.Gravity.CENTER_HORIZONTAL else android.view.Gravity.CENTER
            if (microphoneIcon) setPadding(0, dp(2), 0, 0)
            setTextColor(tone(Color.rgb(245, 245, 245), Color.rgb(30, 30, 34)))
            background = roundedButton(tone(Color.rgb(52, 52, 54), Color.rgb(255, 255, 255)), dp(5))
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
        val durationMs = preferences.getLong("key_haptic_duration_ms", 12L).coerceIn(1L, 500L)
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
        // A dictation result belongs to the editor it was typed into; carrying
        // it over to whatever field the user taps next risks an undo/edit
        // silently mangling unrelated text.
        lastDictationText = null
        dictationTextUndone = false
        editingDictationResult = false
        awaitingEditReplacement = false
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
        lastDictationText = null
        dictationTextUndone = false
        editingDictationResult = false
        awaitingEditReplacement = false
        invalidateSession("输入目标已失效")
        super.onFinishInput()
    }

    /**
     * Detects the committed dictation text getting edited or deleted some
     * other way (backspace, selecting and typing over it, etc.) — not just
     * through our own undo button — so the undo/redo/edit controls don't
     * keep pointing at text that's no longer actually there. Called both
     * from onUpdateSelection() (covers the host app's own keyboard/gestures)
     * and directly after our own footer backspace key (some hosts, e.g.
     * WeCom, don't report onUpdateSelection promptly per keystroke).
     *
     * Only clears once NONE of the dictated span remains — a single
     * backspace only shrinks it by one character, which should still leave
     * the controls up, not hide them immediately. Checks every leading
     * prefix of the dictated text (shortest first would also work, but
     * longest-first short-circuits sooner in the common case) against
     * what's actually sitting immediately before the cursor; if any prefix
     * still matches there, some of the utterance is still present.
     */
    private fun invalidateDictationResultIfTextChanged() {
        val text = lastDictationText ?: return
        if (dictationTextUndone) return
        val connection = currentInputConnection ?: return
        for (len in text.length downTo 1) {
            if (connection.getTextBeforeCursor(len, 0)?.toString() == text.substring(0, len)) return
        }
        lastDictationText = null
        updateDictationResultControls()
    }

    override fun onUpdateSelection(
        oldSelStart: Int,
        oldSelEnd: Int,
        newSelStart: Int,
        newSelEnd: Int,
        candidatesStart: Int,
        candidatesEnd: Int,
    ) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        invalidateDictationResultIfTextChanged()
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
            if (editingDictationResult) {
                // Leave the edit panel the instant recording stops — the
                // "thinking"/polish animation plays on the main voice panel
                // from here on, not the compact edit view. The eventual
                // result still finishes the pending replacement via
                // awaitingEditReplacement (see commitImeText()).
                editingDictationResult = false
                refreshInputView()
            }
            setState("thinking", "正在思考")
            runNativeAction("停止听写") { OpenLessNative.nativeStopDictationForIme() }
        } else {
            recording = true
            processing = false
            // The actual start of a new recording attempt — reset the
            // silence watch here, not in onCapsuleStateChanged's "recording"
            // branch: that branch only reset it when `recording` was still
            // false at the time, but this line already flips it true before
            // the native "recording" callback ever arrives, so that reset
            // was structurally unreachable on every normal tap-to-start.
            recordingStartedAtMs = android.os.SystemClock.elapsedRealtime()
            maxObservedLevelThisSession = 0f
            voiceLinkWarning?.visibility = View.GONE
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
                if (!recording) {
                    // Fresh recording attempt — give it a clean silence watch.
                    recordingStartedAtMs = android.os.SystemClock.elapsedRealtime()
                    maxObservedLevelThisSession = 0f
                    voiceLinkWarning?.visibility = View.GONE
                }
                recording = true
                processing = false
                maxObservedLevelThisSession = maxOf(maxObservedLevelThisSession, level)
                val elapsedMs = android.os.SystemClock.elapsedRealtime() - recordingStartedAtMs
                if (maxObservedLevelThisSession >= SILENCE_LEVEL_THRESHOLD) {
                    voiceLinkWarning?.visibility = View.GONE
                } else if (elapsedMs > SILENCE_CHECK_DELAY_MS) {
                    voiceLinkWarning?.visibility = View.VISIBLE
                }
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
        updateDictationResultControls()
    }

    private fun commitImeText(text: String) {
        if (awaitingEditReplacement) {
            finishEditWithSpokenReplacement(text)
            return
        }
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
        lastDictationText = text
        lastDictationEpoch = sessionEpoch
        dictationTextUndone = false
        setState("done", "已上屏")
    }

    private fun flatCircleButton(icon: String, sizeDp: Int, description: String, action: () -> Unit): TextView {
        return TextView(this).apply {
            text = icon
            textSize = sizeDp * 0.45f
            gravity = android.view.Gravity.CENTER
            setTextColor(tone(Color.WHITE, Color.rgb(40, 40, 44)))
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(tone(Color.rgb(54, 54, 54), Color.rgb(225, 225, 228)))
            }
            contentDescription = description
            setOnClickListener { action() }
        }
    }

    /** Shows/hides and relabels the undo↔redo toggle and edit button for the last dictation result. */
    private fun updateDictationResultControls() {
        // No longer gated on state == "done": that flips to "recording"/
        // "thinking" for the *next* utterance almost immediately, which made
        // these controls disappear right after appearing. They should stay
        // up as long as there's a previous result sitting in the buffer to
        // undo/redo/edit, regardless of what the mic is doing right now.
        val visible = inputMode == InputMode.VOICE && lastDictationText != null && !editingDictationResult
        // This row is a FrameLayout overlay above the footer (see
        // buildDictationResultOverlay()), not part of the panel's own
        // LinearLayout flow, so GONE here never shifts the mic or footer.
        dictationResultRow?.visibility = if (visible) View.VISIBLE else View.GONE
        undoRedoButton?.text = if (dictationTextUndone) "↻" else "✕"
        undoRedoButton?.contentDescription = if (dictationTextUndone) {
            ui("重做本次听写结果", "Redo this dictation")
        } else {
            ui("撤销本次听写结果", "Undo this dictation")
        }
    }

    /** Toggles between removing this dictation's committed text and restoring it. */
    private fun toggleUndoRedoDictation() {
        val text = lastDictationText ?: return
        if (sessionEpoch != lastDictationEpoch) return
        val connection = currentInputConnection ?: return
        if (!dictationTextUndone) {
            // Flip the flag before deleting: onUpdateSelection() may run
            // synchronously inside deleteSurroundingText() and would
            // otherwise see dictationTextUndone still false, mistake this
            // for the user manually erasing the text some other way, and
            // wipe lastDictationText — breaking redo.
            dictationTextUndone = true
            if (!connection.deleteSurroundingText(text.length, 0)) {
                dictationTextUndone = false
                return
            }
            updateStatus("已撤销")
        } else {
            if (!connection.commitText(text, 1)) return
            dictationTextUndone = false
            updateStatus("已上屏")
        }
    }

    /**
     * Switches the voice panel into the "edit dictation result" sub-view.
     * Corrects whatever text the user selected in the real input field
     * (native OS selection, e.g. long-press to select a wrong word); if
     * nothing is selected, falls back to replacing the whole last dictation
     * result so the button still does something useful.
     */
    private fun openEditDictationResult() {
        val connection = currentInputConnection ?: return
        val selected = connection.getSelectedText(0)?.toString()?.takeIf { it.isNotEmpty() }
        if (selected != null) {
            editingOriginalText = selected
            editingReplacesWholeResult = false
        } else {
            val whole = lastDictationText ?: return
            if (sessionEpoch != lastDictationEpoch || dictationTextUndone) return
            editingOriginalText = whole
            editingReplacesWholeResult = true
        }
        editingForClipboardCorrection = false
        addCorrectionRuleForEdit = true
        editingDictationResult = true
        awaitingEditReplacement = true
        refreshInputView()
        // Recording starts immediately when the edit panel opens — the user
        // only has to tap the mic once, to finish, matching "Tap again to
        // finish" rather than requiring a tap to start too.
        toggleDictation()
    }

    /** Backs out of the edit sub-view without applying any correction. */
    private fun closeEditDictationResult() {
        if (recording) {
            runNativeAction("取消听写") { OpenLessNative.nativeCancelDictation() }
        }
        recording = false
        processing = false
        editingDictationResult = false
        awaitingEditReplacement = false
        editingOriginalText = null
        editingForClipboardCorrection = false
        setState("done", "已上屏")
        refreshInputView()
    }

    /**
     * Applies the freshly spoken replacement for editingOriginalText.
     *
     * Normal "edit dictation result" flow: swaps it into the input field
     * (relying on InputConnection.commitText's standard "replace the active
     * selection" behavior when there is a real OS selection, or an explicit
     * delete+insert when we fell back to the whole last result). Clipboard
     * swipe-left "add correction" flow (editingForClipboardCorrection):
     * never touches currentInputConnection at all, since editingOriginalText
     * there is an arbitrary clipboard entry, not necessarily anything
     * currently focused anywhere — only recording a correction rule applies.
     *
     * Either way, recording the change as a correction rule is gated on
     * addCorrectionRuleForEdit — unconditional for the clipboard flow (that
     * is the entire point of swiping), opt-out via the edit panel's checkbox
     * for the normal flow (every edit used to silently add one, which piled
     * up rules for edits that were just rewording, not misrecognitions).
     */
    private fun finishEditWithSpokenReplacement(text: String) {
        recording = false
        processing = false
        val original = editingOriginalText
        val replacesWhole = editingReplacesWholeResult
        val forClipboardCorrection = editingForClipboardCorrection
        val shouldAddRule = addCorrectionRuleForEdit
        editingDictationResult = false
        awaitingEditReplacement = false
        editingOriginalText = null
        editingForClipboardCorrection = false
        if (text.isBlank() || original == null) {
            setState("done", "已上屏")
            refreshInputView()
            return
        }
        if (forClipboardCorrection) {
            if (shouldAddRule && text != original) {
                runNativeAction("记录纠错") {
                    OpenLessNative.nativeAddCorrectionRule(original, text)
                }
            }
            setState("done", ui("已加入纠错规则", "Correction rule added"))
            refreshInputView()
            return
        }
        val connection = currentInputConnection
        if (connection != null) {
            if (replacesWhole) connection.deleteSurroundingText(original.length, 0)
            connection.commitText(text, 1)
        }
        if (shouldAddRule && text != original) {
            runNativeAction("记录纠错") {
                OpenLessNative.nativeAddCorrectionRule(original, text)
            }
        }
        // A sub-span correction leaves the surrounding text's exact new
        // length untracked, so the coarse "undo the whole utterance" no
        // longer has a clean span to act on; a whole-result replacement is
        // still one contiguous span, so undo/redo keeps working against it.
        lastDictationText = if (replacesWhole) text else null
        dictationTextUndone = false
        setState("done", "已上屏")
        refreshInputView()
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

    /**
     * Purely visual 0.98x press-scale via StateListAnimator, which reacts to
     * the view's own pressed drawable-state — it doesn't add or replace any
     * click/touch listener, so a key's existing action/repeat wiring is
     * untouched.
     */
    private fun attachPressScale(view: View) {
        val down = android.animation.ObjectAnimator.ofPropertyValuesHolder(
            view,
            android.animation.PropertyValuesHolder.ofFloat(View.SCALE_X, 0.98f),
            android.animation.PropertyValuesHolder.ofFloat(View.SCALE_Y, 0.98f),
        ).setDuration(100)
        val up = android.animation.ObjectAnimator.ofPropertyValuesHolder(
            view,
            android.animation.PropertyValuesHolder.ofFloat(View.SCALE_X, 1f),
            android.animation.PropertyValuesHolder.ofFloat(View.SCALE_Y, 1f),
        ).setDuration(100)
        view.stateListAnimator = android.animation.StateListAnimator().apply {
            addState(intArrayOf(android.R.attr.state_pressed), down)
            addState(intArrayOf(), up)
        }
    }

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

    // Flat (no keycap relief) background for the encode+candidate strip: a
    // rounded rect a touch lighter than the panel, plus a hairline divider
    // baked in at the fixed 24dp encode/candidate boundary. Since `top`'s
    // height is always exactly dp(60), this offset never drifts.
    private fun buildEncodeAreaBackground(): android.graphics.drawable.Drawable {
        val panel = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = dp(6).toFloat()
            setColor(tone(Color.rgb(58, 58, 58), Color.rgb(228, 228, 232)))
        }
        val divider = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            setColor(tone(Color.rgb(82, 82, 82), Color.rgb(205, 205, 210)))
        }
        val dividerBottomInset = (dp(60) - dp(24) - dp(1)).coerceAtLeast(0)
        return android.graphics.drawable.LayerDrawable(arrayOf(panel, divider)).apply {
            setLayerInset(1, dp(8), dp(24), dp(8), dividerBottomInset)
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

    /**
     * Exposed so the nested SwipeModeContainer can dismiss the keyboard on a
     * downward swipe — requestHideSelf() itself is protected (inherited from
     * InputMethodService), which a same-file nested class that isn't a
     * subclass can't call directly through an instance reference.
     */
    private fun hideKeyboardPanel() {
        requestHideSelf(0)
    }

    /**
     * Relaunches the whole app and kills this process, for the voice panel's
     * "mic isn't producing audio" warning — a wedged capture/backend session
     * is more reliably cleared by a real process restart than by retrying
     * in place.
     */
    private fun restartApp() {
        runCatching {
            packageManager.getLaunchIntentForPackage(packageName)?.let { launchIntent ->
                launchIntent.addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK or android.content.Intent.FLAG_ACTIVITY_CLEAR_TASK)
                startActivity(launchIntent)
            }
        }
        android.os.Process.killProcess(android.os.Process.myPid())
    }

    private fun openKeyboardSettings() {
        requestHideSelf(0)
        android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
            startActivity(
                android.content.Intent(this, OpenLessKeyboardSettingsActivity::class.java)
                    .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        }, 180L)
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
     * Voice/English, matching the toggle switch's order. A downward drag
     * anywhere on the panel dismisses the keyboard entirely, same as any
     * other IME's own hide gesture.
     */
    private class SwipeModeContainer(
        context: android.content.Context,
        // Fraction of the container's width, measured from the left edge,
        // where a downward drag never triggers dismiss — the stroke panel's
        // punctuation rail lives there and owns vertical drags itself
        // (cycling punctuation groups). Without this, a clean vertical drag
        // starting on the rail was won by this container's own intercept
        // check on the very same touch-move event, before the rail (a
        // descendant, checked only after this ancestor) ever got a chance
        // to call requestDisallowInterceptTouchEvent — so its own "stop the
        // outer container" call never ran in time. Excluding the zone by
        // touch-down position sidesteps that dispatch-order race entirely.
        private val verticalDismissExclusionRatio: Float = 0f,
        // Off for the clipboard history sub-panel: its rows use left/right
        // swipes themselves (favorite / add correction rule), and trying to
        // exclude just their region would hit the exact same dispatch-order
        // race noted above for the punctuation rail — simplest to just not
        // compete for horizontal drags at all inside that sub-panel.
        private val horizontalSwipeEnabled: Boolean = true,
        private val onSwipe: (Int) -> Unit,
    ) : LinearLayout(context) {
        private var startX = 0f
        private var startY = 0f
        private var interceptingHorizontal = false
        private var interceptingVertical = false
        private var verticalDismissBlockedForGesture = false
        private val touchSlop = android.view.ViewConfiguration.get(context).scaledTouchSlop
        // Small enough to still claim a horizontal drag early (so a vertical
        // scroll elsewhere doesn't accidentally get treated as a mode swipe
        // partway through), but committing the actual mode switch needs a
        // much bigger, deliberate drag — the system touchSlop alone made
        // this trigger on almost any stray sideways touch.
        private val commitThreshold = (100 * resources.displayMetrics.density).toInt()
        private val dismissThreshold = (120 * resources.displayMetrics.density).toInt()

        init {
            excludeFromSystemGestures(this)
        }

        // InputMethodService.setInputView() re-wraps whatever view we return
        // in its OWN FrameLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT),
        // discarding the fixed-height LayoutParams every panel builder sets
        // on its root — confirmed via a live device: the attached root's
        // own layoutParams.height read back as WRAP_CONTENT (-2), not the
        // 300dp we set. Under a generous (near-fullscreen) WRAP_CONTENT/
        // AT_MOST measure spec from the system, any 0dp/weight=1 flexible
        // child (the spacer used by several panels) happily expands to fill
        // that huge bound instead of a real 300dp. Forcing an EXACTLY
        // 300dp height spec here — regardless of what the parent asks for —
        // makes every panel's height genuinely fixed instead of accidental.
        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
            val fixedHeight = (300 * resources.displayMetrics.density).toInt()
            val exactHeightSpec = android.view.View.MeasureSpec.makeMeasureSpec(fixedHeight, android.view.View.MeasureSpec.EXACTLY)
            super.onMeasure(widthMeasureSpec, exactHeightSpec)
        }

        override fun onInterceptTouchEvent(ev: android.view.MotionEvent): Boolean {
            when (ev.actionMasked) {
                android.view.MotionEvent.ACTION_DOWN -> {
                    startX = ev.x
                    startY = ev.y
                    interceptingHorizontal = false
                    interceptingVertical = false
                    verticalDismissBlockedForGesture = verticalDismissExclusionRatio > 0f &&
                        ev.x < width * verticalDismissExclusionRatio
                }
                android.view.MotionEvent.ACTION_MOVE -> {
                    val dx = ev.x - startX
                    val dy = ev.y - startY
                    // Require a clearly horizontal drag so this never steals a
                    // vertical gesture meant for a nested SwipeRail.
                    if (horizontalSwipeEnabled && !interceptingHorizontal && !interceptingVertical &&
                        kotlin.math.abs(dx) > touchSlop && kotlin.math.abs(dx) > kotlin.math.abs(dy) * 1.5f
                    ) {
                        interceptingHorizontal = true
                        parent?.requestDisallowInterceptTouchEvent(true)
                    }
                    // A downward drag anywhere not already claimed by a
                    // nested vertical gesture (the candidate scroll) or
                    // excluded by touch-down position (the punctuation rail)
                    // dismisses the keyboard.
                    if (!interceptingHorizontal && !interceptingVertical && !verticalDismissBlockedForGesture &&
                        dy > touchSlop && dy > kotlin.math.abs(dx) * 1.5f
                    ) {
                        interceptingVertical = true
                        parent?.requestDisallowInterceptTouchEvent(true)
                    }
                }
            }
            return interceptingHorizontal || interceptingVertical
        }

        override fun onTouchEvent(event: android.view.MotionEvent): Boolean {
            if (event.actionMasked == android.view.MotionEvent.ACTION_UP ||
                event.actionMasked == android.view.MotionEvent.ACTION_CANCEL
            ) {
                val dx = event.x - startX
                val dy = event.y - startY
                if (interceptingVertical && dy > dismissThreshold) {
                    post { (context as? OpenLessImeService)?.hideKeyboardPanel() }
                } else if (kotlin.math.abs(dx) > commitThreshold) {
                    // Swiping left (finger moves toward the start, dx < 0)
                    // steps toward Voice; swiping right steps toward
                    // English. (This is inverted from the raw dx sign — on-
                    // device testing showed dx < 0 landing on English, so
                    // the mapping below matches what actually happens rather
                    // than the "obvious" sign.)
                    // Posted for the same reason as SwipeRail: avoid
                    // rebuilding the input view synchronously mid-gesture.
                    post { onSwipe(if (dx < 0) 1 else -1) }
                }
                interceptingHorizontal = false
                interceptingVertical = false
            }
            return true
        }
    }

    private class ModeToggle(
        context: android.content.Context,
        private val selectedMode: InputMode,
        private val darkTheme: Boolean,
        private val onModeSelected: (InputMode) -> Unit,
    ) : View(context) {
        // Instance properties, not companion constants — this view is
        // rebuilt on every panel refresh, so a live theme switch just means
        // a fresh instance with the other branch's colors.
        private val trackColor = if (darkTheme) Color.rgb(28, 28, 28) else Color.rgb(222, 222, 226)
        private val dividerColor = if (darkTheme) Color.rgb(58, 58, 58) else Color.rgb(200, 200, 204)
        // Matches the outer panel's own background exactly (see
        // OpenLessImeService's panel-root tone(48,48,48 / 242,242,246)), so
        // the selected toggle segment reads as continuous with the panel
        // beneath it.
        private val panelBackgroundColor = if (darkTheme) Color.rgb(48, 48, 48) else Color.rgb(242, 242, 246)
        private val iconColor = if (darkTheme) Color.rgb(240, 240, 240) else Color.rgb(50, 50, 54)
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            strokeCap = Paint.Cap.ROUND
            strokeJoin = Paint.Join.ROUND
        }
        // Labels are drawn at a size derived from the view height, not the
        // system text size, so they never shift with font scale or UI language.
        private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = iconColor
            typeface = android.graphics.Typeface.create(android.graphics.Typeface.DEFAULT, android.graphics.Typeface.BOLD)
        }
        private val textBounds = android.graphics.Rect()
        private val chevronPath = Path()
        private val trackClipPath = Path()

        // Every proportion below is measured off the reference toggle design
        // and expressed in units of the pill height `h`.
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            val modes = InputMode.entries
            val h = height.toFloat()
            val w = width.toFloat()
            val segmentWidth = w / modes.size
            val centerY = h / 2f

            paint.style = Paint.Style.FILL
            paint.color = trackColor
            canvas.drawRoundRect(0f, 0f, w, h, h / 2f, h / 2f, paint)

            paint.color = dividerColor
            paint.strokeWidth = h * 0.012f
            for (i in 1 until modes.size) {
                val x = segmentWidth * i
                canvas.drawLine(x, h * 0.2f, x, h * 0.8f, paint)
            }

            // The selected segment is filled with the exact same color as
            // the panel body below (not a lighter floating pill), and drawn
            // with sharp corners — no rounding, no border, no shadow, no
            // gap — so it reads as one continuous surface with the panel
            // rather than a separate highlighted control sitting on top of
            // it. Clipped to the track's own rounded outline so a sharp
            // corner at the first/last segment doesn't poke past the
            // track's curve — the track's existing shape provides the only
            // rounding here, not a second one on the highlight itself.
            val selectedIndex = modes.indexOf(selectedMode).coerceAtLeast(0)
            val segmentLeft = segmentWidth * selectedIndex
            val segmentRight = segmentWidth * (selectedIndex + 1)
            trackClipPath.reset()
            trackClipPath.addRoundRect(0f, 0f, w, h, h / 2f, h / 2f, Path.Direction.CW)
            canvas.save()
            canvas.clipPath(trackClipPath)
            paint.color = panelBackgroundColor
            canvas.drawRect(segmentLeft, 0f, segmentRight, h, paint)
            canvas.restore()

            paint.color = iconColor
            drawWaveform(canvas, segmentWidth * 0.5f, centerY, h)
            drawLabel(canvas, "笔画", segmentWidth * 1.5f, centerY, h * 0.34f, extraBold = true)
            drawCursorBrackets(canvas, segmentWidth * 2.5f, centerY, h)
            drawLabel(canvas, "EN", segmentWidth * 3.5f, centerY, h * 0.28f)
        }

        /** Five bars — short, mid, tall, mid, short. */
        private fun drawWaveform(canvas: Canvas, centerX: Float, centerY: Float, h: Float) {
            paint.strokeWidth = h * 0.033f
            val spacing = h * 0.11f
            floatArrayOf(0.078f, 0.153f, 0.225f, 0.153f, 0.078f).forEachIndexed { index, half ->
                val x = centerX + (index - 2) * spacing
                canvas.drawLine(x, centerY - h * half, x, centerY + h * half, paint)
            }
        }

        /** "<I>": a bold capital-I beam (with top/bottom serifs) flanked by chevrons nearly as tall as it. */
        private fun drawCursorBrackets(canvas: Canvas, centerX: Float, centerY: Float, h: Float) {
            // Overall extent (openX/tipX/halfH below) is unchanged from the
            // plain-bar version, so the icon's total width/height stays put
            // even though the center beam is now a thicker "I" with serifs.
            val barHalfH = h * 0.16f
            val serifHalfW = h * 0.05f
            paint.strokeWidth = h * 0.045f
            canvas.drawLine(centerX, centerY - barHalfH, centerX, centerY + barHalfH, paint)
            canvas.drawLine(centerX - serifHalfW, centerY - barHalfH, centerX + serifHalfW, centerY - barHalfH, paint)
            canvas.drawLine(centerX - serifHalfW, centerY + barHalfH, centerX + serifHalfW, centerY + barHalfH, paint)

            paint.strokeWidth = h * 0.03f
            val openX = h * 0.14f
            val tipX = h * 0.289f
            val halfH = h * 0.143f
            paint.style = Paint.Style.STROKE
            chevronPath.reset()
            chevronPath.moveTo(centerX - openX, centerY - halfH)
            chevronPath.lineTo(centerX - tipX, centerY)
            chevronPath.lineTo(centerX - openX, centerY + halfH)
            chevronPath.moveTo(centerX + openX, centerY - halfH)
            chevronPath.lineTo(centerX + tipX, centerY)
            chevronPath.lineTo(centerX + openX, centerY + halfH)
            canvas.drawPath(chevronPath, paint)
            paint.style = Paint.Style.FILL
        }

        /** Bold label scaled so its actual ink height (not the font's line height) equals [inkHeight]. */
        private fun drawLabel(canvas: Canvas, label: String, centerX: Float, centerY: Float, inkHeight: Float, extraBold: Boolean = false) {
            textPaint.isFakeBoldText = extraBold
            textPaint.textSize = 100f
            textPaint.getTextBounds(label, 0, label.length, textBounds)
            textPaint.textSize = 100f * inkHeight / textBounds.height().coerceAtLeast(1)
            textPaint.getTextBounds(label, 0, label.length, textBounds)
            canvas.drawText(label, centerX - textBounds.exactCenterX(), centerY - textBounds.exactCenterY(), textPaint)
        }

        override fun onTouchEvent(event: android.view.MotionEvent): Boolean {
            if (event.action == android.view.MotionEvent.ACTION_UP) {
                val modes = InputMode.entries
                val index = (event.x / (width / modes.size.toFloat())).toInt().coerceIn(0, modes.lastIndex)
                onModeSelected(modes[index])
            }
            return true
        }

    }

    /** Central stroke keys use a canvas glyph so their proportions do not depend on a font. */
    private class StrokeGlyphView(
        context: android.content.Context,
        private val glyphCode: String,
        darkTheme: Boolean,
    ) : TextView(context) {
        private val iconColor = if (darkTheme) Color.rgb(232, 232, 232) else Color.rgb(40, 40, 44)
        private val mutedColor = if (darkTheme) Color.rgb(155, 155, 155) else Color.rgb(130, 130, 135)
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = iconColor
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
                paint.color = mutedColor
                paint.textSize = 20f * unit
                // Matches the "0" key's TextView-rendered top-gravity number, which
                // sits lower than this baseline-based canvas position implied.
                canvas.drawText(top, x, 32f * unit, paint)
                paint.color = iconColor
            }
            paint.textSize = if (text in listOf("符号", "通配", "分词", "繁")) 33f * unit else 27f * unit
            canvas.drawText(text, x, if (top.isEmpty()) 61f * unit else 76f * unit, paint)
        }
    }

    /**
     * Red actions are also custom-drawn to keep the reference glyph geometry
     * stable. Takes an explicit icon color rather than a theme flag: this
     * view is reused both on the always-dark-red action keys (icon always
     * white, in both themes) and, undecorated, on the clipboard panel's
     * normal/highlighted keys (icon needs to flip with the theme there).
     */
    private class StrokeActionView(
        context: android.content.Context,
        private val actionCode: String,
        private val iconRotation: Float = 0f,
        private val iconColor: Int = Color.WHITE,
    ) : TextView(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = iconColor
            style = Paint.Style.STROKE
            strokeWidth = 5.2f
            strokeCap = Paint.Cap.SQUARE
            strokeJoin = Paint.Join.MITER
        }
        private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = iconColor
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
                "dir-up", "backspace-icon" -> {
                    // Fixed 32dp icon canvas (not scaled to the button's own,
                    // possibly larger, size) so all four rotated direction
                    // keys and the backspace key render at one identical
                    // visual size no matter how the grid divides the row.
                    // Rotating the canvas (not the whole View) keeps the
                    // button's own rectangular background undistorted even
                    // when the cell itself isn't perfectly square.
                    val iconUnit = (32 * resources.displayMetrics.density) / 100f
                    paint.strokeWidth = 9f * iconUnit
                    paint.strokeCap = Paint.Cap.ROUND
                    paint.strokeJoin = Paint.Join.ROUND
                    canvas.save()
                    if (iconRotation != 0f) canvas.rotate(iconRotation, cx, cy)
                    if (actionCode == "dir-up") {
                        val chevron = Path().apply {
                            moveTo(cx - 22f * iconUnit, cy + 12f * iconUnit)
                            lineTo(cx, cy - 12f * iconUnit)
                            lineTo(cx + 22f * iconUnit, cy + 12f * iconUnit)
                        }
                        canvas.drawPath(chevron, paint)
                    } else {
                        // Classic backspace silhouette: a left-pointing tag
                        // outline with an "X" inside, same line weight as the
                        // direction arrows.
                        val outline = Path().apply {
                            moveTo(cx - 26f * iconUnit, cy)
                            lineTo(cx - 8f * iconUnit, cy - 22f * iconUnit)
                            lineTo(cx + 26f * iconUnit, cy - 22f * iconUnit)
                            lineTo(cx + 26f * iconUnit, cy + 22f * iconUnit)
                            lineTo(cx - 8f * iconUnit, cy + 22f * iconUnit)
                            close()
                        }
                        canvas.drawPath(outline, paint)
                        canvas.drawLine(cx - 2f * iconUnit, cy - 10f * iconUnit, cx + 16f * iconUnit, cy + 10f * iconUnit, paint)
                        canvas.drawLine(cx + 16f * iconUnit, cy - 10f * iconUnit, cx - 2f * iconUnit, cy + 10f * iconUnit, paint)
                    }
                    canvas.restore()
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
        darkTheme: Boolean,
    ) : TextView(context) {
        private val numberPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = if (darkTheme) Color.rgb(155, 155, 155) else Color.rgb(130, 130, 135)
            textAlign = Paint.Align.CENTER
            typeface = android.graphics.Typeface.create("sans-serif", android.graphics.Typeface.NORMAL)
        }
        private val strokePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = if (darkTheme) Color.rgb(232, 232, 232) else Color.rgb(40, 40, 44)
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
                    // Top trimmed by 1/6 of the original 42–79 length (bottom
                    // unchanged) at the user's request: 42 + (79-42)/6 ≈ 48.17.
                    stroke.moveTo(centerX, 48.17f * unit)
                    stroke.lineTo(centerX, 79f * unit)
                }
                "p" -> {
                    // Traced from the reference glyph: straight down for most
                    // of the stroke, hooking left only at the very end — a "J"
                    // shape, not a curve along its whole length. Top trimmed
                    // by 1/6 of the original 38–77 length (bottom/control
                    // points unchanged): 38 + (77-38)/6 = 44.5.
                    stroke.moveTo(centerX - 2f * unit, 44.5f * unit)
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
                    // Top trimmed by 1/6 of the original 42–71 diagonal length
                    // (bottom unchanged): 42 + (71-42)/6 ≈ 46.83.
                    stroke.moveTo(centerX + 1f * unit, 46.83f * unit)
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
        darkTheme: Boolean,
    ) : TextView(context) {
        private val symbolPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = if (darkTheme) Color.WHITE else Color.rgb(30, 30, 34)
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
        private val darkTheme: Boolean,
    ) : TextView(context) {
        private val mutedColor = if (darkTheme) Color.rgb(190, 190, 190) else Color.rgb(120, 120, 125)
        private val activeColor = if (darkTheme) Color.rgb(245, 245, 245) else Color.rgb(30, 30, 34)
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
                    paint.color = mutedColor
                }
                ShiftState.SHIFT_ONCE, ShiftState.CAPS_LOCK -> {
                    paint.style = Paint.Style.FILL
                    paint.color = activeColor
                }
            }
            canvas.drawPath(arrow, paint)
            if (state == ShiftState.CAPS_LOCK) {
                paint.style = Paint.Style.FILL
                paint.color = activeColor
                canvas.drawRoundRect(
                    cx - 17f * unit, cy + 22f * unit, cx + 17f * unit, cy + 28f * unit,
                    3f * unit, 3f * unit, paint,
                )
            }
        }
    }

    private class MicrophoneKeyView(context: android.content.Context, darkTheme: Boolean) : TextView(context) {
        private val microphonePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = if (darkTheme) Color.rgb(190, 190, 190) else Color.rgb(110, 110, 115)
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

    private class VoiceButton(context: android.content.Context, private val darkTheme: Boolean) : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val idlePillColor = if (darkTheme) Color.rgb(54, 54, 54) else Color.rgb(225, 225, 228)
        private val idleIconColor = if (darkTheme) Color.WHITE else Color.rgb(60, 60, 64)
        private val waveformColor = if (darkTheme) Color.rgb(222, 222, 222) else Color.rgb(70, 70, 74)
        private val processingDotColors = if (darkTheme) {
            intArrayOf(
                Color.rgb(245, 245, 245), Color.rgb(205, 205, 205),
                Color.rgb(170, 170, 170), Color.rgb(235, 235, 235),
                Color.rgb(190, 190, 190), Color.rgb(220, 220, 220),
            )
        } else {
            intArrayOf(
                Color.rgb(40, 40, 40), Color.rgb(90, 90, 90),
                Color.rgb(130, 130, 130), Color.rgb(55, 55, 55),
                Color.rgb(100, 100, 100), Color.rgb(70, 70, 70),
            )
        }
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
                paint.color = idlePillColor
                canvas.drawRoundRect(left, top, right, bottom, radius, radius, paint)
            }

            paint.color = idleIconColor
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
                    paint.color = waveformColor
                    paint.strokeWidth = dp(3).toFloat()
                    canvas.drawLine(x, centerY - halfHeight, x, centerY + halfHeight, paint)
                }
            } else if (isProcessing) {
                // Analysis state uses the same restrained monochrome palette;
                // the ring of dots keeps rotating exactly as before, and on
                // top of that the whole ring's radius now breathes — growing
                // then shrinking together as one — rather than each dot
                // sizing itself independently off its own angle.
                val colors = processingDotColors
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
        private const val SILENCE_LEVEL_THRESHOLD = 0.02f
        private const val SILENCE_CHECK_DELAY_MS = 3000L

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
