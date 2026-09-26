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
import android.graphics.Typeface
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
import android.widget.Toast

/** Minimal system IME surface. Voice transport is intentionally added in a later phase. */
class OpenLessImeService : InputMethodService(), OpenLessOverlayBridge.OverlayStateListener {
    internal enum class InputMode { VOICE, STROKE, CLIPBOARD, ENGLISH }
    private enum class ShiftState { OFF, SHIFT_ONCE, CAPS_LOCK }
    // Mirrors iOS's own ABC/123/#+= three-layer model exactly (see
    // buildKeyboardView()) instead of the old two-state symbolMode boolean,
    // which only ever had room for one merged symbol page.
    private enum class EnglishLayer { LETTERS, NUMBERS, SYMBOLS }
    // The English QWERTY panel's letter-input semantics — ENGLISH commits
    // each letter immediately (see commitEnglishChar()), PINYIN instead
    // routes letters into LitePinyinController's own encoding buffer and
    // only ever commits a chosen candidate. Long-press space toggles this
    // (see buildKeyboardView()'s spaceButton); the panel/View tree itself
    // is never rebuilt into a second layout for PINYIN — see
    // docs/pinyin-lite/phase-0-audit.md section 7.
    internal enum class LatinInputMode { ENGLISH, PINYIN }

    private var sessionEpoch = 0L
    // Guards the delayed "settle back to Tap to speak" callback — see
    // scheduleRevertToIdle().
    private var statusRevertToken = 0L
    internal var recording = false
    private var processing = false
    // Armed by the mic button's swipe-up gesture while recording is still
    // in progress (see onCreateInputView()'s voice-panel branch) — recording
    // itself keeps going, this only changes what happens once it eventually
    // stops (skip the LLM polish step). Reset whenever a fresh recording
    // starts or is cancelled, so it never leaks into a later utterance.
    private var rawModeArmed = false
        set(value) {
            field = value
            voiceButton?.rawModeActive = value
            // The RAW gesture changes the mode while the recording prompt is
            // already visible. Refresh the text color immediately in both
            // directions so yellow cannot leak into the next normal session.
            status?.setTextColor(recordingAccentColor())
        }
    // Armed by the mic button's swipe-left gesture while recording is still
    // in progress, or set directly by its idle-swipe-left start — mirrors
    // rawModeArmed exactly (see that field's own doc comment), except the
    // eventual stop calls nativeStopDictationAsQuickNote() instead of the
    // normal/raw stop: the dictation ends, gets archived as a standalone
    // note, and is never inserted into the input field (see
    // quickNoteDictation()).
    private var quickNoteArmed = false
        set(value) {
            field = value
            voiceButton?.quickNoteActive = value
            status?.setTextColor(recordingAccentColor())
        }
    // Armed by the mic button's swipe-right gesture, same shape as
    // quickNoteArmed/rawModeArmed above — except the eventual stop rides the
    // Raw stop call too (see toggleDictation()'s own rawModeArmed ||
    // cloudNoteArmed check — verbatim ASR transcript, no LLM polish), and it's
    // handleImeTextReady() that decides what happens to the resulting text:
    // submitted as JSON to the webhook configured in settings
    // (submitCloudNoteText()) instead of being committed into the input field.
    private var cloudNoteArmed = false
        set(value) {
            field = value
            voiceButton?.cloudNoteActive = value
            status?.setTextColor(recordingAccentColor())
        }

    /** Status text color while a recording prompt is showing — orange for an armed Raw stop, green for an armed Quick notes stop, red for an armed Cloud notes submit, normal otherwise. Single source of truth for rawModeArmed/quickNoteArmed/cloudNoteArmed's setters and updateStatus() alike, so the three gestures can never disagree on which one currently owns the color. */
    private fun recordingAccentColor(): Int = when {
        state == "speaking" && rawModeArmed -> LINK_COLOR_RECORDING_RAW
        state == "speaking" && quickNoteArmed -> LINK_COLOR_QUICK_NOTE
        state == "speaking" && cloudNoteArmed -> LINK_COLOR_CLOUD_NOTE
        else -> statusNormalColor
    }
    internal var inputMode = InputMode.VOICE
    private var englishLayer = EnglishLayer.LETTERS
    // Persisted across sessions the same way inputMode is (see
    // restoreLatinInputMode()/saveLatinInputMode()) — long-pressing space
    // toggles it (see buildKeyboardView()'s spaceButton). The space key's
    // own small hint label always names the mode long-press would switch
    // TO, not the current one — i.e. it reads "拼音" while latinInputMode
    // is ENGLISH, and "英文" while it's PINYIN.
    private var latinInputMode = LatinInputMode.ENGLISH
    private val litePinyinController by lazy { LitePinyinController(this) }
    // The word currently being typed on the English keyboard — appended to
    // per letter, trimmed per backspace, cleared at every word boundary
    // (space/return/punctuation/candidate tap/mode or panel switch). Never
    // touches currentInputConnection itself: every letter is still
    // committed immediately, same as before this feature: this buffer only
    // tracks what to re-query/replace for candidates.
    private val englishComposingWord = StringBuilder()
    private var englishCandidateRow: LinearLayout? = null
    // The row+divider bar as a whole (shown/hidden together when the
    // suggestions setting is toggled) and its overflow "▼" button — see
    // buildEnglishCandidateBar().
    private var englishCandidateBarContainer: View? = null
    private var englishExpandCandidatesButton: View? = null
    private var englishCandidateQueryEpoch = 0L
    // Lazy, not eager: the base dictionary/trie only gets built the first
    // time the English keyboard is actually opened (see
    // EnglishCandidateProvider.ensureLoaded()), so switching to English
    // never pays for it until that mode is first used, and other modes
    // never pay for it at all. Held as the raw Lazy (not just its .value)
    // so onDestroy() below can skip shutdown() when it was never touched.
    private val englishCandidateProviderLazy = lazy { EnglishCandidateProvider(this) }
    private val englishCandidateProvider get() = englishCandidateProviderLazy.value
    internal var traditionalOutput = false
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
    private var voiceRawHint: TextView? = null
    private var statusNormalColor = Color.GRAY
    // Single shared top-level preview bubble for every key across the
    // English and stroke keyboards (see wrapWithKeyPreviewOverlay()) — reset
    // on every panel rebuild since it lives inside that panel's own root
    // View tree, same lifecycle as voiceButton/status below. Null in panels
    // that never show a key preview (voice, clipboard, edit); callers use
    // the safe-call operator so that's a no-op rather than a crash.
    private var keyPreviewOverlay: KeyPreviewOverlay? = null
    // Last height_dp:raise_dp baked into the cached input view — compared in
    // onStartInputView so leaving keyboard settings and refocusing rebuilds
    // when the footprint prefs changed.
    private var appliedKeyboardFootprintKey: String? = null
    // LETTERS layer's swipe-up-bearing keys (buildEnglishCharKey()'s
    // wrapper -> its symbol — row1's digits, row2's @#$%&-+(), row3's
    // :;'.,!?), so a swipe-armed drag can retarget across a row the same
    // way a plain tap-drag already retargets across letters. Cleared and
    // repopulated once per LETTERS render (see addEnglishCharRow()'s own
    // row1 call, which always runs first — row2/row3 add straight into
    // that same fresh map), since a shift toggle etc. throws the whole
    // view tree away via refreshInputView() and would otherwise leave
    // stale View keys behind.
    private val englishSwipeSymbols = HashMap<View, String>()
    private var voiceButton: VoiceButton? = null
    // Silence-detection for the main voice panel: if the mic capture never
    // reports a meaningful level for a while after recording starts, the
    // audio link is probably broken upstream (muted mic, dead capture
    // session, etc.) even though the UI otherwise looks like it's recording.
    private var voiceLinkWarning: TextView? = null
    // Small always-on status dot next to the Logo (every panel's header
    // rebuilds this via buildBrandView()) — a glance-able "is the backend
    // voice link actually alive right now" signal, for the symptom where
    // tapping the mic silently did nothing (no waveform movement) because
    // the link had quietly dropped, then recovered on its own moments
    // later. Reassigned on every panel rebuild, same pattern as
    // voiceButton/status below.
    private var backendLinkIndicator: View? = null
    private var backendLinkPulseAnimator: android.animation.ObjectAnimator? = null
    // Updated only by the heartbeat check (see startBackendHeartbeat()),
    // not by the normal recording/processing/done states — those already
    // take priority in updateBackendLinkIndicator(). False means the most
    // recent heartbeat found isBackendReady() false.
    private var backendLinkHealthy = true
    private val backendHeartbeatHandler = Handler(Looper.getMainLooper())
    private val backendHeartbeatRunnable = object : Runnable {
        override fun run() {
            runBackendHeartbeatCheck()
            backendHeartbeatHandler.postDelayed(this, BACKEND_HEARTBEAT_INTERVAL_MS)
        }
    }
    private var recordingStartedAtMs = 0L
    private var maxObservedLevelThisSession = 0f
    // The exact text this dictation session committed, so the undo/redo
    // toggle can remove/restore precisely that span rather than guessing.
    private var lastDictationText: String? = null
    private var lastDictationEpoch: Long = -1
    private var dictationTextUndone = false
    private var editingDictationResult = false
    // Set right before deleteBackward() edits the field, consumed by the
    // very next invalidateDictationResultIfTextChanged() call — see
    // deleteBackward()'s comment.
    private var selfInitiatedTextChange = false
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
    // Whether finishEditWithSpokenReplacement() should write the spoken
    // replacement to the global Dictionary. Defaults false — the edit
    // panel's checkbox is an opt-in per-edit, not an opt-out, since most
    // edits are just rewording rather than an actual new/misrecognized word
    // worth remembering; ticking it is what makes the write happen at all.
    private var addToDictionaryForEdit = false
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
    // Owns everything specific to the stroke panel — encode entry, the
    // 字候选/联想候选 pipeline, and the number/symbol sub-panel — split out of
    // this class into its own file; see StrokeInputController's own doc
    // comment for the shared-vs-owned boundary.
    private val strokeController by lazy { StrokeInputController(this) }

    // Single source of truth for the encode row's blue text, reused as-is
    // (not a new similar blue) for the selected/first candidate. A property,
    // not a val, since it must track the live system theme, not whatever it
    // resolved to when the service was first created.
    internal val strokeEncodeAccentColor: Int
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
    internal fun tone(dark: Int, light: Int): Int = if (isDarkTheme) dark else light
    // Mirrors whatever's currently in strokeCandidates (word/stroke matches
    // or phrase associations) as plain (label, action) pairs, so the "show
    // more" overlay can replay the exact same set without duplicating the
    // stroke-match vs. association branching logic.
    internal var candidateOverlayEntries: List<Pair<String, () -> Unit>> = emptyList()

    internal fun ui(zh: String, en: String) = if (englishUi) en else zh

    internal fun outputScript(text: String): String {
        if (!traditionalOutput) return text
        return runCatching { simplifiedToTraditional.transliterate(text) }.getOrDefault(text)
    }

        // The 5th stroke has no plain-text glyph in the encode preview — it's
    // drawn as the same shape as the "5" key's own icon (an ImageSpan), so
    // the preview and the key read as the same stroke instead of the bare
    // "乙" character.
    internal fun displayStrokeCode(code: String): CharSequence {
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
                // The path's ink only ever spanned y=[34.67, 68] of its
                // original 100-unit design space (a bare third of the box).
                // Rescaled + repositioned here so the ink fills the box with
                // a small, even margin top and bottom — matching how a
                // plain character glyph (e.g. the "丨" stroke's own) sits
                // within its line — instead of either that ~33%-tall island
                // or (an earlier attempt) flush against the bottom edge with
                // all the blank space pushed to the top.
                val inkTop = 34.67f
                val inkBottom = 68f
                val marginFraction = 0.12f
                val boxHeight = bounds.height().toFloat()
                val unit = (boxHeight * (1f - 2f * marginFraction)) / (inkBottom - inkTop)
                val topPixels = bounds.top + boxHeight * marginFraction
                val cx = bounds.left + bounds.width() / 2f
                val path = Path().apply {
                    moveTo(cx + 1f * unit, topPixels)
                    lineTo(cx - 14f * unit, topPixels + (inkBottom - inkTop) * unit)
                    lineTo(cx + 16f * unit, topPixels + (inkBottom - inkTop) * unit)
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

    internal fun toggleScriptPreference() {
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

    internal fun saveInputMode(mode: InputMode) {
        getSharedPreferences("openless_ime_ui", MODE_PRIVATE).edit()
            .putString("input_mode", mode.name.lowercase())
            .apply()
    }

    private fun restoreLatinInputMode() {
        latinInputMode = when (getSharedPreferences("openless_ime_ui", MODE_PRIVATE).getString("latin_input_mode", "english")) {
            "pinyin" -> LatinInputMode.PINYIN
            else -> LatinInputMode.ENGLISH
        }
    }

    private fun saveLatinInputMode(mode: LatinInputMode) {
        getSharedPreferences("openless_ime_ui", MODE_PRIVATE).edit()
            .putString("latin_input_mode", mode.name.lowercase())
            .apply()
    }

    /**
     * Long-press-space entry point (see buildKeyboardView()'s spaceButton).
     * Per the lite-pinyin plan's 2026-09-25 revision: switching modes never
     * saves a half-typed pinyin encoding (matches englishComposingWord's own
     * "clear, don't carry over" behavior at every other boundary), and never
     * commits anything on its own — only a candidate tap or the English
     * path's own commitEnglishChar() ever calls commitText().
     */
    private fun toggleLatinInputMode() {
        englishComposingWord.clear()
        litePinyinController.clear()
        latinInputMode = if (latinInputMode == LatinInputMode.ENGLISH) LatinInputMode.PINYIN else LatinInputMode.ENGLISH
        saveLatinInputMode(latinInputMode)
        refreshLatinCandidateBar()
        performKeyHaptic()
        refreshInputView()
    }

    /**
     * Populates/hides the shared candidate bar for whichever of
     * English/Pinyin mode is current — used both on a cold panel build and
     * right after toggleLatinInputMode() switches modes. Pinyin's own bar
     * visibility isn't gated by englishSuggestionsEnabled() (a separate,
     * English-word-completion-specific preference); its candidates are core
     * to the mode, not an optional suggestion feature.
     */
    private fun refreshLatinCandidateBar() {
        if (latinInputMode == LatinInputMode.PINYIN) {
            litePinyinController.preloadAsync()
            englishCandidateBarContainer?.visibility = View.VISIBLE
            renderPinyinCandidates(emptyList())
        } else {
            updateEnglishCandidates()
        }
    }

    internal fun refreshLanguage() {
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
        restoreLatinInputMode()
        activeInstance = java.lang.ref.WeakReference(this)
        OpenLessOverlayBridge.imeListener = this
        OpenLessOverlayBridge.imeTextListener = ::handleImeTextReady
        startRuntimeService()
        // Load the offline stroke dictionary and the (much larger, ~220k-
        // phrase) association dictionary while the IME is idle — the latter
        // used to load lazily on whichever word's commit was the first to
        // ever need an association, which is exactly the moment a user is
        // sitting there waiting for the candidate row to update.
        strokeController.preloadAsync()
        clipboardManager.addPrimaryClipChangedListener(clipboardHistoryListener)
        backendHeartbeatHandler.post(backendHeartbeatRunnable)
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
        backendHeartbeatHandler.removeCallbacks(backendHeartbeatRunnable)
        backendLinkPulseAnimator?.cancel()
        clipboardManager.removePrimaryClipChangedListener(clipboardHistoryListener)
        stopRuntimeService()
        strokeController.shutdown()
        if (englishCandidateProviderLazy.isInitialized()) englishCandidateProviderLazy.value.shutdown()
        // Marks this as a clean end-of-session for
        // OpenLessApplication.recordUncleanShutdownIfAny() — an abrupt
        // process kill (native crash, OOM) never reaches this line, which
        // is exactly the "unclean" case that helper is trying to detect.
        getSharedPreferences("openless_runtime", MODE_PRIVATE).edit().putBoolean("session_alive", false).apply()
        super.onDestroy()
    }

    // Panel content height comes from keyboard settings (stretch); optional
    // raise adds empty space below so the whole keyboard sits higher without
    // stretching keys. Keep Android's fullscreen-extract heuristic off.
    override fun onEvaluateFullscreenMode(): Boolean = false

    /** Key-panel height in px (prefs `keyboard_height_dp`, default 300). */
    internal fun keyboardPanelHeightPx(): Int = panelHeightPx(this)

    internal fun keyboardPanelHeightDp(): Int = panelHeightDp(this)

    /** Empty space below the key panel that lifts keys (prefs `keyboard_raise_dp`). */
    internal fun keyboardRaiseHeightPx(): Int = raiseHeightPx(this)

    /**
     * Appends a bottom spacer so the key panel sits higher on screen.
     * Always wraps (raise may be 0) so later pref changes can grow the
     * spacer without rebuilding the whole IME view tree.
     */
    private fun applyKeyboardRaise(content: View): View {
        val panelPx = keyboardPanelHeightPx()
        val raisePx = keyboardRaiseHeightPx()
        return RaisedKeyboardHost(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                panelPx + raisePx,
            )
            addView(
                content,
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, panelPx),
            )
            addView(
                View(this@OpenLessImeService).apply {
                    setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
                    visibility = if (raisePx > 0) View.VISIBLE else View.GONE
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, raisePx),
            )
        }
    }

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
        val content = when {
            editingDictationResult -> buildEditPanel()
            inputMode == InputMode.ENGLISH -> wrapWithKeyPreviewOverlay(buildKeyboardView())
            inputMode == InputMode.STROKE -> wrapWithKeyPreviewOverlay(
                if (strokeController.strokeNumberMode) strokeController.buildStrokeNumberView()
                else strokeController.buildStrokeView(),
            )
            inputMode == InputMode.CLIPBOARD ->
                if (clipboardHistoryMode) buildClipboardHistoryView() else buildClipboardView()
            else -> buildVoiceInputView()
        }
        val view = applyKeyboardRaise(content)
        appliedKeyboardFootprintKey = keyboardFootprintKey()
        return view
    }

