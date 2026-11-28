package com.openless.app

import android.Manifest
import android.content.Context
import android.content.res.Configuration
import android.content.pm.PackageManager
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
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
    // routes letters into RimePinyinController's own encoding buffer and
    // only ever commits a chosen candidate. Long-press space toggles this
    // (see buildKeyboardView()'s spaceButton); the panel/View tree itself
    // is never rebuilt into a second layout for PINYIN — see
    // docs/pinyin-lite/phase-0-audit.md section 7.
    internal enum class LatinInputMode {
        ENGLISH,
        PINYIN;

        fun toggled(): LatinInputMode = if (this == ENGLISH) PINYIN else ENGLISH
    }

    private var lastCancelledImeRequest = 0L
    private var imeRequestId = 0L
    private var imeSessionId: String? = null
    private var typingVoiceOwner: ImeRequestOwner? = null
    private var activeImeOwner: ImeRequestOwner? = null
    private val imeFrameGate = ImeScopedFrameGate()
    private var externalVoiceSessionId: String? = null
    private var imeStopRequested = false
    private var imeCaptureEnded = false
    private var externalRecording = false
    private var externalProcessing = false
    private var microphoneGranted = false
    private var pendingCloudArm = false
    private var pendingImeStop = false
    private var cloudNoteDestination: Pair<String, String>? = null

    private fun sendImeCommand(action: String, cursorContext: AndroidCursorContext? = null): Boolean = try {
        val request = org.json.JSONObject().apply {
            put("action", action)
            put("requestId", imeRequestId)
            imeSessionId?.let { put("sessionId", it) }
            put("raw", rawModeArmed)
            put("quickNote", quickNoteArmed)
            put("cloud", cloudNoteArmed)
            // Core builds the prompt envelope; only the session's "start" carries the snapshot.
            if (action == "start" && cursorContext != null) {
                cursorContext.packageName?.let { put("frontApp", it) }
                put("cursorBefore", cursorContext.before)
                put("cursorAfter", cursorContext.after)
            }
        }
        val response = org.json.JSONObject(OpenLessNative.nativeImeCommand(request.toString()))
        check(response.optBoolean("ok")) { response.optString("error", "IME command failed") }
        true
    } catch (_: Throwable) {
        setState("error", ui("语音操作失败，请重试", "Voice operation failed; please retry"))
        false
    }

    /** Snapshot of the text around the caret for this recording; must run when recording starts, never at stop. Logs metadata only. */
    private fun captureCursorContext(): AndroidCursorContext? {
        if (!OpenLessAndroidPreferences.cursorContextEnabled(this)) return null
        val started = android.os.SystemClock.elapsedRealtime()
        val editor = currentInputEditorInfo
        val connection = currentInputConnection
        val captured = if (editor == null || connection == null) null else ImePrivacyPolicy.captureCursorContext(
            enabled = true, inputType = editor.inputType, imeOptions = editor.imeOptions, packageName = editor.packageName,
            readBefore = { connection.getTextBeforeCursor(it, 0) }, readAfter = { connection.getTextAfterCursor(it, 0) })
        android.util.Log.i("OpenLessImeService", "cursor-context android status=${if (captured == null) "none" else "ok"} " +
            "source=input_connection before_chars=${captured?.before?.length ?: 0} after_chars=${captured?.after?.length ?: 0} " +
            "package=${editor?.packageName} elapsed_ms=${android.os.SystemClock.elapsedRealtime() - started}")
        return captured
    }

    private fun cancelImeSession() {
        if (imeRequestId != 0L) { lastCancelledImeRequest = imeRequestId; sendImeCommand("cancel") }
        imeRequestId = 0L
        imeSessionId = null
        typingVoiceOwner = null
        imeFrameGate.finish(activeImeOwner)
        activeImeOwner = null
        imeStopRequested = false
        imeCaptureEnded = false
        recording = false
        processing = false
        pendingImeStop = false
        cloudNoteDestination = null
        pendingCloudArm = false
        cloudNoteSubmitting = false
        renderVoiceUi()
    }

    private fun stopImeSession() {
        imeStopRequested = true
        imeCaptureEnded = true
        if (imeSessionId == null) pendingImeStop = true
        else if (!sendImeCommand("stop")) { recording = false; processing = false }
    }

    private fun onImeSessionEvent(json: String) {
        val event = runCatching { org.json.JSONObject(json) }.getOrNull() ?: return
        if (event.optLong("requestId") != imeRequestId || imeRequestId == 0L) {
            event.optString("sessionId").takeIf { it.isNotBlank() }?.let(imeFrameGate::retire)
            if (event.optLong("requestId") == lastCancelledImeRequest && event.optString("error") == "Persistence") {
                OpenLessNativeNotice.makeText(this, ui("录音清理失败，重新打开应用后重试", "Recording cleanup failed; reopen the app to retry"), Toast.LENGTH_LONG).show()
            }
            return
        }
        when (event.optString("kind")) {
            "started" -> {
                val identity = event.optString("sessionId")
                val owner = activeImeOwner
                if (owner == null || !owner.matches(sessionEpoch, imeRequestId)) {
                    if (identity.isNotBlank()) imeFrameGate.retire(identity)
                    return
                }
                if (imeSessionId == identity && imeFrameGate.isBound(owner, identity)) {
                    // Replayed acknowledgements cannot reapply cloud mode or
                    // resubmit stop after finalization has already begun.
                    updateStatus(currentMessage)
                    return
                }
                val buffered = imeFrameGate.bind(owner, identity)
                if (!imeFrameGate.isBound(owner, identity)) {
                    cancelImeSession()
                    setState("error", ui("无法确认听写会话，请重试", "Could not identify the dictation session; retry"))
                    return
                }
                imeSessionId = identity
                if (!ImeVoiceFramePolicy.canApplyQueuedStartActions(buffered?.state)) {
                    // The backend already stopped/failed before this ack. Stop is
                    // not idempotent: never send another stop just to obtain text.
                    pendingImeStop = false
                    pendingCloudArm = false
                    imeStopRequested = false
                    buffered?.let { onCapsuleStateChangedWithSession(it.state, it.message, it.level, it.sessionId) }
                    updateStatus(currentMessage)
                    return
                }
                if (pendingCloudArm) { pendingCloudArm = false; cloudNoteArmed = true }
                if (cloudNoteArmed && !sendImeCommand("cloud")) { cancelImeSession(); return }
                if (pendingImeStop) { pendingImeStop = false; stopImeSession() }
                buffered?.let { onCapsuleStateChangedWithSession(it.state, it.message, it.level, it.sessionId) }
                updateStatus(currentMessage)
            }
            "completed" -> {
                if (!ownsImeRequest() || event.optString("sessionId") != imeSessionId) return
                val destination = cloudNoteDestination
                val reviewCloudNote = cloudNoteReviewArmed
                typingVoiceOwner = null
                imeFrameGate.finish(activeImeOwner)
                activeImeOwner = null
                imeStopRequested = false
                imeCaptureEnded = false
                imeSessionId = null
                recording = false
                processing = false
                cloudNoteArmed = false
                if (event.optBoolean("cloud")) {
                    if (reviewCloudNote) openCloudNoteDraft(event.optString("text"), destination)
                    else submitCloudNoteText(event.optString("text"), destination, imeRequestId)
                }
                else if (event.optBoolean("quickNote")) setState("done", ui("笔记已记录", "Note saved"))
                else commitImeText(event.optString("text"))
            }
            "error" -> {
                typingVoiceOwner = null
                imeFrameGate.finish(activeImeOwner)
                activeImeOwner = null
                imeStopRequested = false
                imeCaptureEnded = false
                recording = false
                processing = false
                imeSessionId = null
                pendingImeStop = false
                pendingCloudArm = false
                if (event.optString("error") == "Cancelled") setState("cancelled", ui("已取消", "Cancelled"))
                else setState("error", ui("听写失败，请重试", "Dictation failed; please retry"))
            }
        }
    }

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
            if (value) pendingCloudArm = false
            if (value && cloudNoteArmed) { cloudNoteArmed = false; if (cloudNoteArmed) return }
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
            if (value) pendingCloudArm = false
            if (value && cloudNoteArmed) { cloudNoteArmed = false; if (cloudNoteArmed) return }
            field = value
            voiceButton?.quickNoteActive = value
            status?.setTextColor(recordingAccentColor())
        }
    // The Core session owns retention; this field only displays the accepted destination.
    private var cloudNoteArmed = false
        set(value) {
            if (!value) {
                pendingCloudArm = false
                cloudNoteReviewArmed = false
            }
            if (value && imeRequestId != 0L && imeSessionId == null) {
                pendingCloudArm = true
                updateStatus(ui("正在准备云笔记", "Preparing cloud note"))
                return
            }
            if (field == value) return
            val previous = field
            field = value
            if (imeSessionId != null && !sendImeCommand("cloud")) { field = previous; return }
            if (value) {
                rawModeArmed = false
                quickNoteArmed = false
                val prefs = getSharedPreferences("openless_ime_ui", MODE_PRIVATE)
                cloudNoteDestination = (prefs.getString("key_cloud_note_webhook_url", "") ?: "").trim() to
                    (prefs.getString("key_cloud_note_webhook_token", "") ?: "").trim()
            }
            voiceButton?.cloudNoteActive = value || cloudNoteSubmitting
            status?.setTextColor(recordingAccentColor())
        }
    // Set by a swipe up on the mic while a Cloud notes recording is in
    // progress, which also ends that recording: the recognized text then
    // opens in the draft bar for editing (see openCloudNoteDraft()) instead
    // of going straight to the webhook, as it does after a plain tap.
    // Only ever true alongside cloudNoteArmed, whose setter clears it.
    private var cloudNoteReviewArmed = false
    // The webhook POST that follows a Cloud notes dictation (see
    // submitCloudNoteText()). cloudNoteArmed is already back to false by
    // then — the Core session it mirrors has completed — so this is what
    // keeps the red accent up until the submit itself finishes. Display
    // only: unlike `processing`, it never blocks a new mic tap.
    private var cloudNoteSubmitting = false

    /** Accent of whichever gesture mode is armed — orange for Raw, green for Quick notes, red for Cloud notes (including its submit step) — or null for an ordinary dictation. Single source of truth for the status line, the hint row and the link indicator, so they can never disagree on which mode currently owns the color. */
    private fun armedAccentColor(): Int? = when {
        rawModeArmed -> LINK_COLOR_RECORDING_RAW
        quickNoteArmed -> LINK_COLOR_QUICK_NOTE
        cloudNoteArmed || cloudNoteSubmitting -> LINK_COLOR_CLOUD_NOTE
        else -> null
    }

    /** True from the start of a recording through the "thinking" step that follows it — the span an armed mode keeps its accent for. */
    private fun dictationInProgress(): Boolean = voiceUiState().editorLocked

    /** Status text color while a recording or thinking prompt is showing — the armed mode's accent, normal otherwise. */
    private fun recordingAccentColor(): Int =
        if (dictationInProgress()) armedAccentColor() ?: statusNormalColor else statusNormalColor
    internal var inputMode = InputMode.VOICE
    private var englishLayer = EnglishLayer.LETTERS
    // Persisted across sessions the same way inputMode is (see
    // restoreLatinInputMode()/saveLatinInputMode()) — long-pressing space
    // toggles it (see buildKeyboardView()'s spaceButton). The space key's
    // own small hint label always names the mode long-press would switch
    // TO, not the current one — i.e. it reads "拼音" while latinInputMode
    // is ENGLISH, and "英文" while it's PINYIN.
    private var latinInputMode = LatinInputMode.ENGLISH
    private val rimePinyinController by lazy { RimePinyinController(this) }
    private var pinyinComposingActive = false
    private var quickChipPayload: String? = null
    private var quickChipLabel: String? = null
    private var quickChipExpiresAtMs: Long = 0L
    private var quickChipIsUndoDelete: Boolean = false
    // The word currently being typed on the English keyboard — appended to
    // per letter, trimmed per backspace, cleared at every word boundary
    // (space/return/punctuation/candidate tap/mode or panel switch). Never
    // touches currentInputConnection itself: every letter is still
    // committed immediately, same as before this feature: this buffer only
    // tracks what to re-query/replace for candidates.
    private val englishComposingWord = StringBuilder()
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
    // first time an arrow key is pressed after the "选择" (Select) key turns
    // on; cleared
    // whenever selection mode turns off so the next selection starts fresh
    // from wherever the cursor happens to be then.
    private var clipboardSelectionAnchor = -1
    private var clipboardSelectionActive = -1
    private var clipboardHistoryMode = false
    private var clipboardHistoryCategory = OpenLessClipboardHistory.Category.ALL
    private var shiftState = ShiftState.OFF
    // SHIFT_ONCE that updateAutoShift() turned on for a sentence start, as
    // opposed to the user's own Shift tap — only this kind is turned back off
    // when the caret leaves the sentence start.
    private var autoShift = false
    // The space selectEnglishCandidate() appended, still right before the
    // caret: sentence punctuation typed next takes its place ("word, ").
    private data class EnglishEditAnchor(val connection: android.view.inputmethod.InputConnection, val offset: Int)
    private var englishAutoSpace: EnglishEditAnchor? = null
    private var lastEnglishSpace: EnglishEditAnchor? = null
    private var lastEnglishSpaceAt = 0L
    private var state = "idle"
    private var currentMessage = "点击开始说话"
    private var latestVoiceLevel = 0f
    private var status: TextView? = null
    private var voiceRawHint: TextView? = null
    private var statusNormalColor = Color.GRAY
    // Last height_dp:raise_dp baked into the cached input view — compared in
    // onStartInputView so leaving keyboard settings and refocusing rebuilds
    // when the footprint prefs changed.
    private var appliedKeyboardFootprintKey: String? = null
    private var surfaceHost: ImeSurfaceHost? = null
    private var keyboardSurface: View? = null
    private var draftBar: ImeDraftBar? = null
    private var voiceButton: ImeVoiceButton? = null
    private var voicePanel: ImeVoicePanel? = null
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
    private var keyboardHeader: ImeKeyboardHeader? = null
    // Which recognizer the next dictation uses, as nativeAsrRoute() reports
    // it ("cloud", "local", "localRaw", "unavailable"). The idle dot shows
    // local in dark green and pure-local raw output (no usable polish
    // channel, or no network to reach it) in light white. Refreshed off the
    // main thread by refreshAsrRoute().
    private var asrRoute = "cloud"
    private var networkAvailable = true
    private val asrRouteRefreshing = java.util.concurrent.atomic.AtomicBoolean(false)
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
    internal fun ui(zh: String, en: String) = if (englishUi) en else zh

    internal fun keyboardPalette(): ImePalette = ImePalette.resolve(isDarkTheme)
    internal fun interactionPolicy(): ImeInteractionPolicy = ImeInteractionPolicy.fromStoredRepeat(keyRepeatIntervalMs())
    internal fun emitKeyFeedback() { performKeyHaptic() }

    private fun refreshInteractionPolicy() {
        val policy = interactionPolicy()
        typingPanel?.applyInteractionPolicy(policy)
        voicePanel?.applyInteractionPolicy(policy)
        clipboardPanel?.applyInteractionPolicy(policy)
        strokeController.applyInteractionPolicy(policy)
    }

    internal fun isKeyboardEditingBlocked(): Boolean = dictationEditingLocked()
    internal fun buildKeyboardHeader(): View = buildVoiceHeader()
    internal fun isEnglishKeyboardUi(): Boolean = englishUi


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
                    val align = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                        android.text.style.DynamicDrawableSpan.ALIGN_CENTER
                    } else {
                        android.text.style.ImageSpan.ALIGN_BOTTOM
                    }
                    builder.setSpan(
                        android.text.style.ImageSpan(strokeFifthGlyphDrawable(), align),
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
        val sizePx = (14f * resources.displayMetrics.scaledDensity).toInt().coerceAtLeast(dp(12))
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
        val stored = getSharedPreferences("openless_ime_ui", MODE_PRIVATE).getString("latin_input_mode", null)
        latinInputMode = when (stored) {
            "pinyin" -> LatinInputMode.PINYIN
            "english" -> LatinInputMode.ENGLISH
            else -> if (englishUi) LatinInputMode.ENGLISH else LatinInputMode.PINYIN
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
        englishCandidateQueryEpoch++
        clearPinyinComposingInEditor()
        rimePinyinController.clear()
        latinInputMode = latinInputMode.toggled()
        saveLatinInputMode(latinInputMode)
        clearEnglishAutoText()
        refreshLatinCandidateBar()
        performKeyHaptic()
        updateAutoShift(refresh = false)
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
            rimePinyinController.refresh { renderPinyinCandidates(it) }
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
            offerClipboardQuickChip(text)
        }
    }

    private var learningMigrationComplete = false
    internal fun personalizedLearningAllowed(): Boolean {
        val editor = currentInputEditorInfo ?: return false
        return learningMigrationComplete && ImeLearningPolicy.allowsLearning(editor.inputType, editor.imeOptions)
    }

    override fun onCreate() {
        super.onCreate()
        microphoneGranted = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        learningMigrationComplete = ImeLearningPolicy.migrate(this)
        refreshLanguage()
        restoreInputMode()
        restoreScriptPreference()
        restoreLatinInputMode()
        activeInstance = java.lang.ref.WeakReference(this)
        OpenLessOverlayBridge.imeListener = this
        OpenLessOverlayBridge.imeSessionListener = ::onImeSessionEvent
        startRuntimeService()
        // Pre-warm offline stroke, phrase association, and Rime pinyin engines off the UI thread.
        strokeController.preloadAsync()
        rimePinyinController.preload()
        clipboardManager.addPrimaryClipChangedListener(clipboardHistoryListener)
        backendHeartbeatHandler.post(backendHeartbeatRunnable)
    }

    override fun onDestroy() {
        draftBar?.cancelInteractions()
        cancelImeSession()
        if (activeInstance?.get() === this) {
            activeInstance = null
        }
        if (OpenLessOverlayBridge.imeListener === this) {
            OpenLessOverlayBridge.imeListener = null
            OpenLessOverlayBridge.imeSessionListener = null
        }
        backendHeartbeatHandler.removeCallbacks(backendHeartbeatRunnable)
        keyboardHeader?.cancelInteractions()
        clipboardManager.removePrimaryClipChangedListener(clipboardHistoryListener)
        stopRuntimeService()
        strokeController.shutdown()
        if (englishCandidateProviderLazy.isInitialized()) englishCandidateProviderLazy.value.shutdown()
        rimePinyinController.shutdown()
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
                    setBackgroundColor(keyboardPalette().surface)
                    visibility = if (raisePx > 0) View.VISIBLE else View.GONE
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, raisePx),
            )
        }
    }

    override fun onCreateInputView(): View {
        val host = ImeSurfaceHost(this)
        surfaceHost = host
        rebuildSurfaceContent()
        return host.also { excludeFromSystemGestures(it) }
    }

    private fun rebuildSurfaceContent(): View {
        draftBar?.cancelInteractions()
        draftBar = null
        typingPanel?.closeTransientUi()
        clipboardPanel?.cancelInteractions()
        strokeController.cancelInteractions()
        keyboardHeader?.cancelInteractions()
        keyboardHeader = null
        clipboardPanel = null
        typingPanel = null
        voicePanel = null
        voiceButton = null
        status = null
        voiceRawHint = null
        voiceLinkWarning = null
        dictationResultRow = null
        undoRedoButton = null
        editResultButton = null
        refreshLanguage()
        val content = when {
            inputMode == InputMode.ENGLISH -> buildKeyboardView()
            inputMode == InputMode.STROKE -> strokeController.buildStrokeView()
            inputMode == InputMode.CLIPBOARD ->
                if (clipboardHistoryMode) buildClipboardHistoryView() else buildClipboardView()
            else -> buildVoiceInputView()
        }
        val keyboard = applyKeyboardRaise(content)
        keyboardSurface = keyboard
        val draft = buildDraftBar()
        surfaceHost?.render(keyboard, draft, draftEdit)
        appliedKeyboardFootprintKey = keyboardFootprintKey()
        return keyboard
    }

    private fun replaceSurfaceContent(): View {
        if (surfaceHost == null) {
            setInputView(onCreateInputView())
            return requireNotNull(keyboardSurface)
        }
        return rebuildSurfaceContent()
    }

    /** View components render state; this adapter alone owns native operations. */
    private fun buildVoiceInputView(): View {
        val palette = ImePalette.resolve(isDarkTheme)
        return ImeVoicePanel(
            this, palette, buildVoiceHeader(), englishUi,
            onVoiceGesture = ::handleVoiceGesture,
            onUndo = ::toggleUndoRedoDictation,
            onEdit = { openDictationEditDraft(requireSelection = false) },
            onEnter = { if (!dictationEditingLocked()) sendEnterKey() },
            onBackspace = { if (!dictationEditingLocked()) deleteBackward() },
            onAt = { if (!dictationEditingLocked()) currentInputConnection?.commitText("@", 1) },
            onRestart = ::restartApp,
            onFeedback = ::performKeyHaptic,
            interactionPolicy = interactionPolicy(),
        ).also { panel ->
            voicePanel = panel
            statusNormalColor = palette.secondaryText
            status = panel.status
            voiceButton = panel.voiceButton
            voiceRawHint = panel.modeHint
            voiceLinkWarning = panel.warning
            undoRedoButton = panel.undo
            editResultButton = panel.edit
            dictationResultRow = panel.resultRow
            updateStatus(currentMessage)
        }
    }

    private fun dictationEditingLocked(): Boolean = voiceUiState().editorLocked

    private fun ownsImeRequest(): Boolean = activeImeOwner?.matches(sessionEpoch, imeRequestId) == true
    private fun hasForeignVoiceActivity(): Boolean = (externalRecording || externalProcessing) && !ownsImeRequest()

    private fun voiceUiState(): ImeVoiceUiSnapshot {
        val foreign = hasForeignVoiceActivity()
        return ImeVoiceUiProjection.fromLegacy(
            state, recording || externalRecording, processing || externalProcessing, cloudNoteSubmitting,
            rawModeArmed && !foreign, quickNoteArmed && !foreign, (cloudNoteArmed || pendingCloudArm) && !foreign,
            displayStatus(currentMessage), if (foreign) 0f else latestVoiceLevel,
            awaitingStart = ownsImeRequest() && recording && imeSessionId == null,
        ).withEligibility(ownsImeRequest(), microphoneGranted, isSensitiveField(currentInputEditorInfo), foreign)
    }

    private fun handleVoiceGesture(action: ImeVoiceGesture) {
        if (hasForeignVoiceActivity()) { renderVoiceUi(); return }
        if (processing && action != ImeVoiceGesture.CANCEL) return
        when (action) {
            ImeVoiceGesture.TAP -> toggleDictation()
            ImeVoiceGesture.CANCEL -> if (recording || processing) cancelDictation()
            ImeVoiceGesture.REVIEW_CLOUD -> {
                if (recording && (cloudNoteArmed || pendingCloudArm)) {
                    cloudNoteReviewArmed = true
                    toggleDictation()
                }
            }
            ImeVoiceGesture.RAW -> {
                if (!recording) toggleDictation()
                if (recording) {
                    quickNoteArmed = false
                    rawModeArmed = true
                    performDoubleKeyHaptic()
                    updateStatus(currentMessage)
                }
            }
            ImeVoiceGesture.QUICK_NOTE -> {
                if (!recording) toggleDictation()
                if (recording) {
                    rawModeArmed = false
                    quickNoteArmed = true
                    performDoubleKeyHaptic()
                    updateStatus(currentMessage)
                }
            }
            ImeVoiceGesture.CLOUD_NOTE -> {
                val prefs = getSharedPreferences("openless_ime_ui", MODE_PRIVATE)
                val hasDestination = !prefs.getString("key_cloud_note_webhook_url", "").isNullOrBlank() &&
                    !prefs.getString("key_cloud_note_webhook_token", "").isNullOrBlank()
                if (!hasDestination) {
                    updateStatus(ui("请先配置云笔记地址和 Token，未开始云笔记听写", "Configure the Cloud note URL and token first; recording was not changed"))
                    return
                }
                if (!recording) toggleDictation(initialCloud = true)
                if (recording) {
                    cloudNoteArmed = true
                    performDoubleKeyHaptic()
                    updateStatus(currentMessage)
                }
            }
        }
    }

    private fun buildVoiceHeader(): ImeKeyboardHeader = ImeKeyboardHeader(
        this, keyboardPalette(), buildModeToggle(), brandLogoBitmap,
        onSettings = ::openSettings,
        onKeyboardSettings = ::openKeyboardSettings,
        onModeStep = ::swipeInputMode,
        onHide = ::hideKeyboardPanel,
    ).also {
        keyboardHeader = it
        backendLinkIndicator = it.indicator
        updateBackendLinkIndicator()
    }

    internal fun refreshInputView() {
        if (inputMode == InputMode.STROKE && appliedKeyboardFootprintKey == keyboardFootprintKey() &&
            strokeController.refreshPanel()) return
        if (inputMode == InputMode.CLIPBOARD && clipboardPanel != null &&
            appliedKeyboardFootprintKey == keyboardFootprintKey()) {
            renderClipboardPanel()
            return
        }
        if (inputMode == InputMode.ENGLISH && typingPanel != null &&
            appliedKeyboardFootprintKey == keyboardFootprintKey()) {
            renderTypingKeyboard()
            refreshLatinCandidateBar()
            return
        }
        replaceSurfaceContent()
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
        val keyboard = replaceSurfaceContent()
        val offset = resources.displayMetrics.widthPixels.toFloat() * slideDirection
        // RaisedKeyboardHost wraps [panel, raiseSpacer] — slide the panel's
        // own children (skip header at 0), not the raise spacer. The draft
        // bar sits above that host and stays put as well.
        val container = when (keyboard) {
            is RaisedKeyboardHost -> keyboard.getChildAt(0) as? ViewGroup
            is ViewGroup -> keyboard
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

    private fun buildDivider(): View = View(this).apply { setBackgroundColor(tone(Color.rgb(68, 68, 68), Color.rgb(215, 215, 218))) }

    // Tapping a toggle segment now slides the panel the same way a
    // left/right swipe does (same refreshInputView(slideDirection)
    // mechanics) — direction follows the segments' own left-to-right
    // InputMode.entries order (see ModeToggle.onDraw()), so tapping a
    // segment to the right slides in from the right, matching what a
    // swipe in that direction would already do. Tapping the
    // already-selected segment has no direction to slide from.
    internal fun buildModeToggle(): View = ImeModeBar(
        this, ImePalette.resolve(isDarkTheme),
        listOf(ui("语音", "Voice"), ui("笔画", "Stroke"), ui("编辑", "Edit"), ui("键盘", "Keys")),
        inputMode.ordinal,
    ) { index ->
        val selected = InputMode.entries[index]
        val direction = when {
            selected.ordinal > inputMode.ordinal -> 1
            selected.ordinal < inputMode.ordinal -> -1
            else -> null
        }
        selectInputMode(selected, slideDirection = direction)
    }

    private fun selectInputMode(selected: InputMode, slideDirection: Int? = null) {
        if (recording || processing) cancelDictation()
        inputMode = selected
        saveInputMode(selected)
        englishLayer = EnglishLayer.LETTERS
        englishComposingWord.clear()
        englishCandidateQueryEpoch++
        clearPinyinComposingInEditor()
        rimePinyinController.clear()
        rimePinyinController.resetAssociationContext()
        strokeController.resetForModeSwitch()
        clipboardSelectionMode = false
        clipboardSelectionAnchor = -1
        clipboardSelectionActive = -1
        clipboardHistoryMode = false
        clipboardHistoryCategory = OpenLessClipboardHistory.Category.ALL
        shiftState = ShiftState.OFF
        autoShift = false
        clearEnglishAutoText()
        updateAutoShift(refresh = false)
        if (slideDirection != null) refreshInputView(slideDirection) else refreshInputView()
    }

    /**
     * A deliberate header swipe steps through the same mode order
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

    private var typingPanel: ImeTypingPanel? = null

    /** Native presentation is independent of the IME's input/learning backends. */
    private fun buildKeyboardView(): View {
        // Survives a presentation-only rebuild while processing, but this view
        // still cannot cancel a different request started by a newer view.
        var viewVoiceOwner = typingVoiceOwner
        return ImeTypingPanel(
            this,
            ImePalette.resolve(isDarkTheme),
            buildVoiceHeader(),
            onAction = ::handleTypingAction,
            onFeedback = ::performKeyHaptic,
            onVoiceStart = {
                if (recording || processing) {
                    false
                } else if (!rimePinyinController.isEmpty()) {
                    updateStatus(ui("请先确认拼音候选，再开始说话", "Confirm the Pinyin candidate before speaking"))
                    false
                } else {
                    finalizeEnglishComposingWord()
                    if (!recording && !processing) toggleDictation()
                    if (recording) {
                        viewVoiceOwner = ImeRequestOwner(sessionEpoch, imeRequestId)
                        typingVoiceOwner = viewVoiceOwner
                    }
                    recording
                }
            },
            onVoiceEnd = { cancelled ->
                val owner = viewVoiceOwner
                if (owner != null && owner == typingVoiceOwner && owner.matches(sessionEpoch, imeRequestId)) {
                    if (cancelled) {
                        viewVoiceOwner = null
                        typingVoiceOwner = null
                        if (recording || processing) cancelDictation()
                    } else if (recording) {
                        // Release stops capture, but ownership remains while the
                        // result is processing so the visible Cancel still works.
                        toggleDictation()
                    }
                }
            },
            interfaceEnglish = englishUi,
            interactionPolicy = interactionPolicy(),
            onClearPreedit = ::clearPinyinEncoding,
            onCommitRawPreedit = ::commitRawPinyinInput,
            onCursorMove = ::handleTypingCursorMove,
            onDeleteWord = ::handleTypingDeleteWord,
        ).also {
            typingPanel = it
            renderTypingKeyboard()
            refreshLatinCandidateBar()
            renderClipboardQuickChip()
            renderVoiceUi()
        }
    }

    private fun renderTypingKeyboard() {
        typingPanel?.renderKeyboard(
            ImeKeyboardLayer.valueOf(englishLayer.name),
            if (latinInputMode == LatinInputMode.PINYIN) ImeTypingLanguage.PINYIN else ImeTypingLanguage.ENGLISH,
            when (shiftState) {
                ShiftState.OFF -> ImeLetterCase.LOWER
                ShiftState.SHIFT_ONCE -> ImeLetterCase.SHIFT
                ShiftState.CAPS_LOCK -> ImeLetterCase.CAPS_LOCK
            },
            editorActionLabel(),
            englishUi,
        )
    }

    private fun editorActionLabel(): String {
        val options = currentInputEditorInfo?.imeOptions ?: 0
        if (draftEdit != null || options and EditorInfo.IME_FLAG_NO_ENTER_ACTION != 0) return ui("换行", "Return")
        return when (options and EditorInfo.IME_MASK_ACTION) {
            EditorInfo.IME_ACTION_SEARCH -> ui("搜索", "Search")
            EditorInfo.IME_ACTION_SEND -> ui("发送", "Send")
            EditorInfo.IME_ACTION_GO -> ui("前往", "Go")
            EditorInfo.IME_ACTION_NEXT -> ui("下一项", "Next")
            EditorInfo.IME_ACTION_DONE -> ui("完成", "Done")
            else -> ui("换行", "Return")
        }
    }

    private fun handleTypingAction(action: ImeKeyAction) {
        if (recording || processing) return
        when (action) {
            is ImeKeyAction.Text -> commitEnglishChar(action.value)
            ImeKeyAction.Backspace -> englishDeleteBackward()
            ImeKeyAction.Shift -> cycleEnglishShift()
            ImeKeyAction.Language -> toggleLatinInputMode()
            ImeKeyAction.ToggleNumbers -> handleEnglishBottomModeToggle()
            ImeKeyAction.ToggleSymbols -> handleEnglishRow3ModeToggle()
            ImeKeyAction.Space -> {
                if (latinInputMode == LatinInputMode.PINYIN) {
                    rimePinyinController.space { renderPinyinCandidates(it) }
                } else {
                    finalizeEnglishComposingWord()
                    commitEnglishSpace()
                }
            }
            ImeKeyAction.Enter -> {
                if (latinInputMode != LatinInputMode.PINYIN) finalizeEnglishComposingWord()
                sendEnterKey()
            }
        }
    }

    /** Bottom-row toggle: always jumps straight to/from Letters, regardless of which of Numbers/Symbols was showing (matches iOS's own "ABC" key). */
    private fun handleEnglishBottomModeToggle() {
        englishLayer = if (englishLayer == EnglishLayer.LETTERS) EnglishLayer.NUMBERS else EnglishLayer.LETTERS
        shiftState = ShiftState.OFF
        autoShift = false
        finalizeEnglishComposingWord()
        updateAutoShift(refresh = false)
        refreshInputView()
    }

    /** Row-3 toggle: only ever switches between Numbers and Symbols, never touches Letters (matches iOS's own "#+="/"123" key). */
    private fun handleEnglishRow3ModeToggle() {
        englishLayer = if (englishLayer == EnglishLayer.NUMBERS) EnglishLayer.SYMBOLS else EnglishLayer.NUMBERS
        refreshInputView()
    }

    private fun cycleEnglishShift() {
        // Cycles lowercase -> capitalize-next -> caps-lock -> lowercase. A
        // capital turned on for a sentence start goes straight back off: the
        // user tapping Shift there wants lowercase, not caps lock.
        shiftState = when {
            autoShift && shiftState == ShiftState.SHIFT_ONCE -> ShiftState.OFF
            shiftState == ShiftState.OFF -> ShiftState.SHIFT_ONCE
            shiftState == ShiftState.SHIFT_ONCE -> ShiftState.CAPS_LOCK
            else -> ShiftState.OFF
        }
        autoShift = false
        applyShiftState()
    }

    /**
     * Turns a one-shot capital on at a sentence start (see
     * EnglishAutoText.isSentenceStart) and back off once the caret leaves
     * it — but only a capital it turned on itself; the user's own Shift or
     * caps lock is never overridden. Pass refresh = false when the caller
     * rebuilds the input view anyway.
     */
    private fun updateAutoShift(refresh: Boolean = true) {
        if (shiftState == ShiftState.CAPS_LOCK || (shiftState == ShiftState.SHIFT_ONCE && !autoShift)) return
        val applies = inputMode == InputMode.ENGLISH &&
            latinInputMode == LatinInputMode.ENGLISH &&
            englishLayer == EnglishLayer.LETTERS &&
            EnglishAutoText.allowsAutoCapitals(currentInputEditorInfo?.inputType ?: 0)
        val wanted = applies && currentInputConnection
            ?.getTextBeforeCursor(EnglishAutoText.SENTENCE_LOOKBEHIND, 0)
            ?.let { EnglishAutoText.isSentenceStart(it) } == true
        val next = when {
            wanted && shiftState == ShiftState.OFF -> ShiftState.SHIFT_ONCE
            !wanted && autoShift -> ShiftState.OFF
            else -> return
        }
        shiftState = next
        autoShift = next == ShiftState.SHIFT_ONCE
        if (refresh) applyShiftState()
    }

    private fun englishEditAnchor(connection: android.view.inputmethod.InputConnection): EnglishEditAnchor? {
        val extracted = connection.getExtractedText(android.view.inputmethod.ExtractedTextRequest(), 0) ?: return null
        if (extracted.selectionStart < 0 || extracted.selectionStart != extracted.selectionEnd) return null
        return EnglishEditAnchor(connection, extracted.startOffset + extracted.selectionStart)
    }

    private fun clearEnglishAutoText() {
        englishAutoSpace = null
        lastEnglishSpace = null
        lastEnglishSpaceAt = 0L
    }

    private fun invalidateEnglishAutoTextSelection(start: Int, end: Int) {
        if (start != end || start < 0) {
            clearEnglishAutoText()
            return
        }
        if (englishAutoSpace?.offset != start) englishAutoSpace = null
        if (lastEnglishSpace?.offset != start) {
            lastEnglishSpace = null
            lastEnglishSpaceAt = 0L
        }
    }

    /** Space on the English/pinyin keyboard; in English mode a second quick tap after a word turns "word " into "word. ". */
    private fun commitEnglishSpace() {
        val connection = currentInputConnection ?: return
        val now = android.os.SystemClock.uptimeMillis()
        val position = englishEditAnchor(connection)
        val elapsed = now - lastEnglishSpaceAt
        val quickSecondTap = lastEnglishSpace != null && lastEnglishSpace == position &&
            elapsed >= 0L && elapsed < DOUBLE_SPACE_WINDOW_MS
        clearEnglishAutoText()
        if (quickSecondTap &&
            latinInputMode == LatinInputMode.ENGLISH &&
            !ImeLearningPolicy.isPassword(currentInputEditorInfo?.inputType ?: 0) &&
            EnglishAutoText.doubleSpaceMakesFullStop(connection.getTextBeforeCursor(2, 0) ?: "")
        ) {
            if (connection.deleteSurroundingText(1, 0)) connection.commitText(". ", 1)
            return
        }
        if (connection.commitText(" ", 1)) {
            lastEnglishSpace = englishEditAnchor(connection)
            lastEnglishSpaceAt = now
        }
    }

    /** Backspace on the English keyboard: same field edit as everywhere else, plus keeping englishComposingWord (and therefore the candidate bar) in sync. In Pinyin mode, deletes from the (never-committed) encoding instead — falls back to the normal field edit only once the encoding is already empty (plan 3.3: "编码为空时 Backspace 恢复现有普通删除"). */
    private fun englishDeleteBackward() {
        if (latinInputMode == LatinInputMode.PINYIN) {
            rimePinyinController.backspace({ renderPinyinCandidates(it) }, {
                rimePinyinController.resetAssociationContext()
                deleteBackward()
            })
            return
        }
        clearEnglishAutoText()
        deleteBackward()
        if (englishComposingWord.isNotEmpty()) {
            englishComposingWord.deleteCharAt(englishComposingWord.length - 1)
        }
        updateEnglishCandidates()
    }

    private class LatinCandidate(val label: String, val longPress: (() -> Unit)?, val action: () -> Unit)

    /** Provider callbacks become target-bound actions; presentation owns view reuse. */
    private fun populateLatinCandidateRow(
        candidates: List<LatinCandidate>,
        leadingLabel: String? = null,
        statusHint: String? = null,
    ) {
        val epoch = sessionEpoch
        typingPanel?.submitCandidates(candidates.map { candidate ->
            ImeTypingPanel.Candidate(
                candidate.label,
                select = { if (epoch == sessionEpoch && inputMode == InputMode.ENGLISH) candidate.action() },
                forget = candidate.longPress?.let { action ->
                    { if (epoch == sessionEpoch && inputMode == InputMode.ENGLISH) action() }
                },
            )
        }, leadingLabel, statusHint)
    }

    private fun syncPinyinComposingText(preedit: String?) {
        val connection = currentInputConnection
        val allowed = connection != null &&
            inputMode == InputMode.ENGLISH &&
            latinInputMode == LatinInputMode.PINYIN &&
            !ImeLearningPolicy.isPassword(currentInputEditorInfo?.inputType ?: 0)
        if (allowed && !preedit.isNullOrEmpty()) {
            connection.setComposingText(preedit, 1)
            pinyinComposingActive = true
        } else {
            clearPinyinComposingInEditor()
        }
    }

    private fun clearPinyinComposingInEditor() {
        if (!pinyinComposingActive) return
        pinyinComposingActive = false
        currentInputConnection?.let { connection ->
            connection.setComposingText("", 1)
            connection.finishComposingText()
        }
    }

    private fun clearPinyinEncoding() {
        clearPinyinComposingInEditor()
        rimePinyinController.clear()
        renderPinyinCandidates(emptyList())
    }

    private fun commitRawPinyinInput() {
        if (latinInputMode != LatinInputMode.PINYIN || ImeLearningPolicy.isPassword(currentInputEditorInfo?.inputType ?: 0)) return
        rimePinyinController.commitRawInput({ renderPinyinCandidates(it) }, {})
    }

    private fun handleTypingCursorMove(delta: Int) {
        if (delta == 0) return
        if (latinInputMode == LatinInputMode.PINYIN && !rimePinyinController.isEmpty()) return
        finalizeEnglishComposingWord()
        val key = if (delta > 0) android.view.KeyEvent.KEYCODE_DPAD_RIGHT else android.view.KeyEvent.KEYCODE_DPAD_LEFT
        sendClipboardCursorKey(key)
    }

    private fun handleTypingDeleteWord() {
        if (latinInputMode == LatinInputMode.PINYIN && !rimePinyinController.isEmpty()) {
            clearPinyinEncoding()
            return
        }
        clearEnglishAutoText()
        finalizeEnglishComposingWord()
        val connection = currentInputConnection ?: return
        val selected = connection.getSelectedText(0)?.toString()
        if (!selected.isNullOrEmpty()) {
            if (connection.commitText("", 1)) offerUndoDeleteChip(selected)
            return
        }
        val before = connection.getTextBeforeCursor(24, 0)?.toString().orEmpty()
        if (before.isEmpty()) return
        val deleteLen = computeWordDeleteLength(before)
        if (deleteLen <= 0) return
        val removed = before.takeLast(deleteLen)
        if (connection.deleteSurroundingText(deleteLen, 0)) {
            rimePinyinController.resetAssociationContext()
            offerUndoDeleteChip(removed)
        }
    }

    private fun computeWordDeleteLength(before: String): Int {
        var idx = before.length
        while (idx > 0 && before[idx - 1].isWhitespace()) idx--
        if (idx == 0) return before.length
        val last = before[idx - 1]
        if (last.isLetterOrDigit() && last.code < 128) {
            while (idx > 0 && (before[idx - 1].isLetterOrDigit() && before[idx - 1].code < 128 || before[idx - 1] == '\'')) idx--
        } else if (last in '\u4e00'..'\u9fff') {
            var chars = 0
            while (idx > 0 && chars < 4 && before[idx - 1] in '\u4e00'..'\u9fff') {
                idx--
                chars++
            }
        } else {
            idx--
        }
        return (before.length - idx).coerceAtLeast(1)
    }

    private fun offerUndoDeleteChip(deleted: String) {
        if (deleted.isEmpty() || isSensitiveField(currentInputEditorInfo)) return
        quickChipPayload = deleted
        val preview = deleted.replace('\n', ' ').take(10)
        quickChipLabel = ui("撤销删除「$preview」", "Undo \"$preview\"")
        quickChipExpiresAtMs = android.os.SystemClock.elapsedRealtime() + 15_000L
        quickChipIsUndoDelete = true
        renderClipboardQuickChip()
    }

    private fun offerClipboardQuickChip(rawText: String) {
        val trimmed = rawText.trim()
        if (trimmed.isEmpty() || trimmed.length > 200) return
        val otp = extractVerificationCode(trimmed)
        if (otp != null) {
            quickChipPayload = otp
            quickChipLabel = ui("验证码: $otp", "Code: $otp")
        } else {
            quickChipPayload = trimmed
            val preview = trimmed.replace('\n', ' ').take(18)
            quickChipLabel = ui("粘贴: $preview", "Paste: $preview")
        }
        quickChipExpiresAtMs = android.os.SystemClock.elapsedRealtime() + 120_000L
        quickChipIsUndoDelete = false
        renderClipboardQuickChip()
    }

    private fun extractVerificationCode(text: String): String? {
        if (text.matches(Regex("\\d{4,8}"))) return text
        val hasCue = text.contains("验证码") || text.contains("校验码") || text.contains("动态码") ||
            text.contains("code", ignoreCase = true) || text.contains("otp", ignoreCase = true)
        if (!hasCue) return null
        return Regex("(?<!\\d)(\\d{4,8})(?!\\d)").find(text)?.groupValues?.getOrNull(1)
    }

    private fun dismissQuickChip() {
        quickChipPayload = null
        quickChipLabel = null
        quickChipExpiresAtMs = 0L
        quickChipIsUndoDelete = false
        typingPanel?.setClipboardSuggestion(null, {}, {})
    }

    private fun renderClipboardQuickChip() {
        val panel = typingPanel ?: return
        val now = android.os.SystemClock.elapsedRealtime()
        val payload = quickChipPayload
        val label = quickChipLabel
        if (payload == null || label == null || now > quickChipExpiresAtMs ||
            (!quickChipIsUndoDelete && isSensitiveField(currentInputEditorInfo))
        ) {
            panel.setClipboardSuggestion(null, {}, {})
            return
        }
        panel.setClipboardSuggestion(
            label,
            onPaste = {
                val connection = currentInputConnection
                if (connection != null && connection.commitText(payload, 1)) {
                    performKeyHaptic()
                    dismissQuickChip()
                }
            },
            onDismiss = ::dismissQuickChip,
        )
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
            val extracted = connection.getExtractedText(android.view.inputmethod.ExtractedTextRequest(), 0)
            val previous = if (clipboardSelectionAnchor >= 0 && clipboardSelectionActive >= 0) {
                ImeSelectionRange(clipboardSelectionAnchor, clipboardSelectionActive)
            } else null
            val range = ImeSelectionPolicy.extend(
                extracted?.text, extracted?.startOffset ?: -1,
                extracted?.selectionStart ?: -1, extracted?.selectionEnd ?: -1,
                previous, if (isRight) 1 else -1,
            )
            if (range != null && connection.setSelection(minOf(range.anchor, range.active), maxOf(range.anchor, range.active))) {
                clipboardSelectionAnchor = range.anchor
                clipboardSelectionActive = range.active
            } else {
                // Unknown/partial/changed editor data must not select from a guessed zero.
                clipboardSelectionAnchor = -1
                clipboardSelectionActive = -1
            }
            return
        }
        val now = android.os.SystemClock.uptimeMillis()
        connection.sendKeyEvent(android.view.KeyEvent(now, now, android.view.KeyEvent.ACTION_DOWN, code, 0))
        connection.sendKeyEvent(android.view.KeyEvent(now, now, android.view.KeyEvent.ACTION_UP, code, 0))
    }

    private var clipboardPanel: ImeClipboardPanel? = null

    private fun buildClipboardView(): View = buildClipboardSurface()
    private fun buildClipboardHistoryView(): View = buildClipboardSurface()

    private fun buildClipboardSurface(): View = ImeClipboardPanel(
        this, keyboardPalette(), buildVoiceHeader(), englishUi,
        onCommand = ::handleClipboardCommand,
        onCategory = { category ->
            if (clipboardActionCurrent(sessionEpoch)) {
                OpenLessClipboardHistory.Category.entries.firstOrNull { it.name == category }?.let {
                    clipboardHistoryCategory = it
                    renderClipboardPanel()
                }
            }
        },
        onFeedback = ::performKeyHaptic,
        interactionPolicy = interactionPolicy(),
    ).also {
        clipboardPanel = it
        renderClipboardPanel()
    }

    private fun clipboardActionCurrent(epoch: Long): Boolean =
        epoch == sessionEpoch && inputMode == InputMode.CLIPBOARD && isInputViewShown && !dictationEditingLocked()

    private fun renderClipboardPanel() {
        val panel = clipboardPanel ?: return
        val epoch = sessionEpoch
        val all = OpenLessClipboardHistory.load(this).distinctBy { it.text }
        val vocabulary = if (clipboardHistoryMode && isBackendReady()) {
            runCatching {
                val array = org.json.JSONArray(OpenLessNative.nativeVocabularyPhrases())
                (0 until array.length()).mapTo(mutableSetOf()) { array.getString(it) }
            }.getOrDefault(emptySet())
        } else emptySet()
        fun item(entry: ClipboardEntry) = ImeClipboardPanel.Item(
            text = entry.text,
            favorite = entry.favorite,
            inDictionary = entry.text in vocabulary,
            select = {
                if (clipboardActionCurrent(epoch)) pasteClipboardHistory(entry.text)
            },
            toggleFavorite = {
                if (clipboardActionCurrent(epoch)) {
                    OpenLessClipboardHistory.toggleFavorite(this, entry.text)
                    renderClipboardPanel()
                }
            },
            remove = {
                if (clipboardActionCurrent(epoch)) {
                    OpenLessClipboardHistory.delete(this, entry.text)
                    renderClipboardPanel()
                }
            },
            toggleDictionary = {
                if (clipboardActionCurrent(epoch)) toggleClipboardDictionary(entry.text, entry.text in vocabulary)
            },
        )
        panel.render(
            epoch, clipboardHistoryMode, clipboardSelectionMode, clipboardHistoryCategory.name,
            OpenLessClipboardHistory.filter(all, clipboardHistoryCategory).map(::item),
            all.take(3).map(::item),
        )
        panel.setBlocked(dictationEditingLocked())
    }

    private fun pasteClipboardHistory(text: String) {
        if (currentInputConnection?.commitText(text, 1) == true) {
            OpenLessClipboardHistory.recordCopy(this, text)
            clipboardHistoryMode = false
            renderClipboardPanel()
        } else {
            OpenLessNativeNotice.makeText(this, ui("输入连接已失效，未粘贴", "Editor unavailable; nothing pasted"), Toast.LENGTH_SHORT).show()
        }
    }

    private fun toggleClipboardDictionary(text: String, present: Boolean) {
        if (!isBackendReady()) {
            ensureBackendReady()
            OpenLessNativeNotice.makeText(this, ui("词典服务正在准备，请稍后重试", "Dictionary is preparing; please retry"), Toast.LENGTH_SHORT).show()
            return
        }
        runCatching {
            if (present) OpenLessNative.nativeRemoveVocabularyWord(text) else OpenLessNative.nativeAddVocabularyWord(text)
        }.onSuccess { renderClipboardPanel() }.onFailure {
            OpenLessNativeNotice.makeText(this, ui("词典操作失败，请重试", "Dictionary operation failed; please retry"), Toast.LENGTH_SHORT).show()
        }
    }

    private fun handleClipboardCommand(command: String) {
        if (!clipboardActionCurrent(sessionEpoch)) return
        when (command) {
            "range" -> {
                clipboardSelectionMode = !clipboardSelectionMode
                clipboardSelectionAnchor = -1
                clipboardSelectionActive = -1
                renderClipboardPanel()
            }
            "left" -> sendClipboardCursorKey(android.view.KeyEvent.KEYCODE_DPAD_LEFT)
            "right" -> sendClipboardCursorKey(android.view.KeyEvent.KEYCODE_DPAD_RIGHT)
            "up" -> sendClipboardCursorKey(android.view.KeyEvent.KEYCODE_DPAD_UP)
            "down" -> sendClipboardCursorKey(android.view.KeyEvent.KEYCODE_DPAD_DOWN)
            "all" -> currentInputConnection?.performContextMenuAction(android.R.id.selectAll)
            "copy" -> currentInputConnection?.performContextMenuAction(android.R.id.copy)
            "paste" -> currentInputConnection?.performContextMenuAction(android.R.id.paste)
            "backspace" -> deleteBackward()
            "history" -> {
                if (!isBackendReady()) ensureBackendReady()
                clipboardHistoryMode = true
                renderClipboardPanel()
            }
            "close_history" -> { clipboardHistoryMode = false; renderClipboardPanel() }
            "clear_history" -> { OpenLessClipboardHistory.clearNonFavorites(this); renderClipboardPanel() }
            "edit" -> openDictationEditDraft(requireSelection = true)
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

    /** Shift updates presentation in place; it never replaces the IME window. */
    private fun applyShiftState() {
        if (inputMode == InputMode.ENGLISH) renderTypingKeyboard()
    }

    private fun commitEnglishChar(char: String) {
        if (char.isEmpty()) return
        if (latinInputMode == LatinInputMode.PINYIN && !ImeLearningPolicy.isPassword(currentInputEditorInfo?.inputType ?: 0)) {
            if (char.length == 1 && char[0].isLetter()) {
                // Feed letter to Rime composition; preserve uppercase when Shift/CapsLock is on
                // so mixed English tokens (e.g. AI, DNA) work natively in rime_ice.
                rimePinyinController.appendLetter(char[0]) { renderPinyinCandidates(it) }
                consumeShiftOnce()
                return
            }
            val mapped = smartPinyinPunctuation(char)
            rimePinyinController.insertLiteral(mapped) { renderPinyinCandidates(it) }
            return
        }
        val connection = currentInputConnection ?: return
        val replaceAutoSpace = englishAutoSpace != null && englishAutoSpace == englishEditAnchor(connection) &&
            EnglishAutoText.swallowsAutoSpace(char) && connection.getTextBeforeCursor(1, 0)?.toString() == " "
        clearEnglishAutoText()
        if (replaceAutoSpace) {
            if (!connection.deleteSurroundingText(1, 0) || !connection.commitText("$char ", 1)) return
            englishAutoSpace = englishEditAnchor(connection)
            finalizeEnglishComposingWord()
            consumeShiftOnce()
            return
        }
        if (!connection.commitText(char, 1)) return
        if (personalizedLearningAllowed() && char.length == 1 && char[0].isLetter()) {
            englishComposingWord.append(char.lowercase())
            updateEnglishCandidates()
        } else {
            // Digits/punctuation end whatever word was being tracked, same
            // as space/return already do — "it's" typed via the symbols
            // page still finalizes cleanly, just with no candidate query
            // benefit for that particular boundary.
            finalizeEnglishComposingWord()
        }
        consumeShiftOnce()
    }

    /** A one-shot capital (the user's or updateAutoShift()'s) is spent by the character just typed. */
    private fun consumeShiftOnce() {
        if (shiftState != ShiftState.SHIFT_ONCE) return
        shiftState = ShiftState.OFF
        autoShift = false
        applyShiftState()
    }

    /** Ends the current word: records it for user-frequency learning (if long enough to be a real word) and clears the tracking buffer. */
    private fun finalizeEnglishComposingWord() {
        if (englishComposingWord.isNotEmpty()) {
            val word = englishComposingWord.toString()
            if (personalizedLearningAllowed() && englishSuggestionsEnabled() && word.length >= 2) {
                englishCandidateProvider.recordCommit(word)
            }
            englishComposingWord.clear()
        }
        updateEnglishCandidates()
    }

    private fun englishSuggestionsEnabled(): Boolean =
        getSharedPreferences("openless_ime_ui", MODE_PRIVATE).getBoolean("english_suggestions_enabled", true)

    /**
     * Re-queries EnglishCandidateProvider for the current englishComposingWord
     * prefix (or clears the bar if suggestions are off / nothing is being
     * typed) — called after every letter, backspace, and word boundary.
     * englishCandidateQueryEpoch discards a stale async result that comes
     * back after a faster subsequent keystroke already moved the prefix on.
     */
    private fun updateEnglishCandidates() {
        val epoch = ++englishCandidateQueryEpoch
        if (typingPanel == null) return
        if (!personalizedLearningAllowed() || !englishSuggestionsEnabled()) {
            renderEnglishCandidates(emptyList())
            return
        }
        val prefix = englishComposingWord.toString()
        if (prefix.isEmpty()) {
            renderEnglishCandidates(emptyList())
            return
        }
        englishCandidateProvider.queryTopN(prefix, ENGLISH_CANDIDATE_QUERY_LIMIT) { results ->
            if (epoch == englishCandidateQueryEpoch) renderEnglishCandidates(results)
        }
    }

    private fun renderEnglishCandidates(words: List<String>) {
        populateLatinCandidateRow(
            words.map { word -> LatinCandidate(word, longPress = { forgetEnglishCandidate(word) }) { selectEnglishCandidate(word) } },
        )
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
        if (!personalizedLearningAllowed()) return
        val connection = currentInputConnection ?: return
        val typed = englishComposingWord.toString()
        var typedPrefix = ""
        for (length in typed.length downTo 1) {
            val actual = connection.getTextBeforeCursor(length, 0)?.toString()
            if (actual != null && actual.equals(typed.takeLast(length), ignoreCase = true)) {
                typedPrefix = actual
                connection.deleteSurroundingText(length, 0)
                break
            }
        }
        val candidate = EnglishAutoText.candidateText(word, typedPrefix, shiftState == ShiftState.CAPS_LOCK)
        clearEnglishAutoText()
        if (!connection.commitText("$candidate ", 1)) return
        englishAutoSpace = englishEditAnchor(connection)
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
        OpenLessNativeNotice.makeText(this, ui("已移除“$word”", "Removed \"$word\""), Toast.LENGTH_SHORT).show()
        updateEnglishCandidates()
    }

    /**
     * Renders RimePinyinController's own candidate callback into the same
     * candidateItemView()/englishCandidateRow the English candidates use
     * (plan 5.3's Option B — see docs/pinyin-lite/phase-0-audit.md section
     * 4) — a non-clickable encoding label ("guo") is prepended when there's
     * a non-empty encoding, per phase-0 audit section 5, adding no new row
     * or container so the fixed 300dp panel height is never touched.
     */
    private fun smartPinyinPunctuation(char: String): String {
        if (char.length != 1) return char
        val ch = char[0]
        val afterDigit = rimePinyinController.isEmpty() &&
            currentInputConnection?.getTextBeforeCursor(1, 0)?.lastOrNull()?.isDigit() == true
        return when (ch) {
            ',' -> "，"
            '.' -> if (afterDigit) "." else "。"
            '。' -> if (afterDigit) "." else "。"
            ':' -> if (afterDigit) ":" else "："
            '?' -> "？"
            '!' -> "！"
            ';' -> "；"
            '(' -> "（"
            ')' -> "）"
            else -> char
        }
    }

    private fun renderPinyinCandidates(candidates: List<String>) {
        val preedit = rimePinyinController.currentPreedit().takeIf { it.isNotEmpty() }
        syncPinyinComposingText(preedit)
        val allowForget = personalizedLearningAllowed() && preedit != null
        populateLatinCandidateRow(
            candidates.map { char ->
                LatinCandidate(
                    char,
                    longPress = if (allowForget) ({ forgetPinyinCandidate(char) }) else null,
                ) { selectPinyinCandidate(char) }
            },
            leadingLabel = preedit,
            statusHint = rimePinyinController.statusHint(),
        )
    }

    private fun forgetPinyinCandidate(word: String) {
        if (!personalizedLearningAllowed()) return
        rimePinyinController.deleteCandidate(word, { renderPinyinCandidates(it) }) { deleted ->
            if (deleted) {
                performKeyHaptic()
                OpenLessNativeNotice.makeText(this, ui("已删除「$word」", "Removed \"$word\""), Toast.LENGTH_SHORT).show()
            }
        }
    }

    /** Candidate tap is the ONLY way a pinyin candidate reaches the field — see toggleLatinInputMode()'s doc comment on why Space deliberately never does this. */
    private fun selectPinyinCandidate(char: String) {
        if (ImeLearningPolicy.isPassword(currentInputEditorInfo?.inputType ?: 0)) return
        rimePinyinController.selectCandidate(char) { renderPinyinCandidates(it) }
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
        if (!personalizedLearningAllowed() || !OpenLessAndroidPreferences.strokeAssociationEnabled(this)) {
            renderPinyinAssociations(emptyList())
            return
        }
        rimePinyinController.queryAssociations(strokeController.phraseRepository) { results ->
            // isEmpty(): guards against the case where the user already
            // started typing a fresh encoding by the time this async result
            // lands — associationEpoch alone only protects against a STALE
            // association query, not against a completely different
            // encoding query having taken over the candidate row since.
            if (latinInputMode == LatinInputMode.PINYIN && rimePinyinController.isEmpty()) renderPinyinAssociations(results)
        }
    }

    /** Same row/candidateItemView() machinery as renderPinyinCandidates(), but no encoding label (there's no in-progress encoding here) and suffix-based commit (see selectPinyinAssociation()) instead of committing the whole label — mirrors StrokeInputController.refreshAssociations() exactly. */
    private fun renderPinyinAssociations(candidates: List<StrokePhraseRepository.Candidate>) {
        populateLatinCandidateRow(
            candidates.map { candidate -> LatinCandidate(outputScript(candidate.text), longPress = null) { selectPinyinAssociation(candidate) } },
        )
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
        if (personalizedLearningAllowed() && OpenLessAndroidPreferences.strokeUsageEnabled(this)) {
            strokeController.phraseRepository.recordUsage(candidate.matchedPrefix, candidate.text)
        }
        rimePinyinController.recordCommittedText(suffix)
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
        if (!selected.isNullOrEmpty()) {
            connection.commitText("", 1)
        } else if (!connection.deleteSurroundingTextInCodePoints(1, 0)) {
            // Editors without the codepoint API can interpret DEL themselves;
            // deleting one UTF-16 unit could leave half of an emoji behind.
            connection.sendKeyEvent(android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_DEL))
            connection.sendKeyEvent(android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_DEL))
        }
    }

    internal fun sendEnterKey() {
        clearEnglishAutoText()
        if (inputMode == InputMode.ENGLISH && latinInputMode == LatinInputMode.PINYIN &&
            !ImeLearningPolicy.isPassword(currentInputEditorInfo?.inputType ?: 0)) {
            rimePinyinController.confirmFirst({ renderPinyinCandidates(it) }, { sendEditorEnterKey() })
            return
        }
        sendEditorEnterKey()
    }

    private fun sendEditorEnterKey() {
        val connection = currentInputConnection ?: return
        val options = currentInputEditorInfo?.imeOptions ?: 0
        val action = options and EditorInfo.IME_MASK_ACTION
        if (draftEdit == null && options and EditorInfo.IME_FLAG_NO_ENTER_ACTION == 0 &&
            action != EditorInfo.IME_ACTION_NONE && action != EditorInfo.IME_ACTION_UNSPECIFIED &&
            connection.performEditorAction(action)) return
        connection.sendKeyEvent(android.view.KeyEvent(
            android.view.KeyEvent.ACTION_DOWN,
            android.view.KeyEvent.KEYCODE_ENTER,
        ))
        connection.sendKeyEvent(android.view.KeyEvent(
            android.view.KeyEvent.ACTION_UP,
            android.view.KeyEvent.KEYCODE_ENTER,
        ))
    }

    override fun onStartInput(attribute: EditorInfo?, restarting: Boolean) {
        cancelImeSession()
        closeDraft()
        super.onStartInput(attribute, restarting)
        keyboardHeader?.cancelInteractions()
        typingPanel?.closeTransientUi()
        clipboardPanel?.cancelInteractions()
        refreshLanguage()
        restoreInputMode()
        restoreScriptPreference()
        restoreLatinInputMode()
        strokeController.resetForNewInputSession()
        englishLayer = EnglishLayer.LETTERS
        // An automatic capital belonged to the old field's sentence start;
        // onStartInputView() works out the new field's.
        if (autoShift) shiftState = ShiftState.OFF
        autoShift = false
        clearEnglishAutoText()
        englishComposingWord.clear()
        englishCandidateQueryEpoch++
        clearPinyinComposingInEditor()
        rimePinyinController.clear()
        rimePinyinController.resetAssociationContext()
        rimePinyinController.setLearningAllowed(personalizedLearningAllowed())
        rimePinyinController.bindAfterCommit {
            if (inputMode == InputMode.ENGLISH && latinInputMode == LatinInputMode.PINYIN && rimePinyinController.isEmpty()) {
                refreshPinyinAssociations()
            }
        }
        rimePinyinController.bindCommit { text ->
            if (ImeLearningPolicy.isPassword(currentInputEditorInfo?.inputType ?: 0)) false
            else if (currentInputConnection?.commitText(text, 1) == true) {
                pinyinComposingActive = false
                // Typing an offered association out by hand counts as picking it.
                if (personalizedLearningAllowed() && OpenLessAndroidPreferences.strokeUsageEnabled(this)) {
                    strokeController.phraseRepository.recordTypedContinuation(
                        rimePinyinController.associationLearning.advance(rimePinyinController.associationContext(), text),
                    )
                } else {
                    rimePinyinController.associationLearning.reset()
                }
                rimePinyinController.recordCommittedText(text)
                performKeyHaptic()
                true
            } else false
        }
        if (ImeLearningPolicy.isPassword(attribute?.inputType ?: 0) ||
            ImeEditorFieldPolicy.prefersNumeric(attribute?.inputType ?: 0)) {
            // Field adaptation is temporary; do not overwrite the user's last page.
            inputMode = InputMode.ENGLISH
            latinInputMode = LatinInputMode.ENGLISH
            if (ImeEditorFieldPolicy.prefersNumeric(attribute?.inputType ?: 0)) englishLayer = EnglishLayer.NUMBERS
        }
        renderEnglishCandidates(emptyList())
        startRuntimeService()
        sessionEpoch++
        recording = false
        processing = false
        // A dictation result belongs to the editor it was typed into; carrying
        // it over to whatever field the user taps next risks an undo/edit
        // silently mangling unrelated text.
        lastDictationText = null
        dictationTextUndone = false
        if (isSensitiveField(attribute)) {
            updateStatus("敏感字段，已禁用听写")
        } else {
            setState("idle", "点击开始说话")
        }
    }

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        refreshInteractionPolicy()
        microphoneGranted = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        renderVoiceUi()
        // InputMethodService caches onCreateInputView()'s result across hide/
        // show. Keyboard settings write height/raise prefs while that cached
        // view is still the old footprint — rebuild when they diverge so the
        // next focus actually shows what the settings preview promised.
        // A channel may have been turned off in settings since the last show.
        if (isBackendReady()) refreshAsrRoute()
        val key = keyboardFootprintKey()
        val rebuild = appliedKeyboardFootprintKey != key
        updateAutoShift(refresh = false)
        if (rebuild) {
            // Only onCreateInputView may acknowledge a newly built footprint.
            // Updating this before refresh would incorrectly select the fast path.
            refreshInputView()
        } else if (inputMode == InputMode.ENGLISH && englishLayer == EnglishLayer.LETTERS) {
            applyShiftState()
            refreshLatinCandidateBar()
            renderClipboardQuickChip()
        } else if (inputMode == InputMode.ENGLISH) {
            // A cached alphabet page may now serve a numeric field with the
            // same privacy footprint (for example password -> phone number).
            renderTypingKeyboard()
            refreshLatinCandidateBar()
            renderClipboardQuickChip()
        } else if (inputMode == InputMode.CLIPBOARD) {
            renderClipboardPanel()
        } else if (inputMode == InputMode.STROKE) {
            strokeController.refreshPanel()
        }
    }

    private fun keyboardFootprintKey(): String =
        "${keyboardPanelHeightDp()}:${raiseHeightDp(this)}:$inputMode:$latinInputMode:${personalizedLearningAllowed()}:$isDarkTheme:$englishUi:draft=$draftToken:${draftEdit != null}"

    override fun onFinishInputView(finishingInput: Boolean) {
        draftBar?.cancelInteractions()
        keyboardHeader?.cancelInteractions()
        typingPanel?.closeTransientUi()
        clearEnglishAutoText()
        if (draftEdit != null && (recording || processing)) cancelDictation()
        closeDraft()
        clipboardPanel?.cancelInteractions()
        strokeController.cancelInteractions()
        super.onFinishInputView(finishingInput)
    }

    override fun onKeyDown(keyCode: Int, event: android.view.KeyEvent): Boolean {
        if (keyCode == android.view.KeyEvent.KEYCODE_BACK && draftEdit != null && isInputViewShown) {
            event.startTracking()
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    override fun onKeyUp(keyCode: Int, event: android.view.KeyEvent): Boolean {
        if (keyCode == android.view.KeyEvent.KEYCODE_BACK && draftEdit != null && event.isTracking && !event.isCanceled) {
            discardDraft()
            return true
        }
        return super.onKeyUp(keyCode, event)
    }

    override fun onFinishInput() {
        keyboardHeader?.cancelInteractions()
        typingPanel?.closeTransientUi()
        clipboardPanel?.cancelInteractions()
        clearEnglishAutoText()
        if (draftEdit != null && (recording || processing)) cancelDictation()
        closeDraft()
        cancelImeSession()
        englishCandidateQueryEpoch++
        englishComposingWord.clear()
        clearPinyinComposingInEditor()
        rimePinyinController.clear()
        rimePinyinController.resetAssociationContext()
        strokeController.resetForNewInputSession()
        if (recording) {
            cancelImeSession()
        }
        recording = false
        processing = false
        lastDictationText = null
        dictationTextUndone = false
        invalidateSession("输入目标已失效")
        super.onFinishInput()
    }

    /**
     * Hides undo/redo/edit once none of the dictated span is still immediately
     * before the cursor — the host cleared the field, or backspace removed the
     * utterance. A shorter matching prefix keeps the controls (one backspace
     * must not hide them). The undo action itself still refuses to delete
     * unless the full utterance is exactly before the cursor.
     */
    private fun invalidateDictationResultIfTextChanged() {
        val text = lastDictationText ?: return
        if (dictationTextUndone) return
        val connection = currentInputConnection ?: return
        val beforeCursor = connection.getTextBeforeCursor(text.length, 0) ?: return
        if (DictationSpan.hasRemainingPrefix(beforeCursor, text)) return
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
        if (draftEdit == null) invalidateEnglishAutoTextSelection(newSelStart, newSelEnd)
        // Selecting another span must not discard the user's pending undo/redo.
        if (newSelStart < 0 || newSelStart != newSelEnd) return
        invalidateDictationResultIfTextChanged()
        if (newSelStart == 0) clearCandidatesIfFieldEmptied()
        updateAutoShift()
    }

    /**
     * The host cleared the whole field without restarting input — a chat
     * app's send button does exactly this. Associations (and an English word
     * being typed) refer to text that is gone, so they must not linger in the
     * candidate row; the stroke panel also drops a half-typed code and its
     * code row. A pinyin encoding still being composed is left alone.
     */
    private fun clearCandidatesIfFieldEmptied() {
        // Typing goes to the draft box while it is open; the host field being
        // empty says nothing about what the candidate row refers to.
        if (draftEdit != null) return
        val connection = currentInputConnection ?: return
        val before = connection.getTextBeforeCursor(1, 0) ?: return
        val after = connection.getTextAfterCursor(1, 0) ?: return
        if (before.isNotEmpty() || after.isNotEmpty()) return
        strokeController.clearAfterFieldEmptied()
        rimePinyinController.resetAssociationContext()
        if (inputMode != InputMode.ENGLISH) return
        if (latinInputMode == LatinInputMode.PINYIN) {
            if (rimePinyinController.isEmpty()) renderPinyinAssociations(emptyList())
        } else if (englishComposingWord.isNotEmpty()) {
            englishComposingWord.clear()
            updateEnglishCandidates()
        }
    }

    internal fun toggleDictation(initialCloud: Boolean = false) {
        if (processing) return
        if (recording && quickNoteArmed) {
            quickNoteDictation()
            return
        }
        if (recording) {
            recording = false
            processing = true
            setState("thinking", "正在思考")
            stopImeSession()
        } else {
            if (hasForeignVoiceActivity()) { renderVoiceUi(); return }
        if (isSensitiveField(currentInputEditorInfo)) {
            updateStatus("敏感字段，禁止听写")
            return
        }
        microphoneGranted = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        if (!microphoneGranted) {
            updateStatus("请先授予麦克风权限")
            return
        }
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
            if (draftSubmitting) {
                // A new recording can write into the open draft while the
                // previous Cloud Notes request is still in flight. Its
                // completion must not disable or close this newer draft.
                draftSubmitting = false
                showDraftMessage(ui("新的听写已开始", "New dictation started"), isError = false)
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
            latestVoiceLevel = 0f
            maxObservedLevelThisSession = 0f
            voiceLinkWarning?.visibility = View.GONE
            performKeyHaptic()
            // A new utterance makes the previous result's undo/redo/edit
            // controls stale the moment recording starts, not just once the
            // new result replaces them — leaving them up mid-recording risked
            // an undo tap acting on the wrong utterance.
            lastDictationText = null
            setState("speaking", "再次点击结束 · 下划取消")
            imeRequestId = 0L
            imeSessionId = null
            cloudNoteArmed = initialCloud
            imeRequestId = nextImeRequest.incrementAndGet()
            activeImeOwner = ImeRequestOwner(sessionEpoch, imeRequestId)
            activeImeOwner?.let(imeFrameGate::begin)
            typingVoiceOwner = null
            imeStopRequested = false
            imeCaptureEnded = false
            externalRecording = false
            externalProcessing = false
            externalVoiceSessionId = null
            pendingImeStop = false
            if (!sendImeCommand("start", captureCursorContext())) {
                recording = false
                processing = false
                imeFrameGate.finish(activeImeOwner)
                activeImeOwner = null
                updateStatus(currentMessage)
            } else updateStatus(currentMessage)
        }
    }

    private fun cancelDictation() {
        recording = false
        processing = false
        rawModeArmed = false
        quickNoteArmed = false
        cloudNoteArmed = false
        cloudNoteSubmitting = false
        invalidateSession("已取消")
        setState("cancelled", ui("已取消", "Cancelled"))
        cancelImeSession()
    }

    private fun quickNoteDictation() {
        recording = false
        processing = true
        setState("thinking", ui("正在保存笔记", "Saving note"))
        stopImeSession()
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
        } else {
            refreshAsrRoute()
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
        val uiState = voiceUiState()
        val displayCapture = uiState.phase == ImeVoicePhase.RECORDING || uiState.phase == ImeVoicePhase.STARTING
        val displayProcessing = uiState.phase == ImeVoicePhase.PROCESSING || uiState.phase == ImeVoicePhase.SUBMITTING
        val armedAccent = if (hasForeignVoiceActivity()) null else armedAccentColor()
        val color = when {
            // Matches VoiceButton's own waveform/dots color for each armed
            // gesture exactly (armedModeColor()) — the breathing dot and
            // the mic animation should never disagree about which mode a
            // dictation is currently armed for, recording or thinking.
            (displayCapture || displayProcessing) && armedAccent != null -> armedAccent
            displayCapture -> LINK_COLOR_RECORDING
            displayProcessing -> LINK_COLOR_PROCESSING
            !backendLinkHealthy -> LINK_COLOR_ISSUE
            asrRoute == "localRaw" || (asrRoute == "local" && !networkAvailable) -> LINK_COLOR_READY_LOCAL_RAW
            asrRoute == "local" -> LINK_COLOR_READY_LOCAL
            asrRoute == "unavailable" -> LINK_COLOR_ISSUE
            else -> LINK_COLOR_READY
        }
        val ready = color == LINK_COLOR_READY || color == LINK_COLOR_READY_LOCAL || color == LINK_COLOR_READY_LOCAL_RAW
        keyboardHeader?.renderLink(color, ready)
    }

    /**
     * Asks the backend whether the next dictation runs locally and repaints
     * the dot. The call reads the credential store, so it runs on its own
     * thread; overlapping requests (heartbeat plus a keyboard show) collapse
     * into one.
     */
    private fun refreshAsrRoute() {
        if (!asrRouteRefreshing.compareAndSet(false, true)) return
        Thread {
            val route = runCatching { OpenLessNative.nativeAsrRoute() }.getOrNull() ?: "unavailable"
            val online = hasValidatedNetwork()
            backendHeartbeatHandler.post {
                asrRouteRefreshing.set(false)
                if (route != asrRoute || online != networkAvailable) {
                    asrRoute = route
                    networkAvailable = online
                    updateBackendLinkIndicator()
                }
            }
        }.apply { name = "openless-asr-route" }.start()
    }

    /** Whether the active network reaches the internet — polish needs it; local recognition doesn't. */
    private fun hasValidatedNetwork(): Boolean = runCatching {
        val connectivity = getSystemService(android.net.ConnectivityManager::class.java) ?: return@runCatching true
        val capabilities = connectivity.getNetworkCapabilities(connectivity.activeNetwork) ?: return@runCatching false
        capabilities.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
            capabilities.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_VALIDATED)
    }.getOrDefault(true)

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
        onCapsuleStateChangedWithSession(state, message, level, null)
    }

    override fun onCapsuleStateChangedWithSession(state: String, message: String?, level: Float, sessionId: String?) {
        val frame = ImeCapsuleFrame(sessionId?.takeUnless { it.isBlank() }, state, message, level)
        when (imeFrameGate.accept(frame)) {
            ImeFrameDisposition.OWNED -> {
                if (!ownsImeRequest()) return
                externalRecording = false
                externalProcessing = false
                externalVoiceSessionId = null
                applyOwnedCapsuleFrame(frame)
            }
            ImeFrameDisposition.EXTERNAL -> {
                when (state) {
                    "recording" -> { externalRecording = true; externalProcessing = false; externalVoiceSessionId = frame.sessionId }
                    "transcribing", "polishing" -> { externalRecording = false; externalProcessing = true; externalVoiceSessionId = frame.sessionId }
                    "idle", "done", "cancelled", "error" -> {
                        if (frame.sessionId == null || externalVoiceSessionId == null || frame.sessionId == externalVoiceSessionId) {
                            externalRecording = false
                            externalProcessing = false
                            externalVoiceSessionId = null
                        }
                        // Only real terminal frames retire a known foreign id.
                        // Idle is a presentation cue and may be temporary.
                        if (state != "idle") frame.sessionId?.let(imeFrameGate::retire)
                    }
                }
                renderVoiceUi()
            }
            ImeFrameDisposition.BUFFERED, ImeFrameDisposition.FOREIGN,
            ImeFrameDisposition.UNIDENTIFIED, ImeFrameDisposition.STALE -> Unit
        }
    }

    private fun applyOwnedCapsuleFrame(frame: ImeCapsuleFrame) {
        val state = frame.state
        val message = frame.message
        val level = frame.level
        when (ImeVoiceFramePolicy.resolve(true, imeCaptureEnded, state, imeStopRequested)) {
            ImeVoiceFrameAction.EXTERNAL -> return
            ImeVoiceFrameAction.IGNORE -> return
            ImeVoiceFrameAction.RECORDING -> {
                latestVoiceLevel = level
                recording = true
                processing = false
                maxObservedLevelThisSession = maxOf(maxObservedLevelThisSession, if (level.isFinite()) level else 0f)
                val elapsedMs = android.os.SystemClock.elapsedRealtime() - recordingStartedAtMs
                if (maxObservedLevelThisSession >= SILENCE_LEVEL_THRESHOLD) voiceLinkWarning?.visibility = View.GONE
                else if (elapsedMs > SILENCE_CHECK_DELAY_MS) {
                    voiceLinkWarning?.text = ui("检测到麦克风无声音，点击重启应用", "No mic audio detected — tap to restart the app")
                    voiceLinkWarning?.visibility = View.VISIBLE
                }
                setState("speaking", "再次点击结束 · 下划取消")
            }
            ImeVoiceFrameAction.PROCESSING -> {
                if (recording) performKeyHaptic()
                imeCaptureEnded = true
                recording = false
                processing = true
                setState("thinking", "正在思考")
            }
            ImeVoiceFrameAction.WAIT_RESULT -> {
                imeCaptureEnded = true
                recording = false
                processing = true
                setState("thinking", ui("正在完成听写", "Finishing dictation"))
            }
            ImeVoiceFrameAction.EXTERNALLY_COMPLETED -> {
                imeFrameGate.finish(activeImeOwner)
                activeImeOwner = null
                typingVoiceOwner = null
                recording = false
                processing = false
                imeCaptureEnded = true
                // A global stop does not emit this client's completed/text
                // callback. Never fabricate receipt or write an unknown result.
                setState("error", ui("听写已由其他入口结束，本输入法未收到文字；请检查历史或重试", "Dictation ended elsewhere; this keyboard received no text. Check history or retry"))
            }
            ImeVoiceFrameAction.CANCELLED -> {
                imeFrameGate.finish(activeImeOwner)
                activeImeOwner = null
                typingVoiceOwner = null
                recording = false
                processing = false
                setState("cancelled", message.orDefault("已取消"))
            }
            ImeVoiceFrameAction.ERROR -> {
                recording = false
                imeCaptureEnded = true
                if (imeStopRequested) {
                    // A terminal cue is not the scoped result. Keep the owner
                    // until its completed/error callback prevents early restart.
                    processing = true
                    setState("thinking", message.orDefault("正在确认听写结果"))
                } else {
                    imeFrameGate.finish(activeImeOwner)
                    activeImeOwner = null
                    typingVoiceOwner = null
                    processing = false
                    setState("error", message.orDefault("识别失败"))
                }
            }
        }
        renderVoiceUi(pushAudio = true)
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
        renderVoiceUi()
    }

    private fun renderVoiceUi(pushAudio: Boolean = false) {
        val snapshot = voiceUiState()
        val foreign = hasForeignVoiceActivity()
        val capture = !foreign && snapshot.phase == ImeVoicePhase.RECORDING
        val busy = foreign || snapshot.phase in listOf(ImeVoicePhase.STARTING, ImeVoicePhase.PROCESSING, ImeVoicePhase.SUBMITTING)
        val caption = when {
            foreign -> ui("听写服务正忙，请在发起听写的入口结束", "Dictation is busy; finish it in the entry that started it")
            snapshot.phase == ImeVoicePhase.STARTING -> ui("正在准备录音", "Preparing recording")
            !snapshot.canStart && !snapshot.editorLocked && isSensitiveField(currentInputEditorInfo) -> ui("敏感字段，已禁用听写", "Dictation disabled in this field")
            !snapshot.canStart && !snapshot.editorLocked && !microphoneGranted -> ui("需要麦克风权限，请打开设置", "Microphone permission required; open settings")
            else -> snapshot.message
        }
        status?.text = caption
        status?.setTextColor(recordingAccentColor())
        voiceRawHint?.text = rawModeHintText()
        voiceRawHint?.setTextColor(rawModeHintColor())
        voiceRawHint?.visibility = if (!foreign && rawModeHintVisible()) View.VISIBLE else View.GONE
        voiceButton?.isEnabled = snapshot.canStart || (ownsImeRequest() && recording)
        voiceButton?.isRecording = capture
        voiceButton?.isProcessing = busy
        voiceButton?.rawModeActive = snapshot.mode == ImeVoiceMode.RAW
        voiceButton?.quickNoteActive = snapshot.mode == ImeVoiceMode.QUICK_NOTE
        voiceButton?.cloudNoteActive = snapshot.mode == ImeVoiceMode.CLOUD_NOTE
        if (pushAudio) voiceButton?.audioLevel = snapshot.audioLevel
        voicePanel?.renderAvailability(capture, busy, snapshot.canCancel, snapshot.canStart || (ownsImeRequest() && recording))
        clipboardPanel?.setBlocked(snapshot.editorLocked)
        strokeController.setInteractionBlocked(snapshot.editorLocked)
        voiceButton?.contentDescription = when {
            foreign -> ui("其他入口正在听写", "Another entry is dictating")
            snapshot.phase == ImeVoicePhase.STARTING -> ui("正在准备录音，点击结束", "Preparing recording; tap to finish")
            ownsImeRequest() && recording -> ui("结束听写", "Finish dictation")
            snapshot.canStart -> ui("开始听写", "Start dictation")
            else -> caption
        }
        typingPanel?.renderVoice(
            caption, capture, busy, if (pushAudio) snapshot.audioLevel else null,
            snapshot.canCancel && typingVoiceOwner?.matches(sessionEpoch, imeRequestId) == true,
        )
        updateDictationResultControls()
        updateBackendLinkIndicator()
    }

    /**
     * Swipe-up-for-Raw / swipe-left-for-Quick-notes / swipe-right-for-
     * Cloud-notes discoverability hint while idle; once a recording is
     * actually armed into one of those three modes (live through both
     * recording and the following "thinking" step — 整理 for Raw and Cloud
     * notes, plus Cloud notes' own submit; saving for Quick notes), the
     * row repurposes itself to confirm whichever one is armed instead.
     * Recording-or-thinking with none armed (an ordinary dictation) never
     * reaches this text at all — see rawModeHintVisible(), which hides the
     * row entirely for that case.
     */
    private fun rawModeHintText(): String {
        return when {
            dictationInProgress() && rawModeArmed -> ui("原样转写", "Raw Mode")
            dictationInProgress() && quickNoteArmed -> ui("速记模式", "Quick notes")
            dictationInProgress() && cloudNoteArmed && cloudNoteReviewArmed -> ui("云笔记 · 发送前编辑", "Cloud notes · edit before sending")
            state == "speaking" && cloudNoteArmed -> ui("云笔记 · 上划编辑后发送", "Cloud notes · swipe up to edit first")
            dictationInProgress() && (cloudNoteArmed || cloudNoteSubmitting) -> ui("云笔记", "Cloud notes")
            else -> ui("上划RAW · 左划速记 · 右划云笔记", "Up: Raw · Left: Quick notes · Right: Cloud notes")
        }
    }

    /** Same orange/green/red as the status line's own Raw/Quick-notes/Cloud-notes coloring and every other indicator for each mode (VoiceButton's armed pill/waveform/dots) — muted gray otherwise. */
    private fun rawModeHintColor(): Int {
        val armedAccent = if (dictationInProgress()) armedAccentColor() else null
        return armedAccent ?: Color.argb((0.8f * 255).toInt(), 0xB0, 0xB0, 0xB0)
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
        return !(dictationInProgress() && armedAccentColor() == null)
    }

    /** The request owns its destination; delayed network callbacks cannot affect a later recording. */
    private fun submitCloudNoteText(
        text: String,
        destination: Pair<String, String>?,
        requestId: Long,
        onFinished: ((success: Boolean, message: String) -> Unit)? = null,
    ) {
        if (text.isBlank()) {
            recording = false
            processing = false
            setState("error", "没有识别到文字")
            onFinished?.invoke(false, "没有识别到文字")
            return
        }
        val (url, token) = destination ?: ("" to "")
        recording = false
        processing = false
        if (url.isEmpty() || token.isEmpty()) {
            val message = reportCloudNoteFailure(requestId, text, CloudNoteFailure.MissingDestination)
            onFinished?.invoke(false, message)
            return
        }
        cloudNoteSubmitting = true
        setState("thinking", "正在提交云笔记")
        Thread {
            val mainHandler = android.os.Handler(Looper.getMainLooper())
            var connection: java.net.HttpURLConnection? = null
            try {
                val body = org.json.JSONObject().apply {
                    put("token", token)
                    put("content", text)
                    put("client", "input_method")
                }.toString()
                connection = (java.net.URL(url).openConnection() as java.net.HttpURLConnection).apply {
                    requestMethod = "POST"
                    setRequestProperty("Content-Type", "application/json; charset=utf-8")
                    doOutput = true
                    connectTimeout = 8000
                    readTimeout = 8000
                }
                connection.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
                val code = connection.responseCode
                mainHandler.post {
                    val success = code in 200..299
                    val message = if (success) {
                        if (requestId == imeRequestId) {
                            setState("done", "已提交云笔记", QUICK_NOTE_CONFIRMATION_DELAY_MS)
                        }
                        "已提交云笔记"
                    } else {
                        reportCloudNoteFailure(requestId, text, CloudNoteFailure.Http, code)
                    }
                    onFinished?.invoke(success, message)
                }
            } catch (error: Throwable) {
                android.util.Log.w("OpenLessImeService", "cloud note webhook submit failed: ${error.javaClass.simpleName}")
                mainHandler.post {
                    val message = reportCloudNoteFailure(requestId, text, CloudNoteFailure.Network)
                    onFinished?.invoke(false, message)
                }
            } finally { connection?.disconnect() }
        }.start()
    }

    /**
     * Keeps a transcript the webhook did not accept. Core already removed the
     * recording and history, so the completion text is the only remaining copy.
     * A newer recording may own the status line; the clipboard entry still has
     * to be written, and a toast reports it without touching that session.
     */
    private fun reportCloudNoteFailure(
        requestId: Long,
        text: String,
        failure: CloudNoteFailure,
        httpCode: Int = 0,
    ): String {
        val retained = cloudNoteShouldRetain(text)
        val persisted = retained && OpenLessClipboardHistory.recordCopy(this, text)
        val (messageZh, messageEn) = cloudNoteFailureMessage(
            cloudNoteFailureDetail(failure, httpCode),
            retained,
            persisted,
        )
        val message = ui(messageZh, messageEn)
        if (requestId == imeRequestId) {
            setState("error", message)
        } else {
            OpenLessNativeNotice.makeText(this, message, Toast.LENGTH_LONG).show()
        }
        return message
    }

    /** What Confirm does with the text in the draft bar. */
    private enum class DraftPurpose {
        /** Post it to the Cloud notes webhook. */
        CLOUD_NOTE,
        /** Write it back into the host field, over the text it was opened with. */
        DICTATION_EDIT,
    }

    /**
     * Draft box of the draft bar; non-null exactly while the bar is up. The
     * same instance is re-parented into every rebuilt input view (a mode
     * switch rebuilds the whole panel), so the text, caret, selection and
     * scroll position survive without being copied around.
     */
    private var draftEdit: android.widget.EditText? = null
    private var draftConnection: android.view.inputmethod.InputConnection? = null
    private var draftPurpose = DraftPurpose.CLOUD_NOTE
    private var draftCloudDestination: Pair<String, String>? = null
    // DICTATION_EDIT: the host-field text being replaced, whether it is the
    // whole last dictation result or the host field's selection, and where it
    // started in the host field when the draft opened (null if the host
    // doesn't report absolute offsets).
    private var draftOriginalText: String? = null
    private var draftReplacesWholeResult = false
    private var draftOriginalHostStart: Int? = null
    private var draftRestoreDictationText: String? = null
    private var draftRestoreDictationUndone = false
    private var draftSubmitting = false
    private var draftMessage: String? = null
    private var draftMessageIsError = false
    // Bumped whenever a draft opens or closes; a submit result only applies to the draft that sent it.
    private var draftToken = 0L

    /**
     * Every key, candidate, backspace and dictation commit in this service
     * writes through currentInputConnection. While a draft is up that is the
     * draft box, never the host app's field — and never a fallback to the
     * host connection, even if the draft's own one is unavailable.
     */
    override fun getCurrentInputConnection(): android.view.inputmethod.InputConnection? {
        val edit = draftEdit ?: return super.getCurrentInputConnection()
        draftConnection?.let { return it }
        val base = edit.onCreateInputConnection(EditorInfo()) ?: return null
        return DraftConnection(base, edit).also { draftConnection = it }
    }

    /** The host app's field, also while a draft has taken over currentInputConnection. */
    private fun hostInputConnection(): android.view.inputmethod.InputConnection? = super.getCurrentInputConnection()

    /** Composition and dictation-result state belongs to one text target; drop it when the target flips between the host field and the draft. */
    private fun resetEditingStateForTargetChange() {
        clearEnglishAutoText()
        englishComposingWord.clear()
        englishCandidateQueryEpoch++
        clearPinyinComposingInEditor()
        rimePinyinController.clear()
        rimePinyinController.resetAssociationContext()
        strokeController.resetForModeSwitch()
        clipboardSelectionMode = false
        clipboardSelectionAnchor = -1
        clipboardSelectionActive = -1
        lastDictationText = null
        dictationTextUndone = false
    }

    /** Puts the draft bar up with [text] in the box, caret at the end. */
    private fun openDraft(purpose: DraftPurpose, text: String) {
        resetEditingStateForTargetChange()
        draftToken++
        draftPurpose = purpose
        draftSubmitting = false
        draftMessage = null
        draftMessageIsError = false
        draftConnection = null
        draftEdit = object : android.widget.EditText(this) {
            override fun onSelectionChanged(selStart: Int, selEnd: Int) {
                super.onSelectionChanged(selStart, selEnd)
                invalidateEnglishAutoTextSelection(selStart, selEnd)
            }
        }.apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or
                InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI
            setSingleLine(false)
            minLines = 2
            maxLines = 4
            gravity = android.view.Gravity.TOP or android.view.Gravity.START
            textSize = 16f
            isVerticalScrollBarEnabled = true
            importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO
            // This window never takes input focus; the keys below are the only keyboard.
            showSoftInputOnFocus = false
            // No floating selection/insertion toolbar from the IME window.
            // A tap still places the caret, and the clipboard panel still
            // selects, copies and pastes through the draft connection.
            isLongClickable = false
            setOnLongClickListener { true }
            val noActionMode = object : android.view.ActionMode.Callback {
                override fun onCreateActionMode(mode: android.view.ActionMode?, menu: android.view.Menu?) = false
                override fun onPrepareActionMode(mode: android.view.ActionMode?, menu: android.view.Menu?) = false
                override fun onActionItemClicked(mode: android.view.ActionMode?, item: android.view.MenuItem?) = false
                override fun onDestroyActionMode(mode: android.view.ActionMode?) = Unit
            }
            customSelectionActionModeCallback = noActionMode
            customInsertionActionModeCallback = noActionMode
            setText(text)
            setSelection(this.text.length)
        }
        refreshInputView()
    }

    /** A Cloud notes dictation ended by a swipe up lands here instead of being sent: the text goes into the draft box and waits for Confirm. */
    private fun openCloudNoteDraft(text: String, destination: Pair<String, String>?) {
        recording = false
        processing = false
        if (text.isBlank()) {
            setState("error", "没有识别到文字")
            return
        }
        val existing = draftEdit
        if (existing != null) {
            // Dictated while a draft is still open: the text joins that draft at the caret.
            val editable = existing.text
            val start = existing.selectionStart.coerceAtLeast(0)
            val end = existing.selectionEnd.coerceAtLeast(0)
            editable.replace(minOf(start, end), maxOf(start, end), text)
            if (draftPurpose == DraftPurpose.CLOUD_NOTE) draftCloudDestination = destination
            showDraftMessage(null, isError = false)
        } else {
            openDraft(DraftPurpose.CLOUD_NOTE, text)
            draftCloudDestination = destination
        }
        if (draftPurpose == DraftPurpose.CLOUD_NOTE) {
            setState("done", ui("请确认云笔记草稿", "Review the cloud note draft"))
        } else {
            setState("done", "已上屏")
        }
    }

    /** Confirm button: hands the edited draft to whatever it was opened for, once. */
    private fun confirmDraft() {
        val edit = draftEdit ?: return
        if (draftSubmitting) return
        if (recording || processing) {
            showDraftMessage(ui("请先结束当前听写", "Finish the current dictation first"), isError = true)
            return
        }
        currentInputConnection?.finishComposingText()
        val text = edit.text.toString()
        if (text.isBlank()) {
            showDraftMessage(ui("内容为空，无法确认", "Nothing to confirm"), isError = true)
            return
        }
        when (draftPurpose) {
            DraftPurpose.CLOUD_NOTE -> submitCloudNoteDraft(text)
            DraftPurpose.DICTATION_EDIT -> applyDictationEditDraft(text)
        }
    }

    /** The bar stays up until the request finishes, so a failed submit keeps the text. */
    private fun submitCloudNoteDraft(text: String) {
        draftSubmitting = true
        showDraftMessage(displayStatus("正在提交云笔记"), isError = false)
        val token = draftToken
        val submissionRequestId = imeRequestId
        submitCloudNoteText(text, draftCloudDestination, submissionRequestId) { success, message ->
            if (token != draftToken || submissionRequestId != imeRequestId) return@submitCloudNoteText
            draftSubmitting = false
            if (success) {
                val unchanged = draftEdit?.text?.toString() == text
                if (recording || processing || !unchanged) {
                    showDraftMessage(displayStatus(message), isError = false)
                } else {
                    closeDraft()
                    // The status line only exists on the voice panel.
                    if (inputMode != InputMode.VOICE) OpenLessNativeNotice.makeText(this, displayStatus(message), Toast.LENGTH_SHORT).show()
                }
            } else {
                showDraftMessage(
                    if (englishUi && message.startsWith("云笔记提交失败")) "Cloud notes submit failed" else displayStatus(message),
                    isError = true,
                )
            }
        }
    }

    /** Absolute offset of the host field's caret (selection start), or null if the host doesn't report one. */
    private fun hostCaretOffset(host: android.view.inputmethod.InputConnection): Int? {
        val extracted = host.getExtractedText(android.view.inputmethod.ExtractedTextRequest(), 0) ?: return null
        if (extracted.selectionStart < 0 || extracted.selectionEnd < 0) return null
        return extracted.startOffset + minOf(extracted.selectionStart, extracted.selectionEnd)
    }

    /**
     * Swaps [original] for [text] in the host field, wherever the host's
     * caret is by now; see [ImeDraftReplacement] for how the original is
     * found. False — with the host field untouched — if it is no longer there.
     */
    private fun replaceInHost(
        host: android.view.inputmethod.InputConnection,
        original: String,
        text: String,
        hintStart: Int?,
    ): Boolean {
        val selected = host.getSelectedText(0)?.toString().orEmpty()
        val reach = HOST_SEARCH_REACH + original.length
        val before = host.getTextBeforeCursor(reach, 0)?.toString() ?: return false
        val after = host.getTextAfterCursor(reach, 0)?.toString().orEmpty()
        val plan = ImeDraftReplacement.plan(original, before, selected, after, hostCaretOffset(host), hintStart)
            ?: return false
        host.beginBatchEdit()
        // commitText() replaces the selection it is given.
        val replaced = when (plan) {
            ImeDraftReplacement.ReplaceSelection -> host.commitText(text, 1)
            is ImeDraftReplacement.SelectRange -> host.setSelection(plan.start, plan.end) && host.commitText(text, 1)
            is ImeDraftReplacement.DeleteAroundCaret ->
                host.deleteSurroundingText(plan.before, plan.after) && host.commitText(text, 1)
        }
        host.endBatchEdit()
        return replaced
    }

    /** Replaces the original text in the host field with the edited draft; if the original is gone from the host field, nothing is touched. */
    private fun applyDictationEditDraft(text: String) {
        val original = draftOriginalText
        val host = hostInputConnection()
        if (original == null || host == null) {
            showDraftMessage(displayStatus("输入连接已失效"), isError = true)
            return
        }
        val replacesWhole = draftReplacesWholeResult
        if (!replaceInHost(host, original, text, draftOriginalHostStart)) {
            showDraftMessage(ui("找不到原文，无法替换", "The original text is gone"), isError = true)
            return
        }
        closeDraft()
        // A sub-span correction leaves the surrounding text's exact new
        // length untracked, so the coarse "undo the whole utterance" no
        // longer has a clean span to act on; a whole-result replacement is
        // still one contiguous span, so undo/redo keeps working against it.
        lastDictationText = if (replacesWhole) text else null
        lastDictationEpoch = sessionEpoch
        dictationTextUndone = false
        setState("done", "已上屏")
        updateDictationResultControls()
    }

    /** Cancel button / Back: drops the draft; nothing is sent and the host field stays as it was. */
    private fun discardDraft() {
        if (draftEdit == null || draftSubmitting) return
        val purpose = draftPurpose
        val restoreText = draftRestoreDictationText
        val restoreUndone = draftRestoreDictationUndone
        if (recording || processing) cancelDictation()
        closeDraft()
        if (purpose == DraftPurpose.CLOUD_NOTE) {
            setState("done", ui("已丢弃云笔记草稿", "Cloud note draft discarded"))
            return
        }
        lastDictationText = restoreText
        // Cancelling draft dictation advances the epoch, not the host text target.
        lastDictationEpoch = sessionEpoch
        dictationTextUndone = restoreUndone
        setState("idle", "点击开始说话")
        updateDictationResultControls()
    }

    /** Takes the bar down and hands the keys back to the host field. Never submits or writes anything. */
    private fun closeDraft() {
        if (draftEdit == null) return
        draftToken++
        draftEdit = null
        draftConnection = null
        draftPurpose = DraftPurpose.CLOUD_NOTE
        draftCloudDestination = null
        draftOriginalText = null
        draftReplacesWholeResult = false
        draftOriginalHostStart = null
        draftRestoreDictationText = null
        draftRestoreDictationUndone = false
        draftSubmitting = false
        draftMessage = null
        draftMessageIsError = false
        draftBar?.cancelInteractions()
        draftBar = null
        resetEditingStateForTargetChange()
        refreshInputView()
    }

    private fun showDraftMessage(message: String?, isError: Boolean) {
        draftMessage = message
        draftMessageIsError = isError
        updateDraftControls()
    }

    private fun updateDraftControls() {
        val message = draftMessage ?: when (draftPurpose) {
            DraftPurpose.CLOUD_NOTE -> ui("云笔记草稿", "Cloud note draft")
            DraftPurpose.DICTATION_EDIT -> ui("编辑后替换原文", "Edit, then replace")
        }
        draftBar?.render(
            message, draftMessage != null && draftMessageIsError, draftSubmitting,
            if (draftPurpose == DraftPurpose.CLOUD_NOTE) keyboardPalette().error else keyboardPalette().accent,
        )
    }

    private fun buildDraftBar(): ImeDraftBar? {
        val edit = draftEdit ?: return null
        return ImeDraftBar(
            this, keyboardPalette(), edit,
            onCancel = ::discardDraft,
            onConfirm = ::confirmDraft,
            onFeedback = ::performKeyHaptic,
        ).also { bar ->
            draftBar = bar
            bar.cancel.text = ui("取消", "Cancel")
            bar.cancel.contentDescription = ui("取消草稿", "Cancel draft")
            bar.confirm.text = ui("确认", "Confirm")
            bar.confirm.contentDescription = ui("确认草稿", "Confirm draft")
            updateDraftControls()
        }
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
        lastDictationText = text
        lastDictationEpoch = sessionEpoch
        dictationTextUndone = false
        setState("done", "已上屏")
    }

    /** Shows/hides and relabels the undo↔redo toggle and edit button for the last dictation result. */
    private fun updateDictationResultControls() {
        // No longer gated on state == "done": that flips to "recording"/
        // "thinking" for the *next* utterance almost immediately, which made
        // these controls disappear right after appearing. They should stay
        // up as long as there's a previous result sitting in the buffer to
        // undo/redo/edit, regardless of what the mic is doing right now.
        val visible = inputMode == InputMode.VOICE && lastDictationText != null
        // This row is a FrameLayout overlay above the footer (see
        // buildDictationResultOverlay()), not part of the panel's own
        // LinearLayout flow, so GONE here never shifts the mic or footer.
        dictationResultRow?.visibility = if (visible) View.VISIBLE else View.GONE
        // A dictation made into an open draft can still be undone there, but
        // not edited in a second draft on top of the first.
        editResultButton?.visibility = if (draftEdit == null) View.VISIBLE else View.INVISIBLE
        undoRedoButton?.text = if (dictationTextUndone) "↻" else "✕"
        undoRedoButton?.contentDescription = if (dictationTextUndone) {
            ui("重做本次听写结果", "Redo this dictation")
        } else {
            ui("撤销本次听写结果", "Undo this dictation")
        }
    }

    private fun requireCollapsedDictationCursor(
        connection: android.view.inputmethod.InputConnection,
        selectionMessage: String,
    ): Boolean {
        val extracted = connection.getExtractedText(android.view.inputmethod.ExtractedTextRequest(), 0)
        val length = extracted?.text?.length ?: -1
        if (extracted == null || extracted.selectionStart !in 0..length || extracted.selectionEnd !in 0..length) {
            setState("error", ui("无法确认光标位置，未修改文字", "Cannot verify cursor position; text unchanged"))
            return false
        }
        if (extracted.selectionStart != extracted.selectionEnd) {
            setState("error", selectionMessage)
            return false
        }
        return true
    }

    /** Toggles between removing this dictation's committed text and restoring it. */
    private fun toggleUndoRedoDictation() {
        val text = lastDictationText ?: return
        if (sessionEpoch != lastDictationEpoch) return
        val connection = currentInputConnection ?: return
        if (!requireCollapsedDictationCursor(connection, ui("请先取消选区，再撤销/重做", "Clear the selection before undo/redo"))) return
        if (!dictationTextUndone) {
            // deleteSurroundingText is length-based. If the utterance is no
            // longer exactly before the cursor (backspaced away, or the caret
            // sits inside it), deleting would erase neighboring text.
            val beforeCursor = connection.getTextBeforeCursor(text.length, 0)
            if (beforeCursor == null) {
                setState("error", ui("无法读取原文，未撤销", "Cannot read original text; undo cancelled"))
                return
            }
            if (!DictationSpan.matchesBeforeCursor(beforeCursor, text)) {
                lastDictationText = null
                updateDictationResultControls()
                return
            }
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
     * ✎ on the voice panel, and the long-press on the clipboard panel's
     * History key: puts the text to fix into the draft bar, where every
     * input mode can edit it; Confirm writes it back over the original.
     * The text is whatever is selected in the host field, or — unless
     * [requireSelection] — the whole last dictation result.
     */
    private fun openDictationEditDraft(requireSelection: Boolean) {
        if (draftEdit != null || recording || processing) return
        val connection = currentInputConnection ?: return
        val selected = connection.getSelectedText(0)?.toString()?.takeIf { it.isNotEmpty() }
        val resultIntact = sessionEpoch == lastDictationEpoch && !dictationTextUndone
        val original = when {
            selected != null -> selected
            requireSelection -> {
                OpenLessNativeNotice.makeText(this, ui("请先选中字词", "Please select word"), Toast.LENGTH_SHORT).show()
                return
            }
            else -> lastDictationText?.takeIf { resultIntact } ?: return
        }
        // Cancel puts the undo/redo/edit controls back exactly as they were.
        val restoreText = lastDictationText?.takeIf { sessionEpoch == lastDictationEpoch }
        val restoreUndone = restoreText != null && dictationTextUndone
        val caret = hostCaretOffset(connection)
        val originalStart = when {
            caret == null -> null
            selected != null -> caret
            connection.getTextBeforeCursor(original.length, 0)?.toString() == original -> caret - original.length
            else -> null
        }
        openDraft(DraftPurpose.DICTATION_EDIT, original)
        draftOriginalText = original
        draftReplacesWholeResult = selected == null
        draftOriginalHostStart = originalStart
        draftRestoreDictationText = restoreText
        draftRestoreDictationUndone = restoreUndone
    }

    private fun setState(nextState: String, message: String, revertDelayMs: Long = DONE_TO_IDLE_DELAY_MS) {
        state = nextState
        if (nextState != "done") ++statusRevertToken
        // The submit only ever shows as "thinking"; any other state means
        // it finished, failed, or a new dictation took over.
        if (nextState != "thinking") cloudNoteSubmitting = false
        updateStatus(message)
        // A completed commit/edit (see the various setState("done", "已
        // 上屏") call sites) used to just sit there until the next recording
        // started; the status line and the Raw hint below it should instead
        // settle back to the same ready state as a fresh session shortly
        // after, so it's visibly clear another dictation can start right
        // away — see scheduleRevertToIdle(). revertDelayMs only ever
        // differs from the default for quickNoteDictation()'s own shorter
        // "笔记已记录" confirmation (QUICK_NOTE_CONFIRMATION_DELAY_MS).
        if (nextState == "done" || nextState == "cancelled") scheduleRevertToIdle(revertDelayMs)
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
            if (sessionEpoch == epoch && statusRevertToken == token && !dictationEditingLocked()) {
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

    private fun startRuntimeService() {
        try {
            OpenLessRuntimeService.setImeRequested(this, true)
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
        OpenLessRuntimeService.setImeRequested(this, false)
    }

    /**
     * Outer IME host for stretch+raise: key panel on top + empty lift strip
     * below. Owns exact total height because setInputView replaces LayoutParams.
     * Re-reads prefs on every measure so a settings change applies on the
     * next layout even before a full view rebuild.
     */
    private class RaisedKeyboardHost(
        context: android.content.Context,
    ) : LinearLayout(context) {
        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
            val preferredPanel = panelHeightPx(context)
            val preferredRaise = raiseHeightPx(context)
            val bound = if (View.MeasureSpec.getMode(heightMeasureSpec) == View.MeasureSpec.UNSPECIFIED) {
                preferredPanel + preferredRaise
            } else View.MeasureSpec.getSize(heightMeasureSpec).coerceAtLeast(0)
            val panel = minOf(preferredPanel, bound)
            val raise = minOf(preferredRaise, (bound - panel).coerceAtLeast(0))
            if (childCount >= 1) {
                val keys = getChildAt(0)
                if (keys.layoutParams.height != panel) keys.layoutParams = LayoutParams(LayoutParams.MATCH_PARENT, panel)
            }
            if (childCount >= 2) {
                val spacer = getChildAt(1)
                if (spacer.layoutParams.height != raise) spacer.layoutParams = LayoutParams(LayoutParams.MATCH_PARENT, raise)
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
     * The draft box's own InputConnection, with key events turned into text
     * edits. The IME window never has window focus, and the framework drops
     * key events dispatched to an unfocused window — so Return, Backspace and
     * the cursor keys sent through sendKeyEvent() would silently do nothing.
     */
    private class DraftConnection(
        target: android.view.inputmethod.InputConnection,
        private val edit: android.widget.EditText,
    ) : android.view.inputmethod.InputConnectionWrapper(target, true) {
        override fun sendKeyEvent(event: android.view.KeyEvent): Boolean {
            if (event.action != android.view.KeyEvent.ACTION_DOWN) return true
            val text = edit.text
            val start = minOf(edit.selectionStart, edit.selectionEnd).coerceAtLeast(0)
            val end = maxOf(edit.selectionStart, edit.selectionEnd).coerceAtLeast(0)
            val layout = edit.layout
            when (event.keyCode) {
                android.view.KeyEvent.KEYCODE_ENTER, android.view.KeyEvent.KEYCODE_NUMPAD_ENTER -> commitText("\n", 1)
                android.view.KeyEvent.KEYCODE_DEL ->
                    if (start != end) commitText("", 1) else deleteSurroundingTextInCodePoints(1, 0)
                android.view.KeyEvent.KEYCODE_FORWARD_DEL ->
                    if (start != end) commitText("", 1) else deleteSurroundingTextInCodePoints(0, 1)
                android.view.KeyEvent.KEYCODE_DPAD_LEFT ->
                    if (layout != null) android.text.Selection.moveLeft(text, layout) else edit.setSelection((start - 1).coerceAtLeast(0))
                android.view.KeyEvent.KEYCODE_DPAD_RIGHT ->
                    if (layout != null) android.text.Selection.moveRight(text, layout) else edit.setSelection((end + 1).coerceAtMost(text.length))
                android.view.KeyEvent.KEYCODE_DPAD_UP -> if (layout != null) android.text.Selection.moveUp(text, layout)
                android.view.KeyEvent.KEYCODE_DPAD_DOWN -> if (layout != null) android.text.Selection.moveDown(text, layout)
                android.view.KeyEvent.KEYCODE_MOVE_HOME -> if (layout != null) android.text.Selection.moveToLeftEdge(text, layout)
                android.view.KeyEvent.KEYCODE_MOVE_END -> if (layout != null) android.text.Selection.moveToRightEdge(text, layout)
                else -> {
                    val codePoint = event.unicodeChar
                    if (codePoint != 0 && !Character.isISOControl(codePoint)) commitText(String(Character.toChars(codePoint)), 1)
                }
            }
            return true
        }

        // A multi-line draft has no editor action; the action key is a line break.
        override fun performEditorAction(actionCode: Int): Boolean = commitText("\n", 1)
    }

    companion object {
        private val nextImeRequest = java.util.concurrent.atomic.AtomicLong()
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
        private const val DOUBLE_SPACE_WINDOW_MS = 500L
        // Same hue family as OpenLessOverlayService's OverlayVisualState
        // (recording/processing), plus a light-green "ready" and an amber
        // "link issue" that overlay doesn't have.
        private val LINK_COLOR_READY = Color.rgb(134, 239, 172)
        // Ready, and the next dictation runs on the local model (asrRoute).
        private val LINK_COLOR_READY_LOCAL = Color.rgb(21, 128, 61)
        // Ready, local model and raw output: no usable polish channel or no network.
        private val LINK_COLOR_READY_LOCAL_RAW = Color.rgb(241, 245, 249)
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
        // How far on either side of the host caret an edited text's original is looked for.
        private const val HOST_SEARCH_REACH = 4000
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