    /** Voice panel + floating dictation-result overlay (former onCreateInputView body). */
    private fun buildVoiceInputView(): View {
        val panel = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, keyboardPanelHeightPx())
            minimumHeight = keyboardPanelHeightPx()
            setPadding(dp(16), dp(8), dp(16), dp(4))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
            clipChildren = false
            clipToPadding = false
        }
        panel.addView(buildVoiceHeader(), LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            dp(38),
        ))
        statusNormalColor = tone(Color.rgb(190, 190, 190), Color.rgb(110, 110, 115))
        status = TextView(this).apply {
            text = displayStatus(currentMessage)
            textSize = 16f
            if (englishUi) typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            gravity = android.view.Gravity.CENTER
            setTextColor(statusNormalColor)
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
            // All four swipe gestures follow the same pattern: a LIVE
            // boolean re-evaluated every ACTION_MOVE (not a one-way latch),
            // a matching live visual on VoiceButton (idle capsule orange for
            // swipe-up-to-raw / green for swipe-left-to-quick-notes / red for
            // swipe-right-to-cloud-notes, waveform light red for swipe-down-to-
            // cancel — all four ease back the moment the finger drops back
            // below their own threshold), a single haptic tick on every
            // crossing in either direction, and the actual action (cancel /
            // enter raw mode / enter quick-notes mode / enter cloud-notes mode)
            // only committed on release, gated on whichever one is still
            // active at that instant. Nothing commits mid-drag — swipe down
            // used to cancel the moment the threshold was first crossed
            // during the move, but that gave no chance to back out of an
            // accidental trigger, same reasoning as the swipe-up threshold's
            // own move-to-release change earlier.
            //
            // requestDisallowInterceptTouchEvent is still claimed
            // unconditionally on DOWN (not only once a threshold is
            // crossed), matching buildEnglishCharKey()'s same fix earlier
            // this session: without it, a downward drag here is
            // indistinguishable from the enclosing SwipeModeContainer's own
            // "long downward drag dismisses the keyboard" gesture, and that
            // ancestor can steal the sequence before our own dp(30) check
            // ever sees the full distance.
            var downX = 0f
            var downY = 0f
            var swipeUpActive = false
            var swipeDownActive = false
            // Same shape as swipeUpActive (no `recording` gate on the MOVE
            // check — armable both from idle and mid-recording), just on the
            // horizontal axis — see cloudNoteArmed's own doc comment for what
            // release does in each case.
            var swipeRightActive = false
            // Same shape again, mirrored to the other horizontal direction —
            // see quickNoteArmed's own doc comment for what release does.
            var swipeLeftActive = false
            setOnTouchListener { view, event ->
                when (event.actionMasked) {
                    MotionEvent.ACTION_DOWN -> {
                        downX = event.x
                        downY = event.y
                        swipeUpActive = false
                        swipeDownActive = false
                        swipeRightActive = false
                        swipeLeftActive = false
                        view.parent?.requestDisallowInterceptTouchEvent(true)
                    }
                    MotionEvent.ACTION_MOVE -> {
                        if (recording) {
                            val active = event.y - downY >= dp(30)
                            if (active != swipeDownActive) {
                                swipeDownActive = active
                                voiceButton?.armedForCancel = active
                                performKeyHaptic()
                            }
                        }
                        val active = downY - event.y >= dp(30)
                        if (active != swipeUpActive) {
                            swipeUpActive = active
                            voiceButton?.armedForRawSwipe = active
                            // One tick every time the threshold is
                            // crossed, either direction — entering AND
                            // leaving the armed zone both get felt, not
                            // just the eventual release.
                            performKeyHaptic()
                        }
                        val activeRight = event.x - downX >= dp(30)
                        if (activeRight != swipeRightActive) {
                            swipeRightActive = activeRight
                            voiceButton?.armedForCloudNote = activeRight
                            performKeyHaptic()
                        }
                        val activeLeft = downX - event.x >= dp(30)
                        if (activeLeft != swipeLeftActive) {
                            swipeLeftActive = activeLeft
                            voiceButton?.armedForQuickNote = activeLeft
                            performKeyHaptic()
                        }
                    }
                    MotionEvent.ACTION_UP -> {
                        if (swipeDownActive) {
                            performDoubleKeyHaptic()
                            cancelDictation()
                        } else if (swipeUpActive) {
                            if (recording && !rawModeArmed) {
                                rawModeArmed = true
                                performDoubleKeyHaptic()
                                updateStatus(currentMessage)
                            } else if (!recording && !processing) {
                                toggleDictation()
                                if (recording) {
                                    rawModeArmed = true
                                    performDoubleKeyHaptic()
                                    updateStatus(currentMessage)
                                }
                            }
                        } else if (swipeRightActive) {
                            if (recording && !cloudNoteArmed) {
                                cloudNoteArmed = true
                                performDoubleKeyHaptic()
                                updateStatus(currentMessage)
                            } else if (!recording && !processing) {
                                toggleDictation()
                                if (recording) {
                                    cloudNoteArmed = true
                                    performDoubleKeyHaptic()
                                    updateStatus(currentMessage)
                                }
                            }
                        } else if (swipeLeftActive) {
                            if (recording && !quickNoteArmed) {
                                quickNoteArmed = true
                                performDoubleKeyHaptic()
                                updateStatus(currentMessage)
                            } else if (!recording && !processing) {
                                toggleDictation()
                                if (recording) {
                                    quickNoteArmed = true
                                    performDoubleKeyHaptic()
                                    updateStatus(currentMessage)
                                }
                            }
                        }
                        swipeUpActive = false
                        swipeDownActive = false
                        swipeRightActive = false
                        swipeLeftActive = false
                        voiceButton?.armedForRawSwipe = false
                        voiceButton?.armedForCancel = false
                        voiceButton?.armedForQuickNote = false
                        voiceButton?.armedForCloudNote = false
                    }
                    MotionEvent.ACTION_CANCEL -> {
                        swipeUpActive = false
                        swipeDownActive = false
                        swipeRightActive = false
                        swipeLeftActive = false
                        voiceButton?.armedForRawSwipe = false
                        voiceButton?.armedForCancel = false
                        voiceButton?.armedForQuickNote = false
                        voiceButton?.armedForCloudNote = false
                    }
                }
                false
            }
        }
        // FrameLayout, not LinearLayout: voiceButton is centered purely by
        // its own Gravity.CENTER, independent of whatever else shares this
        // holder — a LinearLayout would instead center the *whole stack* of
        // children as one block, which pushed the mic icon itself upward
        // (away from the status line above) once voiceRawHint became a
        // permanently-participating sibling instead of an occasional one.
        // voiceRawHint/voiceLinkWarning use that same CENTER anchor plus a
        // fixed topMargin, so they sit a fixed distance below the mic
        // without perturbing its own position at all.
        val buttonHolder = FrameLayout(this).apply {
            setBackgroundColor(Color.TRANSPARENT)
            clipChildren = false
            clipToPadding = false
        }
        buttonHolder.addView(voiceButton!!, FrameLayout.LayoutParams(dp(176), dp(72), android.view.Gravity.CENTER))

        // Small companion hint — deliberately muted (low size + alpha) so it
        // doesn't compete with "Tap to speak" for attention, but
        // discoverable enough that a user notices the three swipe gestures
        // exist at all. Sits just above the footer row (right above the
        // Return key, which is horizontally centered there the same way
        // this hint is) rather than directly under the mic — out of the way
        // of the mic capsule and its own waveform/pill animations, but still
        // close enough to read while a hand hovers over the mic. Tap or
        // long-press explains what Raw mode actually does via a Toast,
        // rather than building a dedicated tooltip bubble for a single
        // one-off explanation. Hidden entirely while recording/thinking in
        // an ordinary (non-Raw) dictation — see rawModeHintVisible() —
        // since at that point it's neither teaching a still-relevant
        // gesture nor confirming anything, just noise.
        val rawModeTooltip = ui("Raw模式，语音原样转写，不做AI润色整理", "Raw mode, verbatim transcription without AI polishing")
        voiceRawHint = TextView(this).apply {
            text = rawModeHintText()
            textSize = 11f
            if (englishUi) typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            gravity = android.view.Gravity.CENTER
            setTextColor(rawModeHintColor())
            visibility = if (rawModeHintVisible()) View.VISIBLE else View.GONE
            isClickable = true
            setOnClickListener { Toast.makeText(this@OpenLessImeService, rawModeTooltip, Toast.LENGTH_SHORT).show() }
            setOnLongClickListener {
                Toast.makeText(this@OpenLessImeService, rawModeTooltip, Toast.LENGTH_SHORT).show()
                true
            }
        }
        buttonHolder.addView(
            voiceRawHint,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                android.view.Gravity.BOTTOM or android.view.Gravity.CENTER_HORIZONTAL,
            ).apply {
                bottomMargin = dp(6)
            },
        )
        voiceLinkWarning = TextView(this).apply {
            text = ui("检测到麦克风无声音，点击重启应用", "No mic audio detected — tap to restart the app")
            textSize = 16f
            if (englishUi) {
                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            } else {
                setTypeface(typeface, android.graphics.Typeface.BOLD)
            }
            gravity = android.view.Gravity.CENTER
            setTextColor(Color.rgb(255, 90, 90))
            setPadding(dp(12), 0, dp(12), 0)
            visibility = View.GONE
            isClickable = true
            setOnClickListener { restartApp() }
        }
        buttonHolder.addView(
            voiceLinkWarning,
            FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, android.view.Gravity.CENTER).apply {
                // Below where the Raw hint sits, so the two never overlap on
                // the rare occasion both are visible at once (Raw-armed
                // recording with a simultaneous mic-silence warning).
                topMargin = dp(70)
            },
        )
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
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, keyboardPanelHeightPx())
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
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(240), dp(38)))
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
            setColor(tone(Color.rgb(60, 60, 60), Color.rgb(225, 225, 228)))
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
            if (englishUi) {
                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            } else {
                setTypeface(typeface, android.graphics.Typeface.BOLD)
            }
            contentDescription = ui("回车", "Return")
            layoutParams = LinearLayout.LayoutParams(dp(120), footerButtonHeight)
            flattenFooterButton(this, footerButtonHeight)
        }
        val backspaceButton = keyboardKey(
            "⌫",
            1f,
            action = { deleteBackward() },
            repeatOnLongPress = true,
            repeatAction = { deleteBackward() },
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

    internal fun refreshInputView() {
        setInputView(onCreateInputView())
    }

    /**
     * Same as refreshInputView(), but slides the freshly built panel's
     * content in from the side matching [slideDirection] (+1 = from the
     * right, -1 = from the left) — used for swipe-triggered mode switches
     * and toggle-segment taps so the transition reads as a continuation of
     * the gesture, not an instant swap. Only the content BELOW the header
     * slides; the header itself (every panel builder addView()'s it first,
     * so it's always childAt(0) — Logo, mode toggle, backend-link
     * indicator) stays anchored in place across the transition instead of
     * sliding off and back with the rest, which previously read as the
     * Logo "jumping" on every switch. The starting offset uses the screen
     * width rather than each child's own (not yet measured at this point)
     * width.
     */
    private fun refreshInputView(slideDirection: Int) {
        val newView = onCreateInputView()
        setInputView(newView)
        val offset = resources.displayMetrics.widthPixels.toFloat() * slideDirection
        // RaisedKeyboardHost wraps [panel, raiseSpacer] — slide the panel's
        // own children (skip header at 0), not the raise spacer.
        val container = when (newView) {
            is RaisedKeyboardHost -> newView.getChildAt(0) as? ViewGroup
            is ViewGroup -> newView
            else -> null
        } ?: return
        for (index in 1 until container.childCount) {
            val child = container.getChildAt(index)
            child.translationX = offset
            child.animate()
                .translationX(0f)
                .setDuration(180L)
                .setInterpolator(android.view.animation.DecelerateInterpolator())
                .start()
        }
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
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, keyboardPanelHeightPx())
            minimumHeight = keyboardPanelHeightPx()
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

        // Flexible filler mirrors the empty middle area in the reference,
        // pushing the divider + mic row down to the bottom of the panel.
        root.addView(View(this), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))

        // Placed low in the panel, just above the bottom divider — not
        // right under the text chip — and sized 1.5x (checkbox glyph and
        // label both) so it reads as a deliberate decision, not a small
        // afterthought easy to miss/mis-tap.
        val correctionToggleRow = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
            isClickable = true
            setOnClickListener {
                addToDictionaryForEdit = !addToDictionaryForEdit
                refreshInputView()
            }
        }
        correctionToggleRow.addView(
            TextView(this).apply {
                text = if (addToDictionaryForEdit) "☑" else "☐"
                textSize = 24f
                setTextColor(if (addToDictionaryForEdit) strokeEncodeAccentColor else tone(Color.rgb(140, 140, 140), Color.rgb(150, 150, 154)))
            },
            LinearLayout.LayoutParams(dp(33), ViewGroup.LayoutParams.WRAP_CONTENT).apply { marginEnd = dp(9) },
        )
        correctionToggleRow.addView(
            TextView(this).apply {
                text = ui("加入到字典中", "Add to dictionary")
                textSize = 18f
                setTextColor(tone(Color.rgb(180, 180, 180), Color.rgb(120, 120, 125)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        root.addView(
            correctionToggleRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { bottomMargin = dp(10) },
        )

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
        statusNormalColor = tone(Color.rgb(160, 160, 160), Color.rgb(120, 120, 125))
        status = TextView(this).apply {
            text = displayStatus(currentMessage)
            textSize = 12f
            setTextColor(statusNormalColor)
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

    // Tapping a toggle segment now slides the panel the same way a
    // left/right swipe does (same refreshInputView(slideDirection)
    // mechanics) — direction follows the segments' own left-to-right
    // InputMode.entries order (see ModeToggle.onDraw()), so tapping a
    // segment to the right slides in from the right, matching what a
    // swipe in that direction would already do. Tapping the
    // already-selected segment has no direction to slide from.
    internal fun buildModeToggle(): View = ModeToggle(this, inputMode, isDarkTheme) { selected ->
        val direction = when {
            selected.ordinal > inputMode.ordinal -> 1
            selected.ordinal < inputMode.ordinal -> -1
            else -> null
        }
        selectInputMode(selected, slideDirection = direction)
    }

    /**
     * White-on-transparent wordmark used in every panel header, replacing
     * the old text label, plus the backend-link status dot right after it.
     * A plain horizontal LinearLayout (not a FrameLayout) so the dot just
     * lands after the logo/text in sequence with a fixed gap, regardless of
     * which of the two logo/text branches below actually renders — the
     * existing call sites all pass their own LinearLayout.LayoutParams for
     * this whole row.
     */
    internal fun buildBrandView(): View {
        val bitmap = brandLogoBitmap
        val wrapper = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = android.view.Gravity.START or android.view.Gravity.CENTER_VERTICAL
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
            }, LinearLayout.LayoutParams(widthPx, heightPx))
        } else {
            wrapper.addView(TextView(this).apply {
                text = "OpenLess"
                textSize = 18f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        // Status dot: see backendLinkIndicator's field comment. Reassigned
        // here on every panel rebuild (buildBrandView() is shared by every
        // panel's own header), then immediately colored so it never shows a
        // stale state (e.g. still green right after a rebuild that happened
        // while actually recording) — visible everywhere per product
        // request, not just the Voice panel.
        backendLinkIndicator = View(this).apply {
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(LINK_COLOR_READY)
            }
        }
        // Continuous breathing pulse so the dot reads as "alive" rather
        // than a static badge — cancels whatever animator was running on
        // the previous panel's now-discarded indicator first, since each
        // rebuild creates a brand new View instance.
        backendLinkPulseAnimator?.cancel()
        backendLinkPulseAnimator = android.animation.ObjectAnimator.ofFloat(backendLinkIndicator, View.ALPHA, 1f, 0.35f).apply {
            duration = BACKEND_LINK_READY_PULSE_DURATION_MS
            repeatMode = android.animation.ValueAnimator.REVERSE
            repeatCount = android.animation.ValueAnimator.INFINITE
            start()
        }
        wrapper.addView(
            backendLinkIndicator,
            LinearLayout.LayoutParams(dp(12), dp(12)).apply { marginStart = dp(5) },
        )
        updateBackendLinkIndicator()
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
        englishLayer = EnglishLayer.LETTERS
        englishComposingWord.clear()
        litePinyinController.clear()
        litePinyinController.resetAssociationContext()
        strokeController.resetForModeSwitch()
        clipboardSelectionMode = false
        clipboardSelectionAnchor = -1
        clipboardSelectionActive = -1
        clipboardHistoryMode = false
        clipboardHistoryCategory = OpenLessClipboardHistory.Category.ALL
        shiftState = ShiftState.OFF
        if (slideDirection != null) refreshInputView(slideDirection) else refreshInputView()
    }

    /**
     * Left/right swipe on any panel steps through the same Voice-Stroke-English
     * order as the toggle switch, clamped at both ends (no wraparound) —
     * swiping left keeps landing on Voice, right keeps landing on English.
     * The new panel slides in from the side matching the ordinal direction
     * (not necessarily the raw finger direction — see SwipeModeContainer).
     */
    internal fun swipeInputMode(direction: Int) {
        val modes = InputMode.entries
        val next = modes[(inputMode.ordinal + direction).coerceIn(0, modes.lastIndex)]
        if (next != inputMode) selectInputMode(next, slideDirection = direction)
    }

    /**
     * English ABC/123/#+= keyboard — key relationships (rows, per-row key
     * count, relative widths, and the 123/ABC/#+= switching logic) follow
     * iOS 17's own English keyboard; colors, key surface, and every other
     * visual detail are OpenLess's existing key styling (roundedButton()/
     * tone()), matching the stroke panel. See EnglishLayer for the ABC/123/
     * #+= state and README's "英文键盘 iOS 17 布局" entry for the full
     * rationale.
     */
    /**
     * Wraps a panel root in a plain FrameLayout with a KeyPreviewOverlay as
     * its second (top-drawn) child, so every key preview bubble in that
     * panel renders above the whole keyboard — unclipped by the panel's own
     * clipChildren/candidate-row/mode-toggle bounds — without taking part in
     * its layout at all. The wrapper takes on the panel's own layoutParams
     * (every panel already sets its own fixed MATCH_PARENT x keyboardPanelHeightPx()) so
     * this changes nothing about the panel's measured size; the panel then
     * fills the wrapper exactly as it used to fill the IME window directly.
     */
    private fun wrapWithKeyPreviewOverlay(content: View): View {
        val host = FrameLayout(this).apply {
            layoutParams = content.layoutParams ?: ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, keyboardPanelHeightPx())
            clipChildren = false
            clipToPadding = false
        }
        host.addView(content, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        val overlay = KeyPreviewOverlay(this, isDarkTheme)
        keyPreviewOverlay = overlay
        host.addView(overlay, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        return host
    }

    private fun buildKeyboardView(): View {
        val root = SwipeModeContainer(this) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, keyboardPanelHeightPx())
            minimumHeight = keyboardPanelHeightPx()
            setPadding(dp(8), dp(8), dp(8), dp(8))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
        }
        val header = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
        }
        header.addView(buildBrandView(), LinearLayout.LayoutParams(0, dp(38), 1f))
        header.addView(buildModeToggle(), LinearLayout.LayoutParams(dp(240), dp(38)))
        // This panel's own root padding (8dp) is narrower than the voice panel's
        // (16dp), which it needs for its body rows. Compensate with margins so
        // the header/toggle still land at the same canonical 16dp/8dp inset as
        // every other panel — otherwise the logo and toggle visibly jump left
        // and up when switching modes.
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)).apply {
            marginStart = dp(8)
            marginEnd = dp(8)
        })

        root.addView(buildEnglishCandidateBar(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        refreshLatinCandidateBar()

        when (englishLayer) {
            EnglishLayer.LETTERS -> {
                addEnglishCharRow(
                    root,
                    listOf("q", "w", "e", "r", "t", "y", "u", "i", "o", "p"),
                    swipeSymbols = listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "0"),
                )
                // iOS insets this row by half a key on each side (9 keys
                // spanning the same width as the 10-key rows above/below);
                // 0.5f spacers on either side of 1f-weight letter keys
                // reproduce that without a second, differently-measured row.
                val row2 = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
                row2.addView(View(this), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 0.5f))
                listOf("a", "s", "d", "f", "g", "h", "j", "k", "l")
                    .zip(listOf("@", "#", "$", "%", "&", "-", "+", "(", ")"))
                    .forEach { (key, symbol) -> row2.addView(buildEnglishCharKey(key, 1f, swipeSymbol = symbol)) }
                row2.addView(View(this), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 0.5f))
                root.addView(row2, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))

                val row3 = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
                row3.addView(keyboardKey("⇧", 1.5f, action = { cycleEnglishShift() }))
                listOf("z", "x", "c", "v", "b", "n", "m")
                    .zip(listOf(":", ";", "'", ".", ",", "!", "?"))
                    .forEach { (key, symbol) -> row3.addView(buildEnglishCharKey(key, 1f, swipeSymbol = symbol)) }
                row3.addView(
                    keyboardKey("⌫", 1.5f, action = { englishDeleteBackward() }, repeatOnLongPress = true, repeatAction = { englishDeleteBackward() }).apply {
                        // Same cherry red as the stroke panel's own
                        // right-side action rail, matching the Return key
                        // right next to it.
                        background = roundedButton(Color.rgb(153, 26, 40), dp(5))
                        setTextColor(Color.WHITE)
                    },
                )
                root.addView(row3, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            }
            EnglishLayer.NUMBERS -> {
                addEnglishCharRow(root, listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "0"))
                addEnglishCharRow(root, listOf("-", "/", ":", ";", "(", ")", "$", "&", "@", "\""))
                addEnglishSymbolActionRow(root, listOf(".", ",", "?", "!", "'"))
            }
            EnglishLayer.SYMBOLS -> {
                addEnglishCharRow(root, listOf("[", "]", "{", "}", "#", "%", "^", "*", "+", "="))
                addEnglishCharRow(root, listOf("_", "\\", "|", "~", "<", ">", "€", "£", "¥", "•"))
                addEnglishSymbolActionRow(root, listOf(".", ",", "?", "!", "'"))
            }
        }

        val bottom = LinearLayout(this).apply {
            gravity = android.view.Gravity.CENTER_VERTICAL
        }
        // Same sans-serif-medium as the letter grid (buildEnglishCharKey()'s
        // letterView), applied after the fact rather than as a keyboardKey()
        // parameter — keyboardKey() is shared by every panel's keys, and
        // only these two English bottom-row labels are meant to match the
        // letters' new weight for now. Size/color/position left exactly as
        // keyboardKey() already set them.
        val modeButton = keyboardKey(if (englishLayer == EnglishLayer.LETTERS) "123" else "ABC", 1.3f, action = {
            handleEnglishBottomModeToggle()
        }).apply { typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL) }
        // "Space" stays keyboardKey()'s own plain centered label (own
        // View, own vertical centering, untouched by the hint) — per
        // real-device feedback, side-by-side (hint right-aligned next to
        // "Space", both 12sp) reads much better than the two stacked-line
        // approaches tried first (a single two-line SpannableString label,
        // and before that a padding-pushed overlay), both of which fought
        // this row's actual (tighter than assumed) real height.
        val spaceButton = keyboardKey("Space", 5f, action = {
            // Space never commits a pinyin candidate (see
            // toggleLatinInputMode()'s own doc comment) — a short press
            // always just inserts a literal space, in both modes, and
            // abandons whatever was mid-composition. This is deliberate:
            // it's a software keyboard, not a physical one where "space
            // selects the first candidate" is a decades-old muscle-memory
            // convention worth preserving.
            finalizeEnglishComposingWord()
            litePinyinController.clear()
            currentInputConnection?.commitText(" ", 1)
        }, longPressAction = { toggleLatinInputMode() }).apply {
            // Same size as Return (17sp — keyboardKey()'s own special case
            // for that label), per real-device feedback.
            textSize = 17f
        }
        // Own View, overlaid on top of spaceButton (not inside its own
        // text) — left-aligned, vertically centered, non-interactive so
        // touches still reach the key beneath it. Shows BOTH labels
        // stacked (拼音 always on top, 英文 always on the bottom — fixed
        // position, per user request) rather than just naming the target
        // mode — whichever is currently active is cherry red, the other
        // gray, so long-press's effect is legible at a glance either way.
        val spaceHint = TextView(this).apply {
            val pinyinLabel = ui("拼音", "PINYIN")
            val englishLabel = ui("英文", "ENGLISH")
            val hintText = "$pinyinLabel\n$englishLabel"
            val englishStart = hintText.length - englishLabel.length
            val activeColor = if (isDarkTheme) Color.rgb(190, 45, 60) else Color.rgb(153, 26, 40)
            val inactiveColor = tone(Color.rgb(150, 150, 150), Color.rgb(140, 140, 145))
            text = android.text.SpannableString(hintText).apply {
                setSpan(
                    android.text.style.ForegroundColorSpan(if (latinInputMode == LatinInputMode.PINYIN) activeColor else inactiveColor),
                    0, pinyinLabel.length, android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE,
                )
                setSpan(
                    android.text.style.ForegroundColorSpan(if (latinInputMode == LatinInputMode.ENGLISH) activeColor else inactiveColor),
                    englishStart, hintText.length, android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE,
                )
            }
            textSize = 12f
            gravity = android.view.Gravity.CENTER
            setTypeface(typeface, android.graphics.Typeface.BOLD)
            isClickable = false
            // The actual bug behind every earlier "text is just missing"
            // attempt at a FrameLayout overlay on a keyboardKey(): that key
            // has its own non-zero elevation+translationZ (dp(5)+dp(1), see
            // keyboardKey()'s own body) for its raised-surface look, and on
            // API 21+ ViewGroup draws children in Z order — not insertion
            // order — whenever their Z values differ. spaceButton's Z was
            // silently winning and painting straight over this View despite
            // being added to the FrameLayout first. A higher Z here forces
            // this View to actually draw on top.
            translationZ = dp(10).toFloat()
        }
        val spaceWrapper = FrameLayout(this).apply {
            addView(spaceButton, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            addView(
                spaceHint,
                FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, android.view.Gravity.CENTER_VERTICAL or android.view.Gravity.START).apply {
                    marginStart = dp(14)
                },
            )
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 5f)
        }
        val returnButton = keyboardKey("Return", 1.7f, action = {
            // Plan 3.3: "Enter 有候选时提交第一候选，无编码时使用原有回车行为"
            // — reuses candidateOverlayEntries (already the current row's
            // (label, action) pairs, pinyin or English, set by whichever
            // render*Candidates() last ran) rather than a second field
            // duplicating the same list.
            val firstPinyinCandidate = candidateOverlayEntries.firstOrNull()
            if (latinInputMode == LatinInputMode.PINYIN && firstPinyinCandidate != null) {
                firstPinyinCandidate.second.invoke()
            } else {
                finalizeEnglishComposingWord()
                sendEnterKey()
            }
        }).apply {
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            // Same cherry red as the stroke panel's own right-side action
            // rail (Color.rgb(153, 26, 40)), per product request; fixed
            // white text regardless of theme since keyboardKey()'s own
            // default text color reads poorly against red in light theme.
            background = roundedButton(Color.rgb(153, 26, 40), dp(5))
            setTextColor(Color.WHITE)
        }
        bottom.addView(modeButton)
        bottom.addView(spaceWrapper)
        bottom.addView(returnButton)
        root.addView(bottom, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        return root
    }

    /**
     * A plain character row (digits/symbols, no shift-sensitivity) built
     * from buildEnglishCharKey() — key preview, no drag-to-adjacent-key
     * distinction from a letter row since neither cares about shift.
     *
     * @param swipeSymbols Same size as [keys] when given (the QWERTY top
     *   row's own q..p -> 1..0 mapping) — forwarded 1:1 as each key's own
     *   swipe-up symbol. Left null for every other row (NUMBERS/SYMBOLS
     *   layers are already digits/symbols themselves; nothing to swipe up
     *   to). Clears englishSwipeSymbols first — this is always the first
     *   row built for a fresh LETTERS render (see buildKeyboardView()), so
     *   row2/row3's own direct buildEnglishCharKey() calls right after can
     *   add straight into the same now-fresh map without re-clearing it.
     */
    private fun addEnglishCharRow(parent: LinearLayout, keys: List<String>, swipeSymbols: List<String>? = null) {
        if (swipeSymbols != null) englishSwipeSymbols.clear()
        val row = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
        keys.forEachIndexed { index, key ->
            row.addView(buildEnglishCharKey(key, 1f, swipeSymbol = swipeSymbols?.getOrNull(index)))
        }
        parent.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    /**
     * 123 page's row 3 (`#+=  .  ,  ?  !  '  ⌫`) and #+= page's row 3
     * (`123  .  ,  ?  !  '  ⌫`) — same shape, only the leading toggle
     * key's label/action differs, so both layers share this one builder.
     */
    private fun addEnglishSymbolActionRow(parent: LinearLayout, middleKeys: List<String>) {
        val row = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER }
        val toggleLabel = if (englishLayer == EnglishLayer.NUMBERS) "#+=" else "123"
        row.addView(keyboardKey(toggleLabel, 1.5f, action = { handleEnglishRow3ModeToggle() }))
        middleKeys.forEach { row.addView(buildEnglishCharKey(it, 1f)) }
        row.addView(
            keyboardKey("⌫", 1.5f, action = { englishDeleteBackward() }, repeatOnLongPress = true, repeatAction = { englishDeleteBackward() }).apply {
                background = roundedButton(Color.rgb(153, 26, 40), dp(5))
                setTextColor(Color.WHITE)
            },
        )
        parent.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    /** Bottom-row toggle: always jumps straight to/from Letters, regardless of which of Numbers/Symbols was showing (matches iOS's own "ABC" key). */
    private fun handleEnglishBottomModeToggle() {
        englishLayer = if (englishLayer == EnglishLayer.LETTERS) EnglishLayer.NUMBERS else EnglishLayer.LETTERS
        shiftState = ShiftState.OFF
        finalizeEnglishComposingWord()
        refreshInputView()
    }

    /** Row-3 toggle: only ever switches between Numbers and Symbols, never touches Letters (matches iOS's own "#+="/"123" key). */
    private fun handleEnglishRow3ModeToggle() {
        englishLayer = if (englishLayer == EnglishLayer.NUMBERS) EnglishLayer.SYMBOLS else EnglishLayer.NUMBERS
        refreshInputView()
    }

    private fun cycleEnglishShift() {
        // Cycles lowercase -> capitalize-next -> caps-lock -> lowercase.
        shiftState = when (shiftState) {
            ShiftState.OFF -> ShiftState.SHIFT_ONCE
            ShiftState.SHIFT_ONCE -> ShiftState.CAPS_LOCK
            ShiftState.CAPS_LOCK -> ShiftState.OFF
        }
        refreshInputView()
    }

    /** Backspace on the English keyboard: same field edit as everywhere else, plus keeping englishComposingWord (and therefore the candidate bar) in sync. In Pinyin mode, deletes from the (never-committed) encoding instead — falls back to the normal field edit only once the encoding is already empty (plan 3.3: "编码为空时 Backspace 恢复现有普通删除"). */
    private fun englishDeleteBackward() {
        if (latinInputMode == LatinInputMode.PINYIN && litePinyinController.backspace { renderPinyinCandidates(it) }) {
            return
        }
        deleteBackward()
        if (englishComposingWord.isNotEmpty()) {
            englishComposingWord.deleteCharAt(englishComposingWord.length - 1)
        }
        updateEnglishCandidates()
    }

    /**
     * The selected/first candidate is marked by color+weight only (the same
     * red as the right-hand action rail's ←/↵/清除/123 keys, bold) — no
     * size, background, border or shadow change, so it can't shift
     * candidate width/spacing or row height. A real else branch (not just
     * skipping the isFirst==false case) matters once populateCandidateRow()
     * can reuse a view that used to be first-candidate for one that isn't.
     */
    internal fun styleCandidateFirstState(view: TextView, isFirst: Boolean) {
        if (isFirst) {
            // Same red as the action rail's own background
            // (Color.rgb(153, 26, 40)), brightened a touch for dark
            // theme only — same hue, just a bit lighter so it reads
            // more clearly against a dark panel; light theme keeps the
            // exact action-rail red.
            view.setTextColor(if (isDarkTheme) Color.rgb(190, 45, 60) else Color.rgb(153, 26, 40))
            view.setTypeface(view.typeface, android.graphics.Typeface.BOLD)
        } else {
            view.setTextColor(tone(Color.rgb(245, 245, 245), Color.rgb(30, 30, 34)))
            view.setTypeface(android.graphics.Typeface.DEFAULT, android.graphics.Typeface.NORMAL)
        }
    }

    /**
     * Plain-text candidate item — no independent keycap background, just the
     * label, matching a stroke candidate bar rather than a row of separate
     * buttons. Height always comes from the parent row (MATCH_PARENT) so it
     * can never itself grow the fixed 36dp candidate row.
     */
    internal fun candidateItemView(
        label: String,
        isFirst: Boolean,
        onLongPress: (() -> Unit)? = null,
        action: () -> Unit,
    ): TextView {
        return keyboardKey(label, 1f, action = action, longPressAction = onLongPress).apply {
            // Was 20f/dp(9); briefly tried 17f/dp(12) to fix a clipped-glyph
            // report on a Xiaomi MIUI device (that OEM's default system
            // font renders visibly wider per character than stock Android
            // at the same sp), but 20sp reads noticeably better across
            // devices — settled on keeping the original text size and
            // widening padding a bit instead (dp(9) -> dp(11)), which still
            // gives MIUI's wider glyphs a bit more breathing room without
            // shrinking the text everyone else sees.
            textSize = 20f
            setSingleLine(true)
            maxLines = 1
            background = null
            elevation = 0f
            translationZ = 0f
            setPadding(dp(11), 0, dp(11), 0)
            styleCandidateFirstState(this, isFirst)
        }
    }

    /**
     * "Show more candidates" overlay — a PopupWindow anchored below the
     * candidate row, wrapping the current full candidate/association list
     * (whichever is showing) into a flow of rows. A PopupWindow floats over
     * the existing panel without resizing or displacing it, matching "不允许
     * 推动下方按键或改变键盘高度".
     */
    internal fun showCandidateOverlay(anchor: View) {
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
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, keyboardPanelHeightPx())
            minimumHeight = keyboardPanelHeightPx()
            setPadding(dp(8), dp(8), dp(8), dp(8))
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
        }
        root.addView(buildVoiceHeader(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(38)).apply {
            marginStart = dp(8)
            marginEnd = dp(8)
        })

        // Most-recent-three-clips quick-tap strip (was four rows — reduced
        // per user request, the freed space redistributed evenly across the
        // remaining three so RECENT_CLIPS_COLUMN_HEIGHT_DP's own total is
        // unchanged; see that constant's doc comment for the exact math).
        // The rows use circled Unicode digits (①②③) for their prefixes; the
        // glyphs come from Android's normal font fallback rather than a
        // hand-drawn canvas shape. Flat gray now (was white prefix + cherry
        // red content, before that a blue accent) — per user request, no
        // color distinction between the prefix and the clip text anymore.
        // The action grid below shrinks by the strip's fixed height
        // automatically (LinearLayout's own weight=1f on `grid` further
        // down already absorbs however much space this strip doesn't use —
        // no separate shrink logic needed). Thin divider lines separate the
        // rows rather than a colored background, matching the plain-divider
        // treatment already used for the English candidate bar.
        val circledIndices = listOf('①', '②', '③')
        val recentClipTextColor = tone(Color.rgb(190, 190, 190), Color.rgb(140, 140, 145))
        fun recentClipRow(index: Int, entry: ClipboardEntry?): TextView = TextView(this).apply {
            text = entry?.let { "${circledIndices[index]} ${it.text}" } ?: ""
            textSize = 16f
            maxLines = 1
            ellipsize = android.text.TextUtils.TruncateAt.END
            gravity = android.view.Gravity.CENTER_VERTICAL
            setTextColor(recentClipTextColor)
            setPadding(dp(10), 0, dp(10), 0)
            if (entry != null) {
                isClickable = true
                setOnClickListener {
                    currentInputConnection?.commitText(entry.text, 1)
                    OpenLessClipboardHistory.recordCopy(this@OpenLessImeService, entry.text)
                    refreshInputView()
                }
            }
        }
        // distinctBy(text): recordCopy() already dedupes going forward (an
        // existing entry is moved to the front instead of a second row being
        // added), but this guards the display itself against any duplicate
        // already sitting in an existing history file (e.g. one written
        // before that dedup existed) rather than trusting the stored data.
        val recentClips = OpenLessClipboardHistory.load(this).distinctBy { it.text }
        val recentClipsColumn = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        recentClipsColumn.addView(buildDivider(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)))
        for (index in 0 until 3) {
            recentClipsColumn.addView(
                recentClipRow(index, recentClips.getOrNull(index)),
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f),
            )
            recentClipsColumn.addView(buildDivider(), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)))
        }
        // Added to root below the action grid, not here — per user request,
        // the keyboard/action grid sits above the recent-clips strip now
        // (was the other way around).

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
        fun quickActionLabel(label: String, action: () -> Unit, textSizeSp: Float, highlighted: Boolean = false, midDivider: Boolean = false) =
            keyboardKey(label, 1f, action = action, midDivider = midDivider).apply {
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
            // Unlike the stroke panel's always-dark-red action keys (white
            // in both themes, StrokeActionView's own default), these icons
            // normally sit on a normal or rose key that flips with the
            // theme — pass an explicit color for a caller whose key is red
            // regardless of theme (e.g. backspaceKey below), same reasoning
            // as keyboardKey()'s own graphicIconColor doc comment.
            iconColor: Int = tone(Color.WHITE, Color.rgb(30, 30, 34)),
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
            graphicIconColor = iconColor,
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
        val selectKey = quickActionLabel(ui("选择", "Range"), {
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
        val selectAllKey = quickActionLabel(ui("全选", "Select"), {
            currentInputConnection?.performContextMenuAction(android.R.id.selectAll)
        }, 20f)
        val copyKey = quickActionLabel(ui("复制", "Copy"), {
            currentInputConnection?.performContextMenuAction(android.R.id.copy)
        }, 20f)
        val pasteKey = quickActionLabel(ui("粘贴", "Paste"), {
            currentInputConnection?.performContextMenuAction(android.R.id.paste)
        }, 20f)
        // Two lines ("History" / "Dict") since this button now opens both
        // the clipboard history browser (tap) and the voice-correction flow
        // for whatever's selected in the real input field (long-press) —
        // labeled "Dict" since the long-press flow writes to the global
        // Dictionary now, not a correction rule.
        // quickActionLabel()->keyboardKey()'s own '\n' handling shrinks only
        // the label's first character (built for single-char rows like
        // "1\n!"), which would leave just one letter undersized here, so the
        // spanned text it sets is overwritten with a plain two-line string
        // right after construction.
        // Same cherry red as the stroke panel's own right-side action rail
        // (Color.rgb(153, 26, 40)) — per product request, so these two keys
        // read as the equivalent "action" keys on this panel.
        val clipboardKey = quickActionLabel(ui("历史\n字典", "History\nDict"), {
            clipboardHistoryMode = true
            refreshInputView()
        }, 17f, midDivider = true).apply {
            text = ui("历史\n字典", "History\nDict")
            background = roundedButton(Color.rgb(153, 26, 40), dp(5))
            // quickActionLabel()'s own softWhite text color reads fine on a
            // normal/rose key but not on this now-red one (dark gray in
            // light theme, on dark red) — fixed white regardless of theme,
            // same reasoning as backspaceKey's iconColor override below.
            setTextColor(Color.WHITE)
            setOnLongClickListener {
                openSelectedTextCorrectionViaVoice()
                true
            }
        }
        val backspaceKey = quickActionIcon(
            "backspace-icon",
            { deleteBackward() },
            repeatOnLongPress = true,
            iconColor = Color.WHITE,
        ).apply {
            background = roundedButton(Color.rgb(153, 26, 40), dp(5))
        }
        for (key in listOf(selectAllKey, copyKey, pasteKey, clipboardKey, backspaceKey)) {
            row2.addView(key, cell())
        }
        grid.addView(row2, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
            topMargin = gap / 2
        })

        root.addView(grid, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
            topMargin = dp(8)
        })
        root.addView(recentClipsColumn, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(RECENT_CLIPS_COLUMN_HEIGHT_DP)).apply {
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
        // Assigned once `scroll` itself is built, further down — captured
        // by reference here so SwipeModeContainer's verticalDismissAllowed
        // lambda reads whatever scroll position exists at actual touch
        // time (the lambda only ever runs from a real user gesture, well
        // after this whole view is fully constructed).
        var historyScroll: android.widget.ScrollView? = null
        // Rows handle their own left/right swipe (favorite / correction
        // rule); the panel-switch swipe would otherwise compete for the
        // exact same gesture. A downward drag only dismisses the keyboard
        // once the list below is already scrolled to its own top — per
        // user request, so scrolling up through history doesn't get
        // mistaken for "swipe down to hide" partway through.
        val root = SwipeModeContainer(
            this,
            horizontalSwipeEnabled = false,
            verticalDismissAllowed = { (historyScroll?.scrollY ?: 0) == 0 },
        ) { direction -> swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, keyboardPanelHeightPx())
            minimumHeight = keyboardPanelHeightPx()
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
                if (englishUi) {
                    typeface = Typeface.create("sans-serif-medium", if (selected) Typeface.BOLD else Typeface.NORMAL)
                } else {
                    setTypeface(typeface, if (selected) android.graphics.Typeface.BOLD else android.graphics.Typeface.NORMAL)
                }
                setOnClickListener {
                    clipboardHistoryCategory = category
                    refreshInputView()
                }
            }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))
        }
        // Not a category filter like the five tabs above — an action:
        // deletes every non-favorited history entry outright (favorites are
        // kept). Never shows "selected" styling since it isn't one of
        // clipboardHistoryCategory's own values. No confirmation step per
        // the request as given; only this app's own local history is
        // affected, not anything external.
        tabsRow.addView(TextView(this).apply {
            text = ui("清空", "Clear")
            textSize = 14f
            gravity = android.view.Gravity.CENTER
            setTextColor(tone(Color.rgb(190, 190, 190), Color.rgb(140, 140, 145)))
            if (englishUi) {
                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            }
            setOnClickListener {
                OpenLessClipboardHistory.clearNonFavorites(this@OpenLessImeService)
                refreshInputView()
            }
        }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))
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
        // The Dictionary lives in the Rust backend (CORE_BACKEND), which is
        // only registered once mobile_runtime::run()'s setup() has actually
        // executed — not guaranteed just because the IME is showing. Without
        // this, nativeVocabularyPhrases()/nativeAddVocabularyWord() below
        // silently no-op against an unregistered backend: the star-like icon
        // never appears and the swipe zone's "add" never takes, with no
        // visible error either place.
        ensureBackendReady()
        // Fetched once per panel build, not per row: a native round trip
        // per row would be wasted work when a single JSON snapshot already
        // answers "is this text already in the dictionary" for all of them.
        val vocabularyPhrases: Set<String> = try {
            val array = org.json.JSONArray(OpenLessNative.nativeVocabularyPhrases())
            (0 until array.length()).mapTo(mutableSetOf()) { array.getString(it) }
        } catch (error: Exception) {
            emptySet()
        }
        if (entries.isEmpty()) {
            listContainer.addView(TextView(this).apply {
                text = ui("暂无粘贴板记录", "No clipboard history yet")
                textSize = 14f
                if (englishUi) typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
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
                if (vocabularyPhrases.contains(entry.text)) {
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
                    hasCorrectionRule = { vocabularyPhrases.contains(entry.text) },
                    addLabel = ui("加入收藏", "Add"),
                    removeLabel = ui("取消收藏", "Remove"),
                    addCorrectionLabel = ui("加入词典", "Add to dictionary"),
                    removeCorrectionLabel = ui("移出词典", "Remove from dictionary"),
                    onSuppressClick = { suppressRowClick = true },
                    onToggleFavorite = {
                        OpenLessClipboardHistory.toggleFavorite(this, entry.text)
                        refreshInputView()
                    },
                    onDelete = {
                        OpenLessClipboardHistory.delete(this, entry.text)
                        refreshInputView()
                    },
                    onAddCorrection = {
                        runNativeAction("加入词典") { OpenLessNative.nativeAddVocabularyWord(entry.text) }
                        refreshInputView()
                    },
                    onRemoveCorrection = {
                        OpenLessNative.nativeRemoveVocabularyWord(entry.text)
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
        historyScroll = scroll
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

    internal fun keyboardKey(
        label: String,
        weight: Float,
        action: () -> Unit = {},
        repeatOnLongPress: Boolean = false,
        repeatAction: (() -> Unit)? = null,
        swipeUpAction: (() -> Unit)? = null,
        swipePreview: String? = null,
        // A single-shot delayed action, separate from
        // repeatOnLongPress/repeatAction — used by the voice/mic key, whose
        // long-press (switch to Voice mode) needs a longer fuse than the
        // system's fixed ~500ms long-click timeout so a swipe-up gesture
        // that pauses briefly before moving isn't preempted by it (see
        // ACTION_MOVE below, which cancels this the moment the swipe-up
        // threshold is crossed). setOnLongClickListener's timeout isn't
        // adjustable per-view, so this schedules/cancels its own Handler
        // callback instead of relying on it.
        longPressAction: (() -> Unit)? = null,
        longPressDelayMs: Long = 500L,
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
        // Draws a thin horizontal divider between the two lines of a
        // two-line label (e.g. the clipboard panel's "History"/"Dict"
        // key, which does two unrelated things depending on tap vs.
        // long-press) — a plain '\n' alone read as one cramped label.
        midDivider: Boolean = false,
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
            midDivider -> MidDividerTextView(this, tone(Color.rgb(100, 100, 100), Color.rgb(205, 205, 209)))
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
            // Retargetable like buildEnglishCharKey()'s tap preview: any
            // sibling key built with its own swipeUpAction/swipePreview
            // carries that pair here as its tag, so a swipe-up gesture that
            // drags sideways onto a different key (e.g. across the stroke
            // grid's digit shortcuts) can look up and commit THAT key's own
            // digit instead of the one originally pressed.
            tag = if (swipeUpAction != null && swipePreview != null) Pair(swipeUpAction, swipePreview) else null
            var suppressNextClick = false
            var downX = 0f
            var downY = 0f
            var swipePreviewShown = false
            // A one-way latch, same as swipePreviewShown: once the finger
            // has dragged sideways far enough from where it started, the
            // gesture is treated as a deliberate retarget-drag for the
            // rest of this touch sequence, not re-checked drag-by-drag.
            // Before that latch trips, trackedView never changes even if
            // the finger's raw position happens to land inside another
            // key's real hit bounds — a plain swipe straight up (whose own
            // vertical travel easily reaches a neighboring row) must not
            // silently relabel itself just because it crossed into that
            // row's territory with no real sideways intent.
            var horizontalDragArmed = false
            var trackedView: View = this
            @Suppress("UNCHECKED_CAST")
            fun swipeSpecOf(view: View): Pair<() -> Unit, String>? = view.tag as? Pair<() -> Unit, String>
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
            val longPressHandler = if (longPressAction != null) Handler(Looper.getMainLooper()) else null
            var longPressRunnable: Runnable? = null

            // Hit-tests every swipe-capable sibling in this key's own row
            // group, climbing to the row's own parent so a drag can
            // retarget across rows too — same approach as
            // buildEnglishCharKey()'s findTrackTargetAt, just keyed off a
            // Pair(swipeUpAction, swipePreview) tag instead of a char tag.
            //
            // Vertical slop is deliberately tiny here (unlike the English
            // version's generous dp(24)): the grid's rows sit almost flush
            // against each other (~1dp margin, each row only ~1/4 of the
            // grid's own height), so a large slop makes adjacent rows'
            // hit zones overlap deeply — a plain straight swipe UP (whose
            // whole point is to travel well past the pressed key, still
            // within the same key's column) would then immediately
            // "retarget" to the row above just from that expected vertical
            // travel, with no real leftward/rightward drag at all. A
            // near-zero vertical slop means only a finger that has
            // genuinely reached a different row's real bounds retargets;
            // once the finger is past every row (still swiping straight
            // up), no row matches and findSwipeTargetAt falls back to
            // returning trackedView — i.e. the original key stays selected
            // for a simple swipe, exactly matching a single-key swipe-up's
            // intent.
            fun findSwipeTargetAt(rawX: Float, rawY: Float): View {
                val row = parent as? ViewGroup ?: return this
                val group = row.parent as? ViewGroup
                val candidateRows = if (group != null) {
                    (0 until group.childCount).mapNotNull { index ->
                        (group.getChildAt(index) as? ViewGroup)?.takeIf { candidate ->
                            (0 until candidate.childCount).any { candidate.getChildAt(it).tag is Pair<*, *> }
                        }
                    }
                } else {
                    listOf(row)
                }
                for (candidateRow in candidateRows) {
                    for (index in 0 until candidateRow.childCount) {
                        val sibling = candidateRow.getChildAt(index)
                        if (sibling.tag !is Pair<*, *>) continue
                        val loc = IntArray(2)
                        sibling.getLocationOnScreen(loc)
                        if (rawX >= loc[0] - dp(6) && rawX < loc[0] + sibling.width + dp(6) &&
                            rawY >= loc[1] - dp(2) && rawY < loc[1] + sibling.height + dp(2)
                        ) {
                            return sibling
                        }
                    }
                }
                return trackedView
            }

            setOnTouchListener { view, event ->
                when (event.actionMasked) {
                    MotionEvent.ACTION_DOWN -> {
                        downX = event.x
                        downY = event.y
                        horizontalDragArmed = false
                        trackedView = view
                        view.animate()
                            .scaleX(0.97f)
                            .scaleY(0.97f)
                            .translationZ(dp(3).toFloat())
                            .alpha(0.90f)
                            .setDuration(65L)
                            .start()
                        performKeyHaptic()
                        if (longPressHandler != null && longPressAction != null) {
                            val runnable = Runnable {
                                longPressAction.invoke()
                                suppressNextClick = true
                            }
                            longPressRunnable = runnable
                            longPressHandler.postDelayed(runnable, longPressDelayMs)
                        }
                    }
                    MotionEvent.ACTION_MOVE -> {
                        if (swipeUpAction != null && swipePreview != null) {
                            if (!swipePreviewShown && downY - event.y >= dp(10)) {
                                // Armed only once the initial swipe-up
                                // actually clears the threshold, not on
                                // every key's plain DOWN (that would also
                                // swallow the panel-switch swipe that
                                // legitimately starts on top of a
                                // non-swiping key) — so dragging across the
                                // grid to retarget isn't stolen by
                                // SwipeModeContainer's own swipe intercept
                                // mid-gesture.
                                view.parent?.requestDisallowInterceptTouchEvent(true)
                                // Cancels the voice/mic key's own delayed
                                // longPressAction (switch to Voice mode) the
                                // moment a swipe-up is recognized — a no-op
                                // for every other key (longPressRunnable is
                                // only ever set when longPressAction != null).
                                longPressRunnable?.let { longPressHandler?.removeCallbacks(it) }
                                swipePreviewShown = true
                            }
                            if (swipePreviewShown) {
                                // Retargeting itself is gated behind a
                                // second, separate threshold: the finger
                                // must have moved at least dp(10)
                                // horizontally from where it first went
                                // down, not merely landed inside another
                                // key's hit bounds. A plain swipe straight
                                // up travels well past the pressed key's
                                // own row by design (that vertical distance
                                // is the whole gesture), which — with no
                                // sideways intent at all — would otherwise
                                // still land inside a neighboring row's real
                                // bounds and silently relabel the bubble
                                // (e.g. "0" swiped straight up briefly
                                // reading as "8", the key directly above
                                // it). Once this latches on, though, any
                                // further drag — down, sideways, or across
                                // rows — keeps retargeting and following
                                // the finger for the rest of the gesture;
                                // release always commits whichever key is
                                // currently tracked (see ACTION_UP).
                                if (!horizontalDragArmed && kotlin.math.abs(event.x - downX) >= dp(10)) {
                                    horizontalDragArmed = true
                                }
                                if (horizontalDragArmed) {
                                    val target = findSwipeTargetAt(event.rawX, event.rawY)
                                    if (target !== trackedView) {
                                        trackedView = target
                                        // Same haptic as a normal key press,
                                        // so retargeting onto a new key
                                        // reads as landing on that key, not
                                        // just a silent label swap.
                                        performKeyHaptic()
                                    }
                                }
                                val spec = swipeSpecOf(trackedView)
                                if (spec != null) {
                                    // Pinned to the tracked key's own actual
                                    // on-screen position, a fixed gap above
                                    // it (see KeyPreviewOverlay.showSwipePreview()) —
                                    // not to the finger's current height.
                                    keyPreviewOverlay?.showSwipePreview(spec.second, trackedView)
                                }
                            }
                        }
                    }
                    MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                        if (event.actionMasked == MotionEvent.ACTION_UP && swipePreviewShown) {
                            swipeSpecOf(trackedView)?.first?.invoke()
                            suppressNextClick = true
                        }
                        longPressRunnable?.let { longPressHandler?.removeCallbacks(it) }
                        repeatHandler?.let { handler ->
                            repeatRunnable?.let { handler.removeCallbacks(it) }
                        }
                        if (swipePreviewShown) {
                            keyPreviewOverlay?.hide()
                            swipePreviewShown = false
                        }
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
        }.onFailure { error ->
            android.util.Log.w("OpenLessImeService", "key haptic failed durationMs=$durationMs amplitude=$amplitude", error)
        }
    }

    // Two short ticks instead of one — used for the mic button's swipe
    // gestures (cancel-recording, arm raw mode) so they read as distinctly
    // different from a plain key press's single tick. The gap between them
    // reuses the same user-configurable repeat interval as key-repeat
    // haptics, since that's already tuned to feel like separate pulses
    // rather than one long buzz.
    private fun performDoubleKeyHaptic() {
        performKeyHaptic()
        android.os.Handler(Looper.getMainLooper()).postDelayed({ performKeyHaptic() }, keyRepeatIntervalMs())
    }

    /**
     * A single ABC/123/#+= character-emitting key with an iOS-style press
     * preview: a bubble above the key showing the character that will
     * actually be committed (respecting shift for letters), with an arrow
     * pointing at the key (KeyPreviewBubbleView, plain Canvas drawing — no
     * new dependency). Dragging onto a horizontally adjacent sibling in the
     * same row re-targets the preview and the eventual commit to that key
     * instead (a "sloppy key", same idea as most predictive keyboards).
     *
     * @param swipeSymbol Any key that has one — top-row digits (q..p ->
     *   1..0), row2's @#$%&-+() and row3's :;'.,!? — swiping up commits
     *   this symbol instead of the letter, same interaction as the stroke
     *   panel's own swipe-up keys (see keyboardKey()'s swipeUpAction/
     *   swipePreview). Shown as a small standalone hint pinned to the very
     *   top of the key, NOT a second line of the letter's own text (that
     *   was the first version of this — a single TextView with "symbol\n
     *   letter" and block-centered gravity, which visibly shoved the
     *   letter down off-center by about half the hint line's height).
     *   Two independent views layered in a FrameLayout instead: the letter
     *   stays exactly where it always was, completely unaffected by
     *   whether a swipe-up hint exists at all.
     *
     *   This is a dedicated builder, not a keyboardKey() variant, precisely
     *   so none of this touch handling can affect any other panel's keys.
     */
    private fun buildEnglishCharKey(baseChar: String, weight: Float, swipeSymbol: String? = null): View {
        fun charFor(view: View): String {
            val base = view.tag as? String ?: return ""
            return if (shiftState != ShiftState.OFF && base.length == 1 && base[0].isLetter()) base.uppercase() else base
        }
        val letterView = TextView(this).apply {
            textSize = 22f
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            gravity = android.view.Gravity.CENTER
            setTextColor(tone(Color.rgb(245, 245, 245), Color.rgb(30, 30, 34)))
            isClickable = false
            // A small nudge off dead-center, down toward the digit/symbol
            // hint's baseline — a plain translationY (not padding) so it's
            // a pure render-time shift that doesn't touch this view's
            // measured layout at all.
            translationY = dp(LETTER_VERTICAL_NUDGE_DP).toFloat()
        }
        return FrameLayout(this).apply {
            tag = baseChar
            if (swipeSymbol != null) englishSwipeSymbols[this] = swipeSymbol
            letterView.text = charFor(this)
            addView(letterView, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
            if (swipeSymbol != null) {
                addView(
                    TextView(this@OpenLessImeService).apply {
                        text = swipeSymbol
                        textSize = SWIPE_SYMBOL_HINT_TEXT_SIZE_SP
                        gravity = android.view.Gravity.TOP or android.view.Gravity.CENTER_HORIZONTAL
                        // Muted/secondary, not the same bright/near-black
                        // tone as the letter — a small top-corner hint
                        // should read as secondary at a glance, not
                        // compete with the actual letter for attention.
                        // (Tried matching the swipe-preview bubble's red
                        // here — read as less legible at this size than
                        // the plain gray, so reverted.)
                        setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(140, 140, 145)))
                        setPadding(0, dp(SWIPE_SYMBOL_HINT_TOP_PADDING_DP), 0, 0)
                        isClickable = false
                    },
                    FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.WRAP_CONTENT, android.view.Gravity.TOP),
                )
            }
            background = roundedButton(tone(Color.rgb(52, 52, 54), Color.rgb(255, 255, 255)), dp(5))
            elevation = dp(5).toFloat()
            translationZ = dp(1).toFloat()
            contentDescription = charFor(this)
            var trackedView: View = this
            var downY = 0f
            var swipePreviewShown = false

            // Shown via the shared top-level KeyPreviewOverlay (see
            // wrapWithKeyPreviewOverlay()) instead of a per-key PopupWindow —
            // a single overlay reused by every key means dragging onto a
            // different key just repositions the same bubble instead of
            // tearing one popup down and standing up another.
            fun showBubbleFor(target: View) {
                keyPreviewOverlay?.showTapPreview(charFor(target), target)
            }

            // Hit-tests every char-key row in this key's own row group (not
            // just its immediate row siblings) by their actual on-screen
            // bounds — a little slop added on both axes, since a real
            // finger drifts while sliding. Climbing to the row's own parent
            // and collecting every child row that has at least one
            // String-tagged (char) key lets a vertical drag retarget across
            // rows (e.g. row2's "a" up to row1's "q") the same way a
            // horizontal drag already retargets within one row — the
            // candidate bar, header, and bottom row (space/return/mode
            // toggle) never have any String-tagged children, so they're
            // simply never included. Functional keys within an included row
            // (shift/backspace, tag == null) are still never matched.
            fun findTrackTargetAt(rawX: Float, rawY: Float): View {
                val row = parent as? ViewGroup ?: return this
                val group = row.parent as? ViewGroup
                val candidateRows = if (group != null) {
                    (0 until group.childCount).mapNotNull { index ->
                        (group.getChildAt(index) as? ViewGroup)?.takeIf { candidate ->
                            (0 until candidate.childCount).any { candidate.getChildAt(it).tag is String }
                        }
                    }
                } else {
                    listOf(row)
                }
                for (candidateRow in candidateRows) {
                    for (index in 0 until candidateRow.childCount) {
                        val sibling = candidateRow.getChildAt(index)
                        if (sibling.tag !is String) continue
                        val loc = IntArray(2)
                        sibling.getLocationOnScreen(loc)
                        if (rawX >= loc[0] - dp(6) && rawX < loc[0] + sibling.width + dp(6) &&
                            rawY >= loc[1] - dp(24) && rawY < loc[1] + sibling.height + dp(24)
                        ) {
                            return sibling
                        }
                    }
                }
                return trackedView
            }

            // Same idea as findTrackTargetAt above, but only among this
            // row's own swipe-symbol-bearing keys (englishSwipeSymbols) —
            // once a swipe is armed, sliding sideways still retargets which
            // symbol release will commit, and showSwipePreview() re-anchors
            // to that key so the bubble visibly follows the finger instead
            // of freezing above the key the gesture started on. A generous
            // vertical band since an armed drag has already moved well
            // above the row.
            fun findSwipeTargetAt(rawX: Float, rawY: Float): View {
                val row = parent as? ViewGroup ?: return trackedView
                for (index in 0 until row.childCount) {
                    val sibling = row.getChildAt(index)
                    if (sibling !in englishSwipeSymbols) continue
                    val loc = IntArray(2)
                    sibling.getLocationOnScreen(loc)
                    if (rawX >= loc[0] - dp(6) && rawX < loc[0] + sibling.width + dp(6) &&
                        rawY >= loc[1] - dp(60) && rawY < loc[1] + sibling.height + dp(24)
                    ) {
                        return sibling
                    }
                }
                return trackedView
            }

            setOnTouchListener { view, event ->
                when (event.actionMasked) {
                    MotionEvent.ACTION_DOWN -> {
                        // Without this, a same-row drag to an adjacent key
                        // (a clearly horizontal drag, same shape as a
                        // panel-switch swipe) gets stolen mid-gesture by the
                        // enclosing SwipeModeContainer's own horizontal-swipe
                        // intercept once it crosses touchSlop — the key would
                        // stop receiving MOVE/UP entirely, so the bubble
                        // never follows the finger and nothing ever commits
                        // on release. Claimed unconditionally on DOWN, not
                        // only once a drag is detected: any movement that
                        // starts on a key belongs to that key's own
                        // press/retarget/commit handling, never to a
                        // panel-switch gesture.
                        view.parent?.requestDisallowInterceptTouchEvent(true)
                        trackedView = view
                        downY = event.y
                        swipePreviewShown = false
                        view.animate().scaleX(0.97f).scaleY(0.97f).translationZ(dp(3).toFloat()).alpha(0.90f).setDuration(65L).start()
                        performKeyHaptic()
                        showBubbleFor(view)
                    }
                    MotionEvent.ACTION_MOVE -> {
                        // Swipe-up-for-symbol (swipeSymbol != null; the
                        // digit row, and now row2/row3's punctuation) —
                        // same dp(10) arm threshold as keyboardKey()'s own
                        // swipe handling. Once armed, sliding sideways
                        // still retargets across the row's other
                        // symbol-bearing keys via findSwipeTargetAt (the
                        // bubble follows the finger to whichever symbol is
                        // now under it), separate from findTrackTargetAt's
                        // own retargeting used for the un-armed tap case.
                        //
                        // Live, not a one-way latch: armed is re-evaluated
                        // against the finger's *current* height every move,
                        // not just the first time it crosses the threshold
                        // — dragging back down below it un-arms and falls
                        // through to the normal tap-preview/retarget path
                        // below, giving a way to back out of the symbol
                        // mid-gesture without lifting the finger.
                        if (swipeSymbol != null) {
                            val armed = downY - event.y >= dp(10)
                            if (armed != swipePreviewShown) {
                                view.parent?.requestDisallowInterceptTouchEvent(true)
                                swipePreviewShown = armed
                            }
                            if (swipePreviewShown) {
                                val target = findSwipeTargetAt(event.rawX, event.rawY)
                                if (target !== trackedView) trackedView = target
                                val symbol = englishSwipeSymbols[trackedView] ?: swipeSymbol
                                keyPreviewOverlay?.showSwipePreview(symbol, trackedView)
                                return@setOnTouchListener true
                            }
                        }
                        val target = findTrackTargetAt(event.rawX, event.rawY)
                        if (target !== trackedView) trackedView = target
                        showBubbleFor(trackedView)
                    }
                    MotionEvent.ACTION_UP -> {
                        keyPreviewOverlay?.hide()
                        view.animate().scaleX(1f).scaleY(1f).translationZ(0f).alpha(1f).setDuration(90L).start()
                        if (swipeSymbol != null && swipePreviewShown) {
                            commitEnglishChar(englishSwipeSymbols[trackedView] ?: swipeSymbol)
                        } else {
                            commitEnglishChar(charFor(trackedView))
                        }
                    }
                    MotionEvent.ACTION_CANCEL -> {
                        keyPreviewOverlay?.hide()
                        view.animate().scaleX(1f).scaleY(1f).translationZ(0f).alpha(1f).setDuration(90L).start()
                    }
                }
                true
            }
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, weight).apply {
                setMargins(dp(3), dp(3), dp(3), dp(3))
            }
        }
    }

    private fun commitEnglishChar(char: String) {
        if (char.isEmpty()) return
        if (latinInputMode == LatinInputMode.PINYIN) {
            if (char.length == 1 && char[0].isLetter()) {
                // Never calls commitText() — a letter only feeds the
                // encoding buffer in Pinyin mode; see
                // LitePinyinController's own doc comment.
                litePinyinController.appendLetter(char[0]) { renderPinyinCandidates(it) }
                if (shiftState == ShiftState.SHIFT_ONCE) {
                    shiftState = ShiftState.OFF
                    refreshInputView()
                }
                return
            }
            // A digit/punctuation typed mid-encoding (e.g. from the 123/
            // symbols layer) abandons the encoding the same way space does
            // (plan 3.2 point 9) rather than leaving it stranded, then
            // falls through to commit the symbol itself normally below.
            litePinyinController.clear()
            renderPinyinCandidates(emptyList())
        }
        currentInputConnection?.commitText(char, 1)
        if (char.length == 1 && char[0].isLetter()) {
            englishComposingWord.append(char.lowercase())
            updateEnglishCandidates()
        } else {
            // Digits/punctuation end whatever word was being tracked, same
            // as space/return already do — "it's" typed via the symbols
            // page still finalizes cleanly, just with no candidate query
            // benefit for that particular boundary.
            finalizeEnglishComposingWord()
        }
        if (shiftState == ShiftState.SHIFT_ONCE) {
            shiftState = ShiftState.OFF
            refreshInputView()
        }
    }

    /** Ends the current word: records it for user-frequency learning (if long enough to be a real word) and clears the tracking buffer. */
    private fun finalizeEnglishComposingWord() {
        if (englishComposingWord.isNotEmpty()) {
            val word = englishComposingWord.toString()
            if (englishSuggestionsEnabled() && word.length >= 2) {
                englishCandidateProvider.recordCommit(word)
            }
            englishComposingWord.clear()
        }
        updateEnglishCandidates()
    }

    private fun englishSuggestionsEnabled(): Boolean =
        getSharedPreferences("openless_ime_ui", MODE_PRIVATE).getBoolean("english_suggestions_enabled", true)

    /**
     * Same presentation as the stroke panel's own candidate row: a
     * HorizontalScrollView (so as many words as actually exist can be
     * scrolled through, not a fixed 3-slot row) with the same
     * touch-intercept override and the same overflow-triggered "▼ show
     * more" button opening showCandidateOverlay() — the exact mechanism
     * and candidateItemView() styling the stroke panel itself uses, reused
     * as-is rather than a parallel implementation.
     */
    private fun buildEnglishCandidateBar(): View {
        // Outer vertical wrapper — divider, the actual bar, divider — so
        // hiding it (see updateEnglishCandidates()) when suggestions are
        // off takes both border lines with it instead of leaving two empty
        // lines with nothing between them.
        val wrapper = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        englishCandidateBarContainer = wrapper
        wrapper.addView(
            buildDivider(),
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)).apply {
                marginStart = dp(1)
                marginEnd = dp(1)
            },
        )
        val container = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        wrapper.addView(container, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(36)))
        wrapper.addView(
            buildDivider(),
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(1)).apply {
                marginStart = dp(1)
                marginEnd = dp(1)
                bottomMargin = dp(2)
            },
        )
        val scroll = object : android.widget.HorizontalScrollView(this) {
            override fun onInterceptTouchEvent(ev: MotionEvent): Boolean {
                if (ev.actionMasked == MotionEvent.ACTION_DOWN) {
                    parent?.requestDisallowInterceptTouchEvent(true)
                }
                return super.onInterceptTouchEvent(ev)
            }
        }.apply {
            isHorizontalScrollBarEnabled = false
            isFillViewport = false
            overScrollMode = View.OVER_SCROLL_NEVER
        }
        val row = LinearLayout(this).apply { gravity = android.view.Gravity.CENTER_VERTICAL }
        englishCandidateRow = row
        scroll.addView(row, ViewGroup.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT))
        container.addView(scroll, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))
        val expandButton = StrokeActionView(
            this,
            "triangle-down",
            iconColor = tone(Color.rgb(180, 180, 180), Color.rgb(130, 130, 135)),
        ).apply {
            contentDescription = ui("展开更多候选", "Show more candidates")
            visibility = View.GONE
            setOnClickListener { showCandidateOverlay(this) }
        }
        englishExpandCandidatesButton = expandButton
        container.addView(expandButton, LinearLayout.LayoutParams(dp(28), ViewGroup.LayoutParams.MATCH_PARENT))
        // Same idea as the stroke candidate row's own listener: only shown
        // once the candidates actually overflow the visible scroll width.
        scroll.viewTreeObserver.addOnGlobalLayoutListener {
            expandButton.visibility = if (row.width > scroll.width) View.VISIBLE else View.GONE
        }
        return wrapper
    }

    /**
     * Re-queries EnglishCandidateProvider for the current englishComposingWord
     * prefix (or clears the bar if suggestions are off / nothing is being
     * typed) — called after every letter, backspace, and word boundary.
     * englishCandidateQueryEpoch discards a stale async result that comes
     * back after a faster subsequent keystroke already moved the prefix on.
     */
    private fun updateEnglishCandidates() {
        if (englishCandidateRow == null) return
        if (!englishSuggestionsEnabled()) {
            englishCandidateBarContainer?.visibility = View.GONE
            return
        }
        englishCandidateBarContainer?.visibility = View.VISIBLE
        val prefix = englishComposingWord.toString()
        if (prefix.isEmpty()) {
            renderEnglishCandidates(emptyList())
            return
        }
        val epoch = ++englishCandidateQueryEpoch
        englishCandidateProvider.queryTopN(prefix, ENGLISH_CANDIDATE_QUERY_LIMIT) { results ->
            if (epoch == englishCandidateQueryEpoch) renderEnglishCandidates(results)
        }
    }

    private fun renderEnglishCandidates(words: List<String>) {
        val row = englishCandidateRow ?: return
        row.removeAllViews()
        val overlayEntries = mutableListOf<Pair<String, () -> Unit>>()
        words.forEachIndexed { index, word ->
            row.addView(
                candidateItemView(
                    word,
                    isFirst = index == 0,
                    action = { selectEnglishCandidate(word) },
                    onLongPress = { forgetEnglishCandidate(word) },
                ),
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT),
            )
            overlayEntries.add(word to { selectEnglishCandidate(word) })
        }
        // Shared with the stroke panel's own candidate row (only one of the
        // two is ever visible at a time) — see showCandidateOverlay().
        candidateOverlayEntries = overlayEntries
        if (words.isEmpty()) englishExpandCandidatesButton?.visibility = View.GONE
    }

    /**
     * Tapping a candidate replaces the just-typed prefix with the full word
     * plus a trailing space. Verifies the actual field content matches
     * englishComposingWord (trying progressively shorter suffixes, same
     * defensive technique as invalidateDictationResultIfTextChanged())
     * before deleting, rather than trusting the buffer's length blindly —
     * without this, any drift between the two left the old prefix letters
     * sitting in front of the inserted word instead of being replaced by it.
     */
    private fun selectEnglishCandidate(word: String) {
        val connection = currentInputConnection ?: return
        val typed = englishComposingWord.toString()
        for (length in typed.length downTo 1) {
            val actual = connection.getTextBeforeCursor(length, 0)?.toString()
            if (actual != null && actual.equals(typed.takeLast(length), ignoreCase = true)) {
                connection.deleteSurroundingText(length, 0)
                break
            }
        }
        connection.commitText("$word ", 1)
        englishCandidateProvider.recordCommit(word)
        englishComposingWord.clear()
        updateEnglishCandidates()
        performKeyHaptic()
    }

    /**
     * Long-press on an English candidate — only does anything for a word
     * the user's own typing taught this keyboard (see
     * EnglishCandidateProvider.isCustomWord()); the bundled base dictionary
     * isn't user-removable, so a long-press on one of those is a no-op
     * (silent, not an error toast — a base-dictionary word appearing in the
     * candidate row is completely ordinary, not something to explain away).
     */
    private fun forgetEnglishCandidate(word: String) {
        if (!englishCandidateProvider.isCustomWord(word)) return
        englishCandidateProvider.forgetCustomWord(word)
        performKeyHaptic()
        Toast.makeText(this, ui("已移除“$word”", "Removed \"$word\""), Toast.LENGTH_SHORT).show()
        updateEnglishCandidates()
    }

    /**
     * Renders LitePinyinController's own candidate callback into the same
     * candidateItemView()/englishCandidateRow the English candidates use
     * (plan 5.3's Option B — see docs/pinyin-lite/phase-0-audit.md section
     * 4) — a non-clickable encoding label ("guo") is prepended when there's
     * a non-empty encoding, per phase-0 audit section 5, adding no new row
     * or container so the fixed 300dp panel height is never touched.
     */
    private fun renderPinyinCandidates(candidates: List<String>) {
        val row = englishCandidateRow ?: return
        row.removeAllViews()
        val encoding = litePinyinController.currentEncoding()
        if (encoding.isNotEmpty()) {
            val label = TextView(this).apply {
                text = encoding
                textSize = 16f
                setSingleLine(true)
                gravity = android.view.Gravity.CENTER
                setTextColor(tone(Color.rgb(180, 180, 180), Color.rgb(120, 120, 125)))
                setPadding(dp(9), 0, dp(9), 0)
                isClickable = false
            }
            row.addView(label, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        val overlayEntries = mutableListOf<Pair<String, () -> Unit>>()
        candidates.forEachIndexed { index, char ->
            row.addView(
                candidateItemView(
                    char,
                    isFirst = index == 0,
                    action = { selectPinyinCandidate(char) },
                ),
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT),
            )
            overlayEntries.add(char to { selectPinyinCandidate(char) })
        }
        // Shared with the English/stroke candidate rows (only one of the
        // three is ever visible at a time) — see showCandidateOverlay().
        candidateOverlayEntries = overlayEntries
        if (candidates.isEmpty()) englishExpandCandidatesButton?.visibility = View.GONE
    }

    /** Candidate tap is the ONLY way a pinyin candidate reaches the field — see toggleLatinInputMode()'s doc comment on why Space deliberately never does this. */
    private fun selectPinyinCandidate(char: String) {
        currentInputConnection?.commitText(char, 1)
        // observeCommitForLearning needs the encoding still intact; commitSelection
        // then records frequency under the real sourceKey and consumes that key
        // (full buffer or first syllable) before refreshing the candidate row.
        litePinyinController.observeCommitForLearning(char)
        litePinyinController.commitSelection(char) { renderPinyinCandidates(it) }
        litePinyinController.recordCommittedText(char)
        performKeyHaptic()
        refreshPinyinAssociations()
    }

    /**
     * Post-commit "what word comes next" suggestions — reuses
     * StrokeInputController's own phraseRepository instance (same ~220k-
     * entry index, same user-frequency store Stroke mode already tunes) so
     * a word committed via Pinyin can suggest a continuation the same way
     * Stroke's own commits do, and vice versa. Gated by the same
     * strokeAssociationEnabled preference Stroke's own refreshAssociations()
     * checks — this is explicitly the same feature, not a parallel one.
     */
    private fun refreshPinyinAssociations() {
        if (!OpenLessAndroidPreferences.strokeAssociationEnabled(this)) {
            renderPinyinAssociations(emptyList())
            return
        }
        litePinyinController.queryAssociations(strokeController.phraseRepository) { results ->
            // isEmpty(): guards against the case where the user already
            // started typing a fresh encoding by the time this async result
            // lands — associationEpoch alone only protects against a STALE
            // association query, not against a completely different
            // encoding query having taken over the candidate row since.
            if (latinInputMode == LatinInputMode.PINYIN && litePinyinController.isEmpty()) renderPinyinAssociations(results)
        }
    }

    /** Same row/candidateItemView() machinery as renderPinyinCandidates(), but no encoding label (there's no in-progress encoding here) and suffix-based commit (see selectPinyinAssociation()) instead of committing the whole label — mirrors StrokeInputController.refreshAssociations() exactly. */
    private fun renderPinyinAssociations(candidates: List<StrokePhraseRepository.Candidate>) {
        val row = englishCandidateRow ?: return
        row.removeAllViews()
        val overlayEntries = mutableListOf<Pair<String, () -> Unit>>()
        candidates.forEachIndexed { index, candidate ->
            val displayText = outputScript(candidate.text)
            row.addView(
                candidateItemView(
                    displayText,
                    isFirst = index == 0,
                    action = { selectPinyinAssociation(candidate) },
                ),
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT),
            )
            overlayEntries.add(displayText to { selectPinyinAssociation(candidate) })
        }
        candidateOverlayEntries = overlayEntries
        if (candidates.isEmpty()) englishExpandCandidatesButton?.visibility = View.GONE
    }

    /**
     * Only the part of the association candidate not already on screen gets
     * committed — matches StrokeInputController.commitAssociation() exactly
     * (same StrokePhraseRepository.Candidate type). candidate.matchedPrefix
     * is always populated by StrokePhraseRepository.find() before a
     * candidate is ever returned from searchAsync(), so it's trusted
     * directly here rather than re-deriving the query context.
     */
    private fun selectPinyinAssociation(candidate: StrokePhraseRepository.Candidate) {
        if (isSensitiveField(currentInputEditorInfo) || !candidate.text.startsWith(candidate.matchedPrefix)) return
        val suffix = candidate.text.removePrefix(candidate.matchedPrefix)
        val connection = currentInputConnection ?: return
        if (suffix.isNotEmpty() && !connection.commitText(outputScript(suffix), 1)) return
        if (OpenLessAndroidPreferences.strokeUsageEnabled(this)) {
            strokeController.phraseRepository.recordUsage(candidate.matchedPrefix, candidate.text)
        }
        litePinyinController.recordCommittedText(suffix)
        performKeyHaptic()
        refreshPinyinAssociations()
    }

    /**
     * Backspace, everywhere it's wired up in this service. deleteSurroundingText()
     * alone only ever removes characters relative to the cursor position — it does
     * not reliably consume an active selection (behavior varies by target app), so
     * when there's a real OS-level selection this commits an empty string instead,
     * which every InputConnection implementation replaces the selection with.
     */
    internal fun deleteBackward() {
        val connection = currentInputConnection ?: return
        val selected = connection.getSelectedText(0)
        // Consumed by the very next invalidateDictationResultIfTextChanged()
        // call (from onUpdateSelection(), which this delete triggers) so it
        // skips clearing lastDictationText — deleting via our own backspace
        // (plain or select-all-then-backspace, both land here) should stay
        // undoable, unlike text disappearing for some other reason (e.g. the
        // host app clearing the field itself after sending).
        selfInitiatedTextChange = true
        if (!selected.isNullOrEmpty()) {
            connection.commitText("", 1)
        } else {
            connection.deleteSurroundingText(1, 0)
        }
    }

    internal fun sendEnterKey() {
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
        restoreLatinInputMode()
        refreshLanguage()
        strokeController.resetForNewInputSession()
        englishLayer = EnglishLayer.LETTERS
        englishComposingWord.clear()
        litePinyinController.clear()
        litePinyinController.resetAssociationContext()
        startRuntimeService()
        sessionEpoch++
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

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        // InputMethodService caches onCreateInputView()'s result across hide/
        // show. Keyboard settings write height/raise prefs while that cached
        // view is still the old footprint — rebuild when they diverge so the
        // next focus actually shows what the settings preview promised.
        val key = keyboardFootprintKey()
        if (appliedKeyboardFootprintKey != key) {
            appliedKeyboardFootprintKey = key
            refreshInputView()
        }
    }

    private fun keyboardFootprintKey(): String =
        "${keyboardPanelHeightDp()}:${raiseHeightDp(this)}"

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
     * Detects the committed dictation text getting cleared by something
     * OTHER than our own backspace key or undo button — e.g. the host app
     * clearing the field itself after sending — so the undo/redo/edit
     * controls don't keep pointing at text that's no longer actually there.
     * Deleting via our own backspace (plain or select-all-then-backspace,
     * see deleteBackward()) is deliberately NOT treated as a reason to hide
     * these: that deletion is itself undoable (the undo button just
     * re-commits lastDictationText), so hiding it would strand an
     * accidental full erase with no way back.
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
        if (selfInitiatedTextChange) {
            selfInitiatedTextChange = false
            return
        }
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

    internal fun toggleDictation() {
        if (isSensitiveField(currentInputEditorInfo)) {
            updateStatus("敏感字段，禁止听写")
            return
        }
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            updateStatus("请先授予麦克风权限")
            return
        }
        if (recording && quickNoteArmed) {
            quickNoteDictation()
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
            // rawModeArmed is set by the mic button's own swipe-up gesture
            // (see onCreateInputView()'s voice-panel branch) while this
            // recording was still in progress — recording itself keeps
            // going after that swipe, only the eventual stop (here, however
            // it's triggered) skips the LLM polish step and inserts the ASR
            // transcript as-is. Mirrors the existing "swipe left on the
            // overlay to finish+translate" gesture contract
            // (OpenLessOverlayService.kt), just decided earlier (at the
            // swipe) instead of at this exact call. cloudNoteArmed
            // deliberately does NOT ride this raw stop — per later product
            // request, the webhook gets the same LLM-polished text a normal
            // commit would, not the verbatim ASR transcript — so it falls
            // through to the plain stop call below; handleImeTextReady() is
            // what actually routes the resulting (polished) text to the
            // webhook instead of the input field.
            if (rawModeArmed) {
                runNativeAction("停止听写") { OpenLessNative.nativeStopDictationForImeWithRaw(true) }
            } else {
                runNativeAction("停止听写") { OpenLessNative.nativeStopDictationForIme() }
            }
        } else {
            // nativeStartDictationForIme() itself never throws when the
            // Rust backend isn't registered yet — that side just logs a
            // warning and no-ops — so runNativeAction()'s catch never fired
            // for this case either: tapping the mic on a cold backend
            // silently did nothing while the UI still claimed "recording".
            // Checking readiness first, before touching any UI state, means
            // a cold tap now honestly says so and kicks off warmup, instead
            // of pretending to record.
            if (!isBackendReady()) {
                if (!awaitingBackendReadyRecheck) {
                    backendRecheckStartedAtMs = android.os.SystemClock.elapsedRealtime()
                    android.util.Log.w("OpenLessImeService", "backend not ready at mic tap; starting recovery watch")
                    OpenLessProcessRestartStats(this, "mictap").recordStart()
                }
                setState("error", ui("服务尚未就绪", "Service not ready yet"))
                voiceLinkWarning?.text = ui("服务尚未就绪，点击重启应用", "Service not ready — tap to restart the app")
                voiceLinkWarning?.visibility = View.VISIBLE
                ensureBackendReady()
                scheduleBackendReadyRecheck()
                return
            }
            recording = true
            processing = false
            rawModeArmed = false
            quickNoteArmed = false
            cloudNoteArmed = false
            // The actual start of a new recording attempt — reset the
            // silence watch and fire the start haptic here, not in
            // onCapsuleStateChanged's "recording" branch: that branch only
            // acted when `recording` was still false at the time, but this
            // line already flips it true before the native "recording"
            // callback ever arrives, so both were structurally unreachable
            // on every normal tap-to-start (silently never firing).
            recordingStartedAtMs = android.os.SystemClock.elapsedRealtime()
            maxObservedLevelThisSession = 0f
            voiceLinkWarning?.visibility = View.GONE
            performKeyHaptic()
            // A genuinely new utterance (not the correction/edit sub-flow,
            // which already hides these via editingDictationResult) makes
            // the previous result's undo/redo/edit controls stale the
            // moment recording starts, not just once the new result
            // replaces them — leaving them up mid-recording risked an undo
            // tap acting on the wrong utterance.
            if (!editingDictationResult) {
                lastDictationText = null
            }
            setState("speaking", "再次点击结束 · 下划取消")
            runNativeAction("开始听写") { OpenLessNative.nativeStartDictationForIme() }
        }
    }

    private fun cancelDictation() {
        recording = false
        processing = false
        rawModeArmed = false
        quickNoteArmed = false
        cloudNoteArmed = false
        invalidateSession("已取消")
        runNativeAction("取消听写") { OpenLessNative.nativeCancelDictation() }
    }

    /**
     * Ends the current recording and archives it as a standalone note
     * instead of inserting anything — armed by the mic button's own
     * swipe-left gesture (quickNoteArmed), then triggered by the next tap
     * that would otherwise stop a normal recording (see toggleDictation()'s
     * own quickNoteArmed check up top). Mirrors cancelDictation()'s shape:
     * settles local state immediately rather than waiting on a native
     * round-trip, since nativeStopDictationAsQuickNote() never calls back
     * with a message of its own (the Rust side deliberately skips
     * notify_ime_text for this path — see native_bridge.rs's
     * spawn_stop_dictation_as_quick_note()).
     */
    private fun quickNoteDictation() {
        recording = false
        processing = false
        rawModeArmed = false
        quickNoteArmed = false
        if (editingDictationResult) {
            editingDictationResult = false
            refreshInputView()
        }
        performDoubleKeyHaptic()
        setState("done", "笔记已记录", QUICK_NOTE_CONFIRMATION_DELAY_MS)
        runNativeAction("速记") { OpenLessNative.nativeStopDictationAsQuickNote() }
    }

    // Also requires a registered Activity Context (see
    // OpenLessNative.nativeHasRegisteredActivityContext()'s doc comment),
    // not just a running backend: the Rust backend can stay healthy long
    // after its last Activity is destroyed, but every dictation/waveform
    // status notification silently fails without one — recording would
    // otherwise start with zero visible feedback (no waveform, no red
    // warning, nothing), looking exactly like the mic tap did nothing.
    private fun isBackendReady(): Boolean = try {
        OpenLessNative.requireBackendContract()
        OpenLessNative.nativeHasRegisteredActivityContext()
    } catch (error: Throwable) {
        false
    }

    /**
     * Runs every BACKEND_HEARTBEAT_INTERVAL_MS for the service's whole
     * lifetime (started in onCreate(), stopped in onDestroy()) — not just
     * while actively recording — so backendLinkIndicator can catch the link
     * having quietly dropped *before* the user taps the mic and gets no
     * waveform, instead of only reacting after a failed tap (see "mictap").
     * On a bad reading it also proactively calls ensureBackendReady() the
     * same way a failed mic tap already does, so the self-recovery this
     * symptom relies on gets a head start instead of waiting for the next
     * tap.
     */
    private fun runBackendHeartbeatCheck() {
        val ready = isBackendReady()
        backendLinkHealthy = ready
        lastHeartbeatElapsedRealtime = android.os.SystemClock.elapsedRealtime()
        lastHeartbeatReady = ready
        // Always repaint the currently mounted indicator. Previously this was
        // skipped while recording/processing, and a recovery could remain
        // visually stale until the user switched panels and rebuilt the View.
        updateBackendLinkIndicator()
        if (!ready) {
            OpenLessProcessRestartStats(this, "heartbeat").recordStart()
            ensureBackendReady()
        }
    }

    /**
     * Ready/recording/processing take priority (mirrors the overlay's own
     * state machine) — the heartbeat's own reading (backendLinkHealthy)
     * only shows through while otherwise idle, as a distinct color rather
     * than silently agreeing with "ready". Not gated on an actual
     * connectivity check (that would need a new ACCESS_NETWORK_STATE
     * permission the manifest doesn't declare yet): isBackendReady()
     * false is already the same signal "mictap" reacts to, and is directly
     * what the user described as the link being "disconnected".
     */
    private fun updateBackendLinkIndicator() {
        val color = when {
            // Matches VoiceButton's own waveform color for each armed
            // gesture exactly (rawWaveformColor's when block) — the
            // breathing dot and the waveform should never disagree about
            // which mode a recording is currently armed for.
            recording && rawModeArmed -> LINK_COLOR_RECORDING_RAW
            recording && quickNoteArmed -> LINK_COLOR_QUICK_NOTE
            recording && cloudNoteArmed -> LINK_COLOR_CLOUD_NOTE
            recording -> LINK_COLOR_RECORDING
            processing -> LINK_COLOR_PROCESSING
            !backendLinkHealthy -> LINK_COLOR_ISSUE
            else -> LINK_COLOR_READY
        }
        (backendLinkIndicator?.background as? GradientDrawable)?.setColor(color)
        backendLinkPulseAnimator?.duration =
            if (color == LINK_COLOR_READY) BACKEND_LINK_READY_PULSE_DURATION_MS else BACKEND_LINK_PULSE_DURATION_MS
    }

    // Guards scheduleBackendReadyRecheck() so a second mic tap while a
    // recheck loop is already running doesn't stack a duplicate one.
    private var awaitingBackendReadyRecheck = false

    // elapsedRealtime() at the moment "not ready" was first detected, so the
    // eventual recovery (or timeout) log can report how long it actually took.
    private var backendRecheckStartedAtMs = 0L

    /**
     * Polls isBackendReady() roughly once a second after showing the
     * "service not ready" warning, since nothing else pushes a "the
     * backend just finished warming up" event into this service — without
     * this, the warning stayed stuck until some unrelated action (another
     * mic tap, switching panels) happened to rebuild/reset it, even though
     * the backend may have become ready seconds earlier in the background.
     * Capped at ~60s of retries rather than polling forever if the backend
     * genuinely never recovers. Raised from an earlier 20s cap after a real
     * device recovery was observed taking ~31s (a double cold-start: the
     * warmup Activity got destroyed once mid-warmup, then succeeded on a
     * second attempt), which exceeded that cap and left the warning stuck.
     */
    private fun scheduleBackendReadyRecheck(attempt: Int = 0) {
        if (attempt == 0) {
            if (awaitingBackendReadyRecheck) return
            awaitingBackendReadyRecheck = true
        }
        if (isBackendReady()) {
            awaitingBackendReadyRecheck = false
            val elapsedMs = android.os.SystemClock.elapsedRealtime() - backendRecheckStartedAtMs
            android.util.Log.i("OpenLessImeService", "backend recovered after ${elapsedMs}ms (attempt=$attempt)")
            voiceLinkWarning?.visibility = View.GONE
            setState("idle", ui("点击开始说话", "Tap to speak"))
            return
        }
        if (attempt >= 60) {
            awaitingBackendReadyRecheck = false
            val elapsedMs = android.os.SystemClock.elapsedRealtime() - backendRecheckStartedAtMs
            android.util.Log.w("OpenLessImeService", "backend still not ready after ${elapsedMs}ms; giving up recheck loop")
            return
        }
        android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
            scheduleBackendReadyRecheck(attempt + 1)
        }, 1000L)
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

    /**
     * `message ?: fallback` only substitutes for a literal null — a native
     * capsule-state event carrying a blank-but-non-null message (empty
     * string, or whitespace) would slip through that check unchanged and
     * land the status line on invisible/blank text, with nothing left to
     * ever correct it (see onCapsuleStateChanged()'s "idle" case, which
     * this guards against overwriting setState()'s own delayed "done" ->
     * "idle" revert with a blank message that then never gets fixed, since
     * state has already moved off "done" by the time that revert's own
     * guard checks it).
     */
    private fun String?.orDefault(fallback: String): String = this?.takeIf { it.isNotBlank() } ?: fallback

    override fun onCapsuleStateChanged(state: String, message: String?, level: Float) {
        voiceButton?.audioLevel = level.coerceIn(0f, 1f)
        when (state) {
            "recording" -> {
                if (!recording) {
                    // Fresh recording attempt — give it a clean silence watch.
                    recordingStartedAtMs = android.os.SystemClock.elapsedRealtime()
                    maxObservedLevelThisSession = 0f
                    voiceLinkWarning?.visibility = View.GONE
                    performKeyHaptic()
                }
                recording = true
                processing = false
                maxObservedLevelThisSession = maxOf(maxObservedLevelThisSession, level)
                val elapsedMs = android.os.SystemClock.elapsedRealtime() - recordingStartedAtMs
                if (maxObservedLevelThisSession >= SILENCE_LEVEL_THRESHOLD) {
                    voiceLinkWarning?.visibility = View.GONE
                } else if (elapsedMs > SILENCE_CHECK_DELAY_MS) {
                    // This only runs once an actual recording session is
                    // underway, so it can't overlap with the "backend not
                    // ready" case in toggleDictation() (that one returns
                    // before a session ever starts) — but set this widget's
                    // own text explicitly anyway rather than trusting
                    // whatever the other case last left it as.
                    voiceLinkWarning?.text = ui("检测到麦克风无声音，点击重启应用", "No mic audio detected — tap to restart the app")
                    voiceLinkWarning?.visibility = View.VISIBLE
                }
                setState("speaking", "再次点击结束 · 下划取消")
            }
            "transcribing" -> {
                // Guarded by the same "was still recording" check as
                // "polishing" below, since either one can be the first to
                // fire after recording actually stops — only whichever
                // gets there first should vibrate.
                if (recording) performKeyHaptic()
                recording = false
                processing = true
                setState("thinking", "正在思考")
            }
            "polishing" -> {
                if (recording) performKeyHaptic()
                recording = false
                processing = true
                setState("thinking", "正在思考")
            }
            "done" -> {
                recording = false
                processing = false
                setState("done", message.orDefault("已完成"))
                performKeyHaptic()
            }
            "cancelled" -> {
                recording = false
                processing = false
                setState("idle", message.orDefault("已取消"))
            }
            "error" -> {
                recording = false
                processing = false
                setState("error", message.orDefault("识别失败"))
            }
            "idle" -> if (!recording) {
                setState("idle", message.orDefault("点击开始说话"))
            }
        }
    }

    private fun invalidateSession(message: String) {
        sessionEpoch++
        updateStatus(message)
    }

    internal fun isSensitiveField(attribute: EditorInfo?): Boolean {
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
        status?.setTextColor(recordingAccentColor())
        voiceRawHint?.text = rawModeHintText()
        voiceRawHint?.setTextColor(rawModeHintColor())
        voiceRawHint?.visibility = if (rawModeHintVisible()) View.VISIBLE else View.GONE
        voiceButton?.isRecording = recording
        voiceButton?.isProcessing = processing
        updateDictationResultControls()
        updateBackendLinkIndicator()
    }

    /**
     * Swipe-up-for-Raw / swipe-left-for-Quick-notes / swipe-right-for-
     * Cloud-notes discoverability hint while idle; once a recording is
     * actually armed into one of those three modes (live through both
     * recording and the following "thinking"/整理 step for Raw and Cloud
     * notes — quick note never reaches "thinking", see
     * quickNoteDictation()), the row repurposes itself to confirm whichever
     * one is armed instead. Recording-or-thinking with none armed (an
     * ordinary dictation) never reaches this text at all — see
     * rawModeHintVisible(), which hides the row entirely for that case.
     */
    private fun rawModeHintText(): String {
        return when {
            (state == "speaking" || state == "thinking") && rawModeArmed -> ui("原样转写", "Raw Mode")
            (state == "speaking" || state == "thinking") && cloudNoteArmed -> ui("云笔记", "Cloud notes")
            state == "speaking" && quickNoteArmed -> ui("速记模式", "Quick notes")
            else -> ui("上划RAW · 左划速记 · 右划云笔记", "Up: Raw · Left: Quick notes · Right: Cloud notes")
        }
    }

    /** Same orange/green/red as the status line's own Raw/Quick-notes/Cloud-notes coloring and every other indicator for each mode (VoiceButton's armed pill/waveform) — muted gray otherwise. */
    private fun rawModeHintColor(): Int {
        return when {
            (state == "speaking" || state == "thinking") && rawModeArmed -> LINK_COLOR_RECORDING_RAW
            (state == "speaking" || state == "thinking") && cloudNoteArmed -> LINK_COLOR_CLOUD_NOTE
            state == "speaking" && quickNoteArmed -> LINK_COLOR_QUICK_NOTE
            else -> Color.argb((0.8f * 255).toInt(), 0xB0, 0xB0, 0xB0)
        }
    }

    /**
     * Hidden while actively recording or thinking in an ordinary dictation
     * (none of Raw/Quick-notes/Cloud-notes armed) — at that point it's neither teaching
     * a still-relevant gesture (idle) nor confirming an active one, just a
     * stray label under the mic. Visible the rest of the time: idle
     * (teaches all three gestures) and recording/thinking once one of them
     * is actually armed (confirms it).
     */
    private fun rawModeHintVisible(): Boolean {
        return !((state == "speaking" || state == "thinking") && !rawModeArmed && !quickNoteArmed && !cloudNoteArmed)
    }

    /**
     * Dispatches a just-finished dictation's text to whichever destination
     * the mic's own swipe gesture armed for this session — an ordinary
     * commit into the input field, or (cloudNoteArmed) a JSON POST to the
     * webhook configured in settings, never both. Wired up as
     * OpenLessOverlayBridge.imeTextListener in onCreate() — every stop this
     * function ever sees has gone through the plain (LLM-polished) pipeline,
     * since toggleDictation() only routes to the Raw stop for rawModeArmed,
     * not cloudNoteArmed (per product request, Cloud notes gets the same
     * polished text a normal commit would); quick note's stop uses a
     * completely separate JNI call/callback that never reaches this
     * function at all (see quickNoteDictation()).
     */
    private fun handleImeTextReady(text: String) {
        if (cloudNoteArmed) {
            cloudNoteArmed = false
            submitCloudNoteText(text)
        } else {
            commitImeText(text)
        }
    }

    /**
     * POSTs the LLM-polished transcript (see handleImeTextReady()'s own doc
     * comment on why this is never the verbatim Raw one) to the URL/token
     * configured in the native settings page's "云笔记提交" section
     * (OpenLessKeyboardSettingsActivity) — never inserted into the input
     * field, never archived locally either (contrast quickNoteDictation(),
     * which does both of those things quick note's own way). Runs the
     * request off the main thread (a plain background Thread, not the
     * shared LitePinyin-style executor — this fires at most once per
     * dictation, not per keystroke) and settles the status bubble once it
     * resolves either way.
     */
    private fun submitCloudNoteText(text: String) {
        if (text.isBlank()) {
            recording = false
            processing = false
            setState("error", "没有识别到文字")
            return
        }
        val preferences = getSharedPreferences("openless_ime_ui", MODE_PRIVATE)
        val url = preferences.getString("key_cloud_note_webhook_url", null)?.trim().orEmpty()
        val token = preferences.getString("key_cloud_note_webhook_token", null)?.trim().orEmpty()
        recording = false
        processing = false
        if (url.isEmpty() || token.isEmpty()) {
            setState("error", "请先在设置中填写云笔记的地址/Token")
            return
        }
        setState("thinking", "正在提交云笔记")
        Thread {
            val mainHandler = android.os.Handler(Looper.getMainLooper())
            try {
                val body = org.json.JSONObject().apply {
                    put("token", token)
                    put("content", text)
                    put("client", "input_method")
                }.toString()
                val connection = (java.net.URL(url).openConnection() as java.net.HttpURLConnection).apply {
                    requestMethod = "POST"
                    setRequestProperty("Content-Type", "application/json; charset=utf-8")
                    doOutput = true
                    connectTimeout = 8000
                    readTimeout = 8000
                }
                connection.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
                val code = connection.responseCode
                connection.disconnect()
                mainHandler.post {
                    if (code in 200..299) {
                        setState("done", "已提交云笔记", QUICK_NOTE_CONFIRMATION_DELAY_MS)
                    } else {
                        setState("error", "云笔记提交失败（$code）")
                    }
                }
            } catch (error: Throwable) {
                android.util.Log.w("OpenLessImeService", "cloud note webhook submit failed", error)
                mainHandler.post {
                    setState("error", "云笔记提交失败，请检查网络")
                }
            }
        }.start()
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
                setColor(tone(Color.rgb(60, 60, 60), Color.rgb(225, 225, 228)))
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
            scheduleRevertToIdle()
        } else {
            if (!connection.commitText(text, 1)) return
            dictationTextUndone = false
            updateStatus("已上屏")
            scheduleRevertToIdle()
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
        // Starts unchecked every time — adding to the Dictionary is an
        // opt-in the user ticks deliberately, not a guess based on how much
        // of the result was selected. The user can still flip the checkbox
        // themselves before finishing.
        addToDictionaryForEdit = false
        editingDictationResult = true
        awaitingEditReplacement = true
        refreshInputView()
        // Recording starts immediately when the edit panel opens — the user
        // only has to tap the mic once, to finish, matching "Tap again to
        // finish" rather than requiring a tap to start too.
        toggleDictation()
    }

    /**
     * Long-press "History"/"历史" in the clipboard panel: same mechanism as
     * openEditDictationResult()'s selected-text branch (replaces the
     * selection in the real input field with the spoken correction) — but
     * with no whole-last-result fallback. Without an actual selection
     * there's nothing this gesture can reasonably act on, so it surfaces a
     * hint instead of guessing.
     */
    private fun openSelectedTextCorrectionViaVoice() {
        val selected = currentInputConnection?.getSelectedText(0)?.toString()?.takeIf { it.isNotEmpty() }
        if (selected == null) {
            Toast.makeText(this, ui("请先选中字词", "Please select word"), Toast.LENGTH_SHORT).show()
            return
        }
        editingOriginalText = selected
        editingReplacesWholeResult = false
        addToDictionaryForEdit = false
        editingDictationResult = true
        awaitingEditReplacement = true
        refreshInputView()
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
        setState("done", "已上屏")
        refreshInputView()
    }

    /**
     * Applies the freshly spoken replacement for editingOriginalText: swaps
     * it into the input field (relying on InputConnection.commitText's
     * standard "replace the active selection" behavior when there is a real
     * OS selection, or an explicit delete+insert when we fell back to the
     * whole last result).
     *
     * Remembering the corrected word/phrase in the global Dictionary
     * (nativeAddVocabularyWord — the same store the desktop Dictionary UI
     * reads/writes; corrections themselves are hand-maintained only, see
     * native_bridge.rs's spawn_add_vocabulary_word()) is gated on
     * addToDictionaryForEdit, opt-out via the edit panel's checkbox (every
     * edit used to silently add one, which piled up entries for edits that
     * were just rewording, not an actual new/misrecognized word).
     */
    private fun finishEditWithSpokenReplacement(text: String) {
        recording = false
        processing = false
        val original = editingOriginalText
        val replacesWhole = editingReplacesWholeResult
        val shouldAddToDictionary = addToDictionaryForEdit
        editingDictationResult = false
        awaitingEditReplacement = false
        editingOriginalText = null
        if (text.isBlank() || original == null) {
            setState("done", "已上屏")
            refreshInputView()
            return
        }
        val connection = currentInputConnection
        if (connection != null) {
            if (replacesWhole) connection.deleteSurroundingText(original.length, 0)
            connection.commitText(text, 1)
        }
        if (shouldAddToDictionary && text != original) {
            runNativeAction("加入词典") {
                OpenLessNative.nativeAddVocabularyWord(text)
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

    private fun setState(nextState: String, message: String, revertDelayMs: Long = DONE_TO_IDLE_DELAY_MS) {
        state = nextState
        updateStatus(message)
        // A completed commit/edit (see the various setState("done", "已
        // 上屏") call sites) used to just sit there until the next recording
        // started; the status line and the Raw hint below it should instead
        // settle back to the same ready state as a fresh session shortly
        // after, so it's visibly clear another dictation can start right
        // away — see scheduleRevertToIdle(). revertDelayMs only ever
        // differs from the default for quickNoteDictation()'s own shorter
        // "笔记已记录" confirmation (QUICK_NOTE_CONFIRMATION_DELAY_MS).
        if (nextState == "done") scheduleRevertToIdle(revertDelayMs)
    }

    /**
     * Shows "点击开始说话"/"Tap to speak" again DONE_TO_IDLE_DELAY_MS after
     * whatever transient confirmation is currently up (已上屏/已完成 via
     * setState()'s "done" branch, or 已撤销/已上屏 from
     * toggleUndoRedoDictation()'s direct updateStatus() calls, which don't
     * go through setState() at all since undo/redo doesn't change `state`).
     * A monotonic token, not a state-string comparison, guards the delayed
     * callback: every call here bumps it, so only the most recently
     * scheduled revert actually fires — tapping undo right after a commit
     * (before the commit's own revert would have fired) correctly restarts
     * the 2-second countdown from the undo instead of the two racing and
     * the earlier one winning. sessionEpoch is still checked too, for a
     * genuinely new input session in the meantime.
     */
    private fun scheduleRevertToIdle(delayMs: Long = DONE_TO_IDLE_DELAY_MS) {
        val epoch = sessionEpoch
        val token = ++statusRevertToken
        android.os.Handler(Looper.getMainLooper()).postDelayed({
            if (sessionEpoch == epoch && statusRevertToken == token) {
                setState("idle", ui("点击开始说话", "Tap to speak"))
            }
        }, delayMs)
    }

    private fun displayStatus(message: String): String {
        if (!englishUi) return message
        return when (message) {
            "点击开始说话" -> "Tap to speak"
            "再次点击结束 · 下划取消" -> "Tap again to finish · Swipe down to cancel"
            "正在思考" -> "Thinking"
            "已完成", "已上屏" -> "Done"
            "笔记已记录" -> "Note saved"
            "正在提交云笔记" -> "Submitting"
            "已提交云笔记" -> "Cloud notes submitted"
            "请先在设置中填写云笔记的地址/Token" -> "Fill in the Cloud notes URL/token in settings first"
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

    internal fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

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

    internal fun roundedButton(color: Int, radius: Int): android.graphics.drawable.Drawable {
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
    internal fun buildEncodeAreaBackground(): android.graphics.drawable.Drawable {
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
    internal class SwipeRail(
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
     * Outer IME host for stretch+raise: key panel on top + empty lift strip
     * below. Forces EXACT total height the same way [SwipeModeContainer]
     * forces the key-panel height (setInputView discards LayoutParams).
     * Re-reads prefs on every measure so a settings change applies on the
     * next layout even before a full view rebuild.
     */
    private class RaisedKeyboardHost(
        context: android.content.Context,
    ) : LinearLayout(context) {
        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
            val panel = panelHeightPx(context)
            val raise = raiseHeightPx(context)
            if (childCount >= 1) {
                getChildAt(0).layoutParams = LayoutParams(LayoutParams.MATCH_PARENT, panel)
            }
            if (childCount >= 2) {
                val spacer = getChildAt(1)
                spacer.layoutParams = LayoutParams(LayoutParams.MATCH_PARENT, raise)
                spacer.visibility = if (raise > 0) View.VISIBLE else View.GONE
            }
            val exactHeightSpec = android.view.View.MeasureSpec.makeMeasureSpec(
                panel + raise,
                android.view.View.MeasureSpec.EXACTLY,
            )
            super.onMeasure(widthMeasureSpec, exactHeightSpec)
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
    internal class SwipeModeContainer(
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
        // Checked once, at the moment a drag first looks vertical enough to
        // possibly claim (see onInterceptTouchEvent's ACTION_MOVE branch) —
        // returning false there lets the gesture fall through to a nested
        // scrollable child instead (e.g. the clipboard history list), so a
        // downward drag only dismisses the keyboard once that child is
        // already scrolled to its own top, the same "pull past the top to
        // dismiss" convention most scrollable sheets use. Defaults to
        // "always allowed" for every panel that has no such nested scroll.
        private val verticalDismissAllowed: () -> Boolean = { true },
        private val onSwipe: (Int) -> Unit,
    ) : LinearLayout(context) {
        private var startX = 0f
        private var startY = 0f
        private var interceptingHorizontal = false
        private var interceptingVertical = false
        private var verticalDismissBlockedForGesture = false
        private val touchSlop = android.view.ViewConfiguration.get(context).scaledTouchSlop
        private val dismissThreshold = (120 * resources.displayMetrics.density).toInt()
        // Committing an actual mode switch needs a much bigger, deliberate
        // drag than the touchSlop-based early claim above (which only
        // exists so a vertical scroll elsewhere doesn't get mistaken for a
        // mode swipe partway through) — a fraction of the panel's own width
        // rather than a fixed dp figure, so the required drag scales with
        // the device instead of feeling different on a small vs. large
        // screen. Computed against `width` at commit time (onTouchEvent),
        // not here at construction, since the view isn't laid out yet.
        private val commitThresholdFraction = 1f / 3f

        init {
            excludeFromSystemGestures(this)
        }

        // InputMethodService.setInputView() re-wraps whatever view we return
        // in its OWN FrameLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT),
        // discarding the fixed-height LayoutParams every panel builder sets
        // on its root — confirmed via a live device: the attached root's
        // own layoutParams.height read back as WRAP_CONTENT (-2), not the
        // configured height we set. Under a generous (near-fullscreen)
        // WRAP_CONTENT/AT_MOST measure spec from the system, any 0dp/weight=1
        // flexible child (the spacer used by several panels) happily expands
        // to fill that huge bound instead of the real configured height.
        // Forcing an EXACTLY height spec here — regardless of what the parent
        // asks for — makes every panel's height genuinely fixed instead of
        // accidental.
        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
            val fixedHeight = panelHeightPx(context)
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
                    // dismisses the keyboard — unless verticalDismissAllowed()
                    // says a nested scrollable child still has room to
                    // scroll up first (the clipboard history list).
                    if (!interceptingHorizontal && !interceptingVertical && !verticalDismissBlockedForGesture &&
                        dy > touchSlop && dy > kotlin.math.abs(dx) * 1.5f && verticalDismissAllowed()
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
                } else if (kotlin.math.abs(dx) > width * commitThresholdFraction) {
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
        // Weakened from a fully-opaque line to a near-invisible one — the
        // floating selected pill below is meant to be the only thing the
        // user reads as "current mode", not these dividers, but the layout
        // math that positions the four icons/labels still uses the same
        // segmentWidth split, so the dividers stay (very faintly) for
        // continuity rather than being deleted outright.
        private val dividerColor = run {
            val base = if (darkTheme) Color.rgb(58, 58, 58) else Color.rgb(200, 200, 204)
            Color.argb(28, Color.red(base), Color.green(base), Color.blue(base))
        }
        // Same surface color as every ordinary key's own background
        // (roundedButton(tone(Color.rgb(52, 52, 54), Color.WHITE), ...) —
        // see buildEnglishCharKey()/keyboardKey()) — reusing it here ties
        // the selected pill into the keyboard's existing "raised key"
        // visual language instead of introducing a new floating-pill color.
        private val selectedPillColor = if (darkTheme) Color.rgb(52, 52, 54) else Color.WHITE
        private val iconColor = if (darkTheme) Color.rgb(240, 240, 240) else Color.rgb(50, 50, 54)
        // Unselected icons/labels dim slightly (not a color or hue change)
        // so the fully-opaque selected one reads as the one thing actively
        // "on" — a light touch, not the kind of visible recolor the spec
        // asks not to introduce.
        private val dimmedIconColor = Color.argb(178, Color.red(iconColor), Color.green(iconColor), Color.blue(iconColor))
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

        // Continuous [0, modes.size - 1] position of the selected pill, in
        // segment units — settles on the selected index, but slides through
        // fractional values mid-animation. This view is a fresh instance on
        // every panel rebuild (see the class doc above), so a real mode
        // switch is detected by comparing against lastRenderedMode (a
        // companion field, surviving across those rebuilds) rather than any
        // state this instance could hold itself; an unrelated rebuild that
        // leaves the mode unchanged just settles here with no animation.
        private var indicatorPosition: Float
        private var indicatorAnimator: android.animation.ValueAnimator? = null

        init {
            val modes = InputMode.entries
            val targetIndex = modes.indexOf(selectedMode).coerceAtLeast(0).toFloat()
            val previousMode = lastRenderedMode
            if (previousMode != null && previousMode != selectedMode) {
                val startIndex = modes.indexOf(previousMode).coerceAtLeast(0).toFloat()
                indicatorPosition = startIndex
                indicatorAnimator = android.animation.ValueAnimator.ofFloat(startIndex, targetIndex).apply {
                    duration = 180L
                    interpolator = android.view.animation.DecelerateInterpolator()
                    addUpdateListener {
                        indicatorPosition = it.animatedValue as Float
                        invalidate()
                    }
                    start()
                }
            } else {
                indicatorPosition = targetIndex
            }
            lastRenderedMode = selectedMode
        }

        override fun onDetachedFromWindow() {
            indicatorAnimator?.cancel()
            super.onDetachedFromWindow()
        }

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

            // Floating rounded pill instead of the old sharp, edge-to-edge
            // highlight: inset on every side so it reads as its own raised
            // surface hovering over the track rather than a panel-colored
            // cutout flush with the track's shape. Insets alone keep it
            // well inside the track's rounded corners even at the first/
            // last segment, so no extra clip path is needed the way the
            // sharp-cornered version required one.
            // Proportional again, not fixed dp — carries forward the exact
            // ratio the last explicit dp spec settled on (5dp inset / 38dp
            // track height, 53dp pill / 60dp segment width at the 240dp
            // outer track width in use then), so the pill keeps this same
            // look if the outer track's own width/height are ever tuned
            // again, instead of needing a matching manual dp recompute.
            val pillInsetV = h * (5f / 38f)
            val pillWidth = segmentWidth * (53f / 60f)
            val pillCenterX = segmentWidth * (indicatorPosition + 0.5f)
            val pillLeft = pillCenterX - pillWidth / 2f
            val pillRight = pillCenterX + pillWidth / 2f
            val pillTop = pillInsetV
            val pillBottom = h - pillInsetV
            paint.color = selectedPillColor
            canvas.drawRoundRect(pillLeft, pillTop, pillRight, pillBottom, (pillBottom - pillTop) / 2f, (pillBottom - pillTop) / 2f, paint)

            val selectedIndex = modes.indexOf(selectedMode).coerceAtLeast(0)
            fun colorFor(index: Int) = if (index == selectedIndex) iconColor else dimmedIconColor

            paint.color = colorFor(0)
            drawWaveform(canvas, segmentWidth * 0.5f, centerY, h)
            textPaint.color = colorFor(1)
            drawLabel(canvas, "笔画", segmentWidth * 1.5f, centerY, h * 0.38f, extraBold = true)
            paint.color = colorFor(2)
            drawCursorBrackets(canvas, segmentWidth * 2.5f, centerY, h)
            textPaint.color = colorFor(3)
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

        private companion object {
            // Survives across this view's per-rebuild instances (see the
            // class doc above) so a fresh instance can tell "the mode
            // actually changed since last drawn" from "this rebuild is for
            // some unrelated reason" and only animate the former.
            @Volatile
            private var lastRenderedMode: InputMode? = null
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
    internal class StrokeActionView(
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
        private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = iconColor
            style = Paint.Style.FILL
        }

        override fun onDraw(canvas: Canvas) {
            val u = minOf(width, height).coerceAtLeast(1) / 100f
            val cx = width / 2f
            val cy = height / 2f
            paint.strokeWidth = 5.2f * u
            when (actionCode) {
                "triangle-down" -> {
                    // Fixed absolute size, independent of this button's own
                    // (roughly 28x36dp, non-square) cell — about the same
                    // visual footprint as a single candidate glyph
                    // (candidateItemView's 20sp text), not scaled to the
                    // fixed-100-unit icon canvas the other cases share.
                    val triangleSize = 16f * resources.displayMetrics.density
                    val topY = cy - triangleSize / 2f
                    val path = Path().apply {
                        moveTo(cx - triangleSize / 2f, topY)
                        lineTo(cx + triangleSize / 2f, topY)
                        lineTo(cx, topY + triangleSize)
                        close()
                    }
                    canvas.drawPath(path, fillPaint)
                }
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

    /**
     * Two-line label (via a plain "\n") with a thin horizontal divider
     * drawn between the lines — the divider just needs the vertical
     * midpoint of the view, which lands between the two centered lines of
     * text closely enough without measuring actual text layout bounds.
     */
    /**
     * The one key-press preview bubble shared by every keyboard (English
     * and stroke) — a short, wide "floating keycap": a rounded rect with a
     * short stubby arrow, soft light-gray body and near-black text, no
     * accent color. Always sized/positioned by its owner (KeyPreviewOverlay)
     * rather than by itself; this class only draws. Plain Canvas drawing,
     * matching every other custom key glyph in this file (ShiftKeyView,
     * ActionSymbolView, etc.) — no new dependency.
     */
    private class KeyPreviewBubbleView(
        context: android.content.Context,
        darkTheme: Boolean,
    ) : View(context) {
        var label: String = ""
            set(value) {
                field = value
                invalidate()
            }
        // Non-null (the stroke keyboard's swipe-up digit preview) matches
        // the actual pressed key's own font size (TextView.getTextSize(),
        // already resolved to raw pixels, so no sp->px conversion is
        // needed here) instead of the default size derived from the
        // bubble's own height, which the English tap preview still relies on.
        var fixedTextSizePx: Float? = null
            set(value) {
                field = value
                invalidate()
            }
        private val bodyPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            // Lighter than the stroke encode/candidate row's own card
            // background in both themes (buildEncodeAreaBackground():
            // 58,58,58 dark / 228,228,232 light) rather than darker — a
            // darker-than-the-card bubble read as low-contrast/blended-in;
            // sitting a step lighter than that card makes the bubble read
            // as its own raised surface instead.
            color = if (darkTheme) Color.rgb(96, 96, 100) else Color.rgb(246, 246, 249)
        }
        private val borderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = 2f
            // Dark theme: a lighter outline than the body for definition
            // against a similarly-dark panel. Light theme: a slightly
            // darker outline than the now near-white body, since a lighter
            // outline there would have no contrast against anything.
            color = if (darkTheme) Color.rgb(128, 128, 133) else Color.rgb(210, 210, 215)
        }
        // Same red as the right-hand action rail's ←/↵/清除/123 keys and
        // the stroke candidate row's own selected/first candidate
        // (candidateItemView()), brightened for dark theme by the same
        // amount as candidateItemView()'s own dark-theme branch — the two
        // stay in sync since they're tuned together.
        private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = if (darkTheme) Color.rgb(190, 45, 60) else Color.rgb(153, 26, 40)
            textAlign = Paint.Align.CENTER
            typeface = android.graphics.Typeface.DEFAULT_BOLD
            // FILL_AND_STROKE over DEFAULT_BOLD's own fill adds a touch more
            // weight than the typeface alone — there's no heavier built-in
            // weight to switch to without bundling a custom font.
            style = Paint.Style.FILL_AND_STROKE
            strokeWidth = 1.4f
        }
        private val outline = Path()

        private fun dp(value: Int): Float = value * resources.displayMetrics.density

        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            val w = width.toFloat()
            val h = height.toFloat()
            if (w <= 0f || h <= 0f) return
            val arrowHeight = dp(ARROW_HEIGHT_DP)
            val arrowHalfWidth = dp(ARROW_WIDTH_DP) / 2f
            val bodyBottom = h - arrowHeight
            // Also capped by the flat run left on either side of the arrow
            // notch (w/2 - arrowHalfWidth) — at the narrowest bubble widths
            // this is tighter than bodyBottom/2, and skipping it would let
            // the bottom-corner arcs overrun the notch and self-intersect.
            val radius = dp(CORNER_RADIUS_DP)
                .coerceAtMost(bodyBottom / 2f)
                .coerceAtMost((w / 2f - arrowHalfWidth).coerceAtLeast(0f))
            val cx = w / 2f
            // One continuous outline (all four corners rounded to the same
            // radius, short triangular notch at the bottom center) so a
            // stroked border never shows a seam where the arrow meets the
            // body.
            outline.reset()
            outline.moveTo(radius, 0f)
            outline.lineTo(w - radius, 0f)
            outline.arcTo(android.graphics.RectF(w - 2f * radius, 0f, w, 2f * radius), -90f, 90f)
            outline.lineTo(w, bodyBottom - radius)
            outline.arcTo(android.graphics.RectF(w - 2f * radius, bodyBottom - 2f * radius, w, bodyBottom), 0f, 90f)
            outline.lineTo(cx + arrowHalfWidth, bodyBottom)
            outline.lineTo(cx, h)
            outline.lineTo(cx - arrowHalfWidth, bodyBottom)
            outline.lineTo(radius, bodyBottom)
            outline.arcTo(android.graphics.RectF(0f, bodyBottom - 2f * radius, 2f * radius, bodyBottom), 90f, 90f)
            outline.lineTo(0f, radius)
            outline.arcTo(android.graphics.RectF(0f, 0f, 2f * radius, 2f * radius), 180f, 90f)
            outline.close()
            canvas.drawPath(outline, bodyPaint)
            canvas.drawPath(outline, borderPaint)
            textPaint.textSize = fixedTextSizePx ?: (bodyBottom * 0.5f)
            val metrics = textPaint.fontMetrics
            val textY = bodyBottom / 2f - (metrics.ascent + metrics.descent) / 2f
            canvas.drawText(label, cx, textY, textPaint)
        }

        companion object {
            const val ARROW_HEIGHT_DP = 12
            const val ARROW_WIDTH_DP = 22
            const val CORNER_RADIUS_DP = 4
            const val BODY_HEIGHT_DP = 48
            const val TOTAL_HEIGHT_DP = BODY_HEIGHT_DP + ARROW_HEIGHT_DP
        }
    }

    /**
     * Single top-level overlay hosting the one KeyPreviewBubbleView shared
     * by every key in a panel (see wrapWithKeyPreviewOverlay()). Added as
     * the last (topmost-drawn) child of the FrameLayout that wraps a
     * panel's own root, so the bubble renders above the whole keyboard —
     * unaffected by the panel's own clipChildren/candidate-row/mode-toggle
     * bounds — without taking part in that panel's layout: it never
     * changes size, position, or measurement of anything else, and it
     * never intercepts touch (dispatchTouchEvent always returns false, so
     * every gesture falls straight through to whatever key is underneath).
     */
    private class KeyPreviewOverlay(
        context: android.content.Context,
        darkTheme: Boolean,
    ) : FrameLayout(context) {
        private val bubble = KeyPreviewBubbleView(context, darkTheme).apply {
            alpha = 0f
            visibility = View.GONE
        }

        init {
            isClickable = false
            isFocusable = false
            clipChildren = false
            clipToPadding = false
            addView(bubble, LayoutParams(0, 0))
        }

        // A pure visual layer: never claims a touch sequence, regardless of
        // where the bubble is currently drawn, so a stroke swipe or an
        // English drag-retarget already in progress is never disturbed by
        // the preview appearing on top of it.
        override fun onInterceptTouchEvent(ev: MotionEvent): Boolean = false
        override fun dispatchTouchEvent(ev: MotionEvent): Boolean = false

        private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

        private fun relativeLocation(target: View): IntArray {
            val targetLoc = IntArray(2)
            target.getLocationOnScreen(targetLoc)
            val selfLoc = IntArray(2)
            getLocationOnScreen(selfLoc)
            return intArrayOf(targetLoc[0] - selfLoc[0], targetLoc[1] - selfLoc[1])
        }

        private fun placeBubble(centerX: Int, top: Int, bubbleWidth: Int, bubbleHeight: Int) {
            val maxLeft = (width - bubbleWidth - dp(4)).coerceAtLeast(dp(4))
            val left = (centerX - bubbleWidth / 2).coerceIn(dp(4), maxLeft)
            val clampedTop = top.coerceAtLeast(dp(2))
            val params = bubble.layoutParams as LayoutParams
            params.width = bubbleWidth
            params.height = bubbleHeight
            params.leftMargin = left
            params.topMargin = clampedTop
            bubble.layoutParams = params
        }

        /**
         * TAP preview (English/number/symbol keys): centered directly above
         * the anchor key, sized relative to it, with the default
         * proportional-to-body text size (fixedTextSizePx left null).
         */
        fun showTapPreview(text: String, anchor: View) {
            val loc = relativeLocation(anchor)
            val bubbleWidth = (anchor.width * BUBBLE_WIDTH_RATIO).toInt().coerceAtLeast(dp(40))
            val bubbleHeight = dp(KeyPreviewBubbleView.TOTAL_HEIGHT_DP)
            bubble.fixedTextSizePx = null
            bubble.label = text
            val centerX = loc[0] + anchor.width / 2
            val top = loc[1] - bubbleHeight - dp(TAP_GAP_DP)
            placeBubble(centerX, top, bubbleWidth, bubbleHeight)
            show()
        }

        /**
         * SWIPE preview (stroke digit swipe-up): pinned to the actual
         * anchor key's own on-screen position, same as showTapPreview —
         * not to the finger's current height. It used to float a fixed
         * distance above the raw touch point instead, which looked right
         * for a straight vertical swipe on one key but drifted away from
         * the real key as soon as a drag retargeted sideways or across
         * rows (the bubble's X snapped to the new key while its Y stayed
         * tied to wherever the finger happened to be).
         *
         * Text size is a fixed constant here, not the tracked key's own
         * face text size — the "0" digit's key is the microphone icon key,
         * whose own face text is much smaller than every other digit key's
         * (10sp vs 17sp, since it's mostly an icon), so sizing off the key
         * made "0"'s bubble read noticeably smaller than the other nine.
         */
        fun showSwipePreview(text: String, anchor: View) {
            val loc = relativeLocation(anchor)
            val bubbleWidth = (anchor.width * BUBBLE_WIDTH_RATIO).toInt().coerceAtLeast(dp(40))
            val bubbleHeight = dp(KeyPreviewBubbleView.TOTAL_HEIGHT_DP)
            bubble.fixedTextSizePx = dp(SWIPE_TEXT_SIZE_DP).toFloat()
            bubble.label = text
            val centerX = loc[0] + anchor.width / 2
            // 20dp further above the key than the tap preview's own gap —
            // a separate constant so this doesn't also shift
            // showTapPreview's gap for the English/number keys.
            val top = loc[1] - bubbleHeight - dp(SWIPE_GAP_DP)
            placeBubble(centerX, top, bubbleWidth, bubbleHeight)
            show()
        }

        // No fade animation: a fast tap's down-to-up gap is often shorter
        // than any fade would take, so an animated show/hide left the
        // bubble stuck mid-fade — either never reaching full opacity
        // (looked "too transparent to read") or reversing before it ever
        // became visible at all. Popping straight to alpha=1 (matching the
        // pre-refactor PopupWindow, which never animated either)
        // guarantees the bubble is fully opaque the instant it shows,
        // regardless of tap speed.
        private fun show() {
            bubble.animate().cancel()
            bubble.alpha = 1f
            bubble.scaleX = 1f
            bubble.scaleY = 1f
            bubble.visibility = View.VISIBLE
        }

        fun hide() {
            bubble.animate().cancel()
            bubble.visibility = View.GONE
        }

        private companion object {
            const val BUBBLE_WIDTH_RATIO = 1.2f
            const val TAP_GAP_DP = 3
            const val SWIPE_GAP_DP = TAP_GAP_DP + 20
            // Bigger than the default proportional size the tap preview
            // falls back to (bodyBottom * 0.5, ~24dp-equivalent here) and,
            // being a flat constant rather than derived from any one key's
            // own face size, the same for every digit including "0".
            const val SWIPE_TEXT_SIZE_DP = 22
        }
    }

    private class MidDividerTextView(
        context: android.content.Context,
        private val dividerColor: Int,
    ) : TextView(context) {
        private val dividerPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { strokeWidth = 1.5f }

        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            dividerPaint.color = dividerColor
            val marginX = width * 0.22f
            canvas.drawLine(marginX, height / 2f, width - marginX, height / 2f, dividerPaint)
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
        // Panel's own dark-theme background is rgb(48,48,48); this reads
        // 12/255 lighter (tried 6 — too subtle to tell apart from the panel
        // — then 24, which read as too strong; 12 is the settled value).
        private val idlePillColor = if (darkTheme) Color.rgb(60, 60, 60) else Color.rgb(225, 225, 228)
        private val idleIconColor = if (darkTheme) Color.WHITE else Color.rgb(60, 60, 64)
        // Match the raw-mode recording indicator exactly, so the swipe-up
        // preview and the resulting raw recording state use one consistent
        // orange-yellow accent.
        private val swipeArmedPillColor = LINK_COLOR_RECORDING_RAW
        // Same idea, for the swipe-left-to-Note gesture's own idle-pill
        // preview — see armedForQuickNote/pillArmedAmountGreen below.
        private val quickNoteArmedPillColor = LINK_COLOR_QUICK_NOTE
        // Same idea again, for the swipe-right-to-Cloud-notes gesture's own
        // idle-pill preview — see armedForCloudNote/pillArmedAmountRed below.
        private val cloudNoteArmedPillColor = LINK_COLOR_CLOUD_NOTE
        // Light red, blended into the waveform bars while a swipe-down-to-
        // cancel gesture is past its commit threshold — same value in both
        // themes, mirroring swipeArmedPillColor's own choice.
        private val cancelArmedWaveformColor = Color.rgb(255, 150, 150)
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

        var rawModeActive: Boolean = false
            set(value) {
                field = value
                invalidate()
            }

        var quickNoteActive: Boolean = false
            set(value) {
                field = value
                invalidate()
            }

        var cloudNoteActive: Boolean = false
            set(value) {
                field = value
                invalidate()
            }

        var audioLevel: Float = 0f
            set(value) {
                field = value
                invalidate()
            }

        // Live "would releasing now enter raw mode?" indicator — the
        // setter only starts an ease toward the target (see
        // pillArmedAmount below); it doesn't jump straight there, so
        // rapid on/off toggling right at the dp(30) threshold doesn't
        // flicker instantly between the two colors.
        var armedForRawSwipe: Boolean = false
            set(value) {
                if (field == value) return
                field = value
                pillArmedAnimator?.cancel()
                pillArmedAnimator = android.animation.ValueAnimator.ofFloat(pillArmedAmount, if (value) 1f else 0f).apply {
                    duration = 120L
                    addUpdateListener { pillArmedAmount = it.animatedValue as Float }
                    start()
                }
            }
        private var pillArmedAmount: Float = 0f
            set(value) {
                field = value
                invalidate()
            }
        private var pillArmedAnimator: android.animation.ValueAnimator? = null

        // Same pattern as armedForRawSwipe/pillArmedAmount above, for the
        // swipe-left-to-Note idle-pill preview.
        var armedForQuickNote: Boolean = false
            set(value) {
                if (field == value) return
                field = value
                pillArmedAnimatorGreen?.cancel()
                pillArmedAnimatorGreen = android.animation.ValueAnimator.ofFloat(pillArmedAmountGreen, if (value) 1f else 0f).apply {
                    duration = 120L
                    addUpdateListener { pillArmedAmountGreen = it.animatedValue as Float }
                    start()
                }
            }
        private var pillArmedAmountGreen: Float = 0f
            set(value) {
                field = value
                invalidate()
            }
        private var pillArmedAnimatorGreen: android.animation.ValueAnimator? = null

        // Same pattern again, for the swipe-right-to-Cloud-notes idle-pill
        // preview.
        var armedForCloudNote: Boolean = false
            set(value) {
                if (field == value) return
                field = value
                pillArmedAnimatorRed?.cancel()
                pillArmedAnimatorRed = android.animation.ValueAnimator.ofFloat(pillArmedAmountRed, if (value) 1f else 0f).apply {
                    duration = 120L
                    addUpdateListener { pillArmedAmountRed = it.animatedValue as Float }
                    start()
                }
            }
        private var pillArmedAmountRed: Float = 0f
            set(value) {
                field = value
                invalidate()
            }
        private var pillArmedAnimatorRed: android.animation.ValueAnimator? = null

        // Same pattern as armedForRawSwipe/pillArmedAmount above, applied
        // to the recording waveform instead of the idle capsule — swipe
        // DOWN past the threshold while recording tints the waveform bars
        // light red instead of the capsule turning green.
        var armedForCancel: Boolean = false
            set(value) {
                if (field == value) return
                field = value
                waveformCancelAnimator?.cancel()
                waveformCancelAnimator = android.animation.ValueAnimator.ofFloat(waveformCancelAmount, if (value) 1f else 0f).apply {
                    duration = 120L
                    addUpdateListener { waveformCancelAmount = it.animatedValue as Float }
                    start()
                }
            }
        private var waveformCancelAmount: Float = 0f
            set(value) {
                field = value
                invalidate()
            }
        private var waveformCancelAnimator: android.animation.ValueAnimator? = null

        private fun lerpColor(from: Int, to: Int, amount: Float): Int {
            val t = amount.coerceIn(0f, 1f)
            return Color.rgb(
                (Color.red(from) + (Color.red(to) - Color.red(from)) * t).toInt(),
                (Color.green(from) + (Color.green(to) - Color.green(from)) * t).toInt(),
                (Color.blue(from) + (Color.blue(to) - Color.blue(from)) * t).toInt(),
            )
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
            pillArmedAnimator?.cancel()
            pillArmedAnimatorGreen?.cancel()
            pillArmedAnimatorRed?.cancel()
            waveformCancelAnimator?.cancel()
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
                val pillColor = lerpColor(
                    lerpColor(
                        lerpColor(idlePillColor, swipeArmedPillColor, pillArmedAmount),
                        quickNoteArmedPillColor,
                        pillArmedAmountGreen,
                    ),
                    cloudNoteArmedPillColor,
                    pillArmedAmountRed,
                )
                paint.color = pillColor
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
                // The normal waveform is monochrome; RAW recording uses the
                // same orange-yellow accent as the RAW indicator and the
                // swipe-up preview. Quiet input stays compact while speech
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
                val rawWaveformColor = when {
                    rawModeActive -> LINK_COLOR_RECORDING_RAW
                    quickNoteActive -> LINK_COLOR_QUICK_NOTE
                    cloudNoteActive -> LINK_COLOR_CLOUD_NOTE
                    else -> waveformColor
                }
                val currentWaveformColor = lerpColor(rawWaveformColor, cancelArmedWaveformColor, waveformCancelAmount)
                for (index in 0 until barCount) {
                    val x = startX + index * gap
                    val distanceFromCenter = kotlin.math.abs(index - envelopeCenter) / envelopeCenter
                    val envelope = 1f - distanceFromCenter * distanceFromCenter * 0.75f
                    val flow = 0.55f + 0.45f * kotlin.math.sin(
                        (phase * 2.2f + index * 0.9f).toDouble(),
                    ).toFloat()
                    val halfHeight = minOf(height * 0.95f, dp(66).toFloat()) *
                        (0.035f + live * 0.965f) * envelope * flow
                    paint.color = currentWaveformColor
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
        const val PREF_KEYBOARD_HEIGHT_DP = "keyboard_height_dp"
        const val DEFAULT_KEYBOARD_HEIGHT_DP = 300
        const val MIN_KEYBOARD_HEIGHT_DP = 220
        const val MAX_KEYBOARD_HEIGHT_DP = 420
        const val PREF_KEYBOARD_RAISE_DP = "keyboard_raise_dp"
        const val DEFAULT_KEYBOARD_RAISE_DP = 0
        const val MIN_KEYBOARD_RAISE_DP = 0
        const val MAX_KEYBOARD_RAISE_DP = 120

        fun panelHeightDp(context: android.content.Context): Int {
            return context.getSharedPreferences("openless_ime_ui", android.content.Context.MODE_PRIVATE)
                .getInt(PREF_KEYBOARD_HEIGHT_DP, DEFAULT_KEYBOARD_HEIGHT_DP)
                .coerceIn(MIN_KEYBOARD_HEIGHT_DP, MAX_KEYBOARD_HEIGHT_DP)
        }

        fun panelHeightPx(context: android.content.Context): Int {
            return (panelHeightDp(context) * context.resources.displayMetrics.density).toInt()
        }

        fun raiseHeightDp(context: android.content.Context): Int {
            return context.getSharedPreferences("openless_ime_ui", android.content.Context.MODE_PRIVATE)
                .getInt(PREF_KEYBOARD_RAISE_DP, DEFAULT_KEYBOARD_RAISE_DP)
                .coerceIn(MIN_KEYBOARD_RAISE_DP, MAX_KEYBOARD_RAISE_DP)
        }

        fun raiseHeightPx(context: android.content.Context): Int {
            return (raiseHeightDp(context) * context.resources.displayMetrics.density).toInt()
        }

        private const val SILENCE_LEVEL_THRESHOLD = 0.02f
        private const val SILENCE_CHECK_DELAY_MS = 3000L
        private const val BACKEND_HEARTBEAT_INTERVAL_MS = 6000L
        // How long a transient confirmation (已上屏/已完成/已撤销) stays up
        // before settling back to "点击开始说话" — see scheduleRevertToIdle().
        private const val DONE_TO_IDLE_DELAY_MS = 2000L
        // quickNoteDictation()'s own "笔记已记录" confirmation — shorter
        // than DONE_TO_IDLE_DELAY_MS per product request, since there's no
        // undo/redo/edit row to give the user time to read (quick note
        // never inserts anything).
        private const val QUICK_NOTE_CONFIRMATION_DELAY_MS = 1000L
        // Not a hard "show exactly N" cap — the candidate bar is a
        // HorizontalScrollView (see buildEnglishCandidateBar()), so this
        // just bounds how many the provider bothers ranking/returning per
        // keystroke; comfortably more than can fit on screen at once so
        // scrolling actually reveals more real options.
        private const val ENGLISH_CANDIDATE_QUERY_LIMIT = 10
        // buildClipboardView()'s "recent clips" strip. Originally 92dp for
        // 4 equal-weight rows + 5 divider lines (dp(1) each), raised ~1.2x
        // to 109dp per user request: (92-5)/4=21.75dp/row * 1.2 = 26.1,
        // 26.1*4+5=109.4 -> 109. Then reduced from 4 rows to 3 (also per
        // user request) WITHOUT changing this total — 3 rows only need 4
        // dividers, not 5, so the freed divider's dp plus the freed row's
        // own share both land back on the remaining 3: (109-4)/3=35dp/row,
        // up from 26dp. The button grid below needs no matching change —
        // it's already LinearLayout.LayoutParams(.., 0, 1f), i.e. "whatever
        // space is left after this strip's fixed height," so it grows or
        // shrinks on its own the moment this constant or the row count
        // changes.
        private const val RECENT_CLIPS_COLUMN_HEIGHT_DP = 109
        // Swipe-up symbol hint, any LETTERS-layer row (buildEnglishCharKey())
        // — its own independent TextView pinned to the key's top edge, so
        // these are absolute sizes, not a ratio of the 22sp letter below
        // it. Small and close to the top by design; tune these two numbers
        // directly. Shared by row1's digits and row2/row3's punctuation so
        // all three rows read as one consistent hint style.
        private const val SWIPE_SYMBOL_HINT_TEXT_SIZE_SP = 11f
        private const val SWIPE_SYMBOL_HINT_TOP_PADDING_DP = 2
        // Pure render-time offset (View.translationY), not part of the
        // letter TextView's own measured layout — see buildEnglishCharKey().
        private const val LETTER_VERTICAL_NUDGE_DP = 2
        // Same hue family as OpenLessOverlayService's OverlayVisualState
        // (recording/processing), plus a light-green "ready" and an amber
        // "link issue" that overlay doesn't have.
        private val LINK_COLOR_READY = Color.rgb(134, 239, 172)
        private val LINK_COLOR_RECORDING = Color.rgb(244, 63, 94)
        // Recording with rawModeArmed set (mic swipe-up: skip LLM polish on
        // stop) — distinct orange so glancing at the dot tells the two
        // recording states apart without reading any text.
        private val LINK_COLOR_RECORDING_RAW = Color.rgb(249, 115, 22)
        // Recording with quickNoteArmed set (mic swipe-left: end the
        // dictation and archive it as a standalone note instead of
        // inserting) — green, the same treatment LINK_COLOR_RECORDING_RAW
        // gets for its own swipe-up gesture.
        private val LINK_COLOR_QUICK_NOTE = Color.rgb(34, 197, 94)
        // Recording with cloudNoteArmed set (mic swipe-right: end the dictation
        // and POST the verbatim transcript to the configured webhook
        // instead of inserting anything) — red, the third accent alongside
        // LINK_COLOR_RECORDING_RAW's orange and LINK_COLOR_QUICK_NOTE's
        // green.
        private val LINK_COLOR_CLOUD_NOTE = Color.rgb(239, 68, 68)
        private val LINK_COLOR_PROCESSING = Color.rgb(56, 189, 248)
        private val LINK_COLOR_ISSUE = Color.rgb(250, 204, 21)
        // Ready is the state the indicator sits in almost all the time, so
        // its pulse is slower/gentler ("breathing") than the other, more
        // urgent states — which keep the original brisker cadence.
        private const val BACKEND_LINK_PULSE_DURATION_MS = 900L
        private const val BACKEND_LINK_READY_PULSE_DURATION_MS = 1800L

        @Volatile
        private var lastHeartbeatElapsedRealtime = 0L
        @Volatile
        private var lastHeartbeatReady: Boolean? = null

        /** Runtime/heartbeat state shown in native keyboard settings for diagnosis. */
        fun backendDebugSnapshot(): String {
            val service = activeInstance?.get()
                ?: return "ime=none heartbeat=none"
            val heartbeat = lastHeartbeatReady?.let { if (it) "ready" else "not-ready" } ?: "none"
            val ageMs = if (lastHeartbeatElapsedRealtime == 0L) {
                -1L
            } else {
                (android.os.SystemClock.elapsedRealtime() - lastHeartbeatElapsedRealtime).coerceAtLeast(0L)
            }
            return "ime=present recording=${service.recording} processing=${service.processing} " +
                "backend=${heartbeat} ageMs=$ageMs indicator=${service.backendLinkIndicator != null}"
        }

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

        /**
         * Whether the input panel is actually on screen right now — checked by
         * OpenLessBackendWarmupActivity.launchWarmup() right before it steals
         * the foreground, so it knows afterward whether restoring the panel is
         * even meaningful (see requestInputPanelAfterWarmup()'s doc comment).
         */
        fun isInputPanelCurrentlyShown(): Boolean {
            val service = activeInstance?.get() ?: return false
            return service.isInputViewShown
        }

        /**
         * Re-open the IME after the one-time backend Activity gives focus
         * back — but only when there is still a real, keyboard-wanting field
         * focused at fire time. Without this check, a warmup silently
         * triggered by an unrelated onStartInput() (e.g. Camera/Dialer
         * momentarily focusing their own non-text-entry views) or by the
         * periodic backend heartbeat could force the panel open over
         * whatever app the user has since switched to — up to ~500ms after
         * the original trigger (120ms launch delay + 180ms self-background +
         * this call's own delay), plenty of time to have left the field, the
         * app, or even backgrounded to the home screen entirely. Callers are
         * additionally expected to only invoke this at all when the panel
         * was visible right before the warmup Activity took focus (see
         * isInputPanelCurrentlyShown()) — this is the second, independent
         * check against the state at the moment of firing.
         */
        fun requestInputPanelAfterWarmup(delayMs: Long = 260L) {
            val service = activeInstance?.get() ?: return
            android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
                if (activeInstance?.get() === service) {
                    val attribute = service.currentInputEditorInfo
                    if (attribute != null && attribute.inputType != InputType.TYPE_NULL) {
                        service.requestShowSelf(InputMethodManager.SHOW_IMPLICIT)
                    }
                }
            }, delayMs)
        }
    }
}
