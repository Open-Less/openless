package com.openless.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView
import kotlin.math.abs

/** Presentation-only input surface: no service, engine, JNI or preferences. */
internal class ImeTypingPanel(
    context: Context,
    private val palette: ImePalette,
    header: View,
    private val onAction: (ImeKeyAction) -> Unit,
    private val onFeedback: () -> Unit,
    private val onVoiceStart: () -> Boolean,
    private val onVoiceEnd: (cancelled: Boolean) -> Unit,
    interfaceEnglish: Boolean = false,
    interactionPolicy: ImeInteractionPolicy = ImeInteractionPolicy.DEFAULT,
    private val onClearPreedit: () -> Unit = {},
    private val onCommitRawPreedit: () -> Unit = {},
    private val onCursorMove: (Int) -> Unit = {},
    private val onDeleteWord: () -> Unit = {},
) : LinearLayout(context) {
    internal data class Candidate(
        val label: String,
        val select: () -> Unit,
        val forget: (() -> Unit)? = null,
    )

    private var policy = interactionPolicy
    private val topSlot = FrameLayout(context)
    private val preeditBar = LinearLayout(context)
    private val preeditScroll = HorizontalScrollView(context)
    private val preeditLabel = label("", 15f)
    private val preeditClear = label("✕", 14f)
    private val preview = PreviewBubble(context)
    private val body = FrameLayout(context)
    private val grid = KeyGrid(context)
    private var candidateExpanded = false
    private val candidateStrip = ImeCandidateStrip(context, palette, interfaceEnglish) { expanded ->
        candidateExpanded = expanded
        updateGridAccessibility()
    }
    private val voiceSheet = LinearLayout(context)
    private val voiceMessage = label("", 16f)
    private val voiceWaveform = LevelView(context)
    private val voiceHint = label("", 13f)
    private val voiceCancel = label("取消 / Cancel", 15f)
    private var candidates = emptyList<Candidate>()
    private var candidatePreedit: String? = null
    private var english = interfaceEnglish
    private var currentLanguage = ImeTypingLanguage.ENGLISH
    private var currentLetterCase = ImeLetterCase.LOWER
    private var listening = false
    private var processing = false
    private var holdingVoice = false
    private var voiceCancelled = false
    private var lastLayout: List<List<ImeKeySpec>>? = null

    init {
        orientation = VERTICAL
        setBackgroundColor(palette.surface)
        clipChildren = false
        clipToPadding = false

        // Child 0 stays anchored during mode-switch slide animations.
        topSlot.addView(header, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        preeditLabel.apply {
            gravity = Gravity.CENTER_VERTICAL or Gravity.START
            setTextColor(palette.accent)
            setTypeface(Typeface.DEFAULT, Typeface.BOLD)
            setPadding(dp(12), 0, dp(12), 0)
            tag = "ime-cand-preedit"
            isClickable = true
            isFocusable = true
            setOnClickListener {
                if (!listening && !processing && !candidatePreedit.isNullOrEmpty()) {
                    onFeedback()
                    onCommitRawPreedit()
                }
            }
        }
        preeditScroll.apply {
            isHorizontalScrollBarEnabled = false
            overScrollMode = View.OVER_SCROLL_NEVER
            addView(preeditLabel, ViewGroup.LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        }
        preeditClear.apply {
            setTextColor(palette.secondaryText)
            tag = "ime-preedit-clear"
            contentDescription = if (english) "Clear pinyin" else "清除编码"
            isClickable = true
            isFocusable = true
            setOnClickListener {
                if (!listening && !processing) {
                    onFeedback()
                    onClearPreedit()
                }
            }
        }
        preeditBar.apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setBackgroundColor(palette.surface)
            isClickable = true
            visibility = GONE
            addView(preeditScroll, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f))
            addView(preeditClear, LayoutParams(dp(42), LayoutParams.MATCH_PARENT))
        }
        topSlot.addView(preeditBar, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        addView(topSlot, LayoutParams(LayoutParams.MATCH_PARENT, dp(40)))

        addView(candidateStrip, LayoutParams(LayoutParams.MATCH_PARENT, dp(42)))
        addView(View(context).apply { setBackgroundColor(palette.separator) }, LayoutParams(LayoutParams.MATCH_PARENT, dp(1)))

        body.addView(grid, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        body.addView(candidateStrip.expandedView, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        voiceSheet.apply {
            orientation = VERTICAL
            gravity = Gravity.CENTER
            setPadding(dp(16), dp(8), dp(16), dp(8))
            setBackgroundColor(palette.surface)
            isClickable = true
            visibility = GONE
            addView(voiceMessage, LayoutParams(LayoutParams.MATCH_PARENT, dp(36)))
            addView(voiceWaveform, LayoutParams(dp(200), 0, 1f))
            addView(voiceHint, LayoutParams(LayoutParams.MATCH_PARENT, dp(30)))
            addView(voiceCancel.apply {
                setTextColor(palette.error)
                background = GradientDrawable().apply {
                    cornerRadius = dp(18).toFloat()
                    setColor(palette.auxiliary)
                }
                setOnClickListener { grid.cancelTouch(); onVoiceEnd(true) }
            }, LayoutParams(dp(132), dp(38)).apply { topMargin = dp(4) })
        }
        body.addView(voiceSheet, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        body.clipChildren = false
        body.clipToPadding = false
        preview.visibility = GONE
        body.addView(preview, FrameLayout.LayoutParams(dp(54), dp(64)))
        addView(body, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
    }

    /** 下发统一互动策略：值变化时先中止旧触摸/计时器；相同值不影响进行中的交互。 */
    fun applyInteractionPolicy(policy: ImeInteractionPolicy) {
        if (policy == this.policy) return
        grid.cancelTouch()
        this.policy = policy
    }

    fun renderKeyboard(
        layer: ImeKeyboardLayer,
        language: ImeTypingLanguage,
        letterCase: ImeLetterCase,
        enterLabel: String,
        englishUi: Boolean,
    ) {
        english = englishUi
        currentLanguage = language
        currentLetterCase = letterCase
        preeditClear.contentDescription = if (english) "Clear pinyin" else "清除编码"
        val layout = ImeKeyboardLayouts.rows(layer, language, letterCase, enterLabel)
        if (layout != lastLayout) {
            lastLayout = layout
            grid.setKeys(layout, layer == ImeKeyboardLayer.LETTERS)
        } else {
            grid.refreshKeyVisuals()
        }
        if (candidates.isEmpty() && candidatePreedit.isNullOrEmpty()) {
            candidateStrip.setEmptyHint(if (english) "Hold space to speak" else "按住空格说话")
        }
    }

    fun submitCandidates(items: List<Candidate>, preedit: String?, statusHint: String? = null) {
        candidates = items
        candidatePreedit = preedit
        val hasPreedit = !preedit.isNullOrEmpty()
        preeditBar.visibility = if (hasPreedit) VISIBLE else GONE
        if (hasPreedit) {
            preeditLabel.text = preedit
            preeditLabel.contentDescription = preedit
            preeditScroll.post { preeditScroll.fullScroll(View.FOCUS_RIGHT) }
        }
        candidateStrip.submit(
            items.map { ImeCandidateStrip.Item(it.label, it.select, it.forget) },
            preedit = null,
            emptyHint = statusHint ?: if (english) "Hold space to speak" else "按住空格说话",
        )
        candidateStrip.setBlocked(listening || processing)
    }

    fun setClipboardSuggestion(label: String?, onPaste: () -> Unit, onDismiss: () -> Unit) {
        candidateStrip.setQuickChip(label, onPaste, onDismiss)
    }

    fun renderVoice(message: String, recording: Boolean, busy: Boolean, level: Float? = null, cancellable: Boolean = recording || busy) {
        listening = recording
        processing = busy
        voiceCancel.isEnabled = cancellable
        voiceCancel.alpha = if (cancellable) 1f else 0.4f
        voiceCancel.text = if (english) "Cancel" else "取消"
        voiceMessage.text = message
        if (!recording && !busy && !holdingVoice && message.isNotBlank() && candidates.isEmpty() && candidatePreedit.isNullOrEmpty()) {
            candidateStrip.setEmptyHint(message)
        }
        voiceSheet.visibility = if (holdingVoice || recording || busy) VISIBLE else GONE
        voiceHint.text = when {
            holdingVoice && voiceCancelled -> if (english) "Release to cancel" else "松开取消"
            holdingVoice -> if (english) "Release to insert · Slide up to cancel" else "松开输入 · 上滑取消"
            busy -> if (english) "Processing…" else "正在处理…"
            else -> ""
        }
        voiceHint.setTextColor(if (voiceCancelled) palette.error else palette.secondaryText)
        if (level != null) voiceWaveform.push(if (recording && level.isFinite()) level.coerceIn(0f, 1f) else 0f)
        if (!recording) voiceWaveform.clear()
        candidateStrip.setBlocked(recording || busy)
        updateGridAccessibility()
    }

    fun closeTransientUi() {
        grid.cancelTouch()
        candidateStrip.cancelInteractions()
    }

    private fun updateGridAccessibility() {
        grid.importantForAccessibility = if (candidateExpanded || holdingVoice || listening || processing) {
            IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        } else IMPORTANT_FOR_ACCESSIBILITY_AUTO
    }

    private fun label(value: String, size: Float) = TextView(context).apply {
        text = value
        textSize = size
        gravity = Gravity.CENTER
        setTextColor(palette.text)
        includeFontPadding = false
        maxLines = 1
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
    private fun dpf(value: Float) = value * resources.displayMetrics.density

    /** Floating popup preview rendered with exact FontMetrics centering. */
    private inner class PreviewBubble(context: Context) : View(context) {
        private var glyph: String = ""
        private var accentHighlight: Boolean = false
        private val bgPaint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val borderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
        }
        private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textAlign = Paint.Align.CENTER
            typeface = Typeface.DEFAULT_BOLD
        }
        private val rect = RectF()

        init {
            isClickable = false
            importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            elevation = dpf(8f)
        }

        fun show(value: String, accented: Boolean) {
            glyph = value
            accentHighlight = accented
            invalidate()
        }

        override fun onDraw(canvas: Canvas) {
            val radius = dpf(10f)
            val pad = dpf(1f)
            rect.set(pad, pad, width - pad, height - pad)
            bgPaint.color = if (accentHighlight) palette.accent else palette.key
            canvas.drawRoundRect(rect, radius, radius, bgPaint)
            borderPaint.color = palette.separator
            borderPaint.strokeWidth = dpf(1f)
            canvas.drawRoundRect(rect, radius, radius, borderPaint)

            textPaint.color = if (accentHighlight) Color.WHITE else palette.text
            textPaint.textSize = if (glyph.length > 1) dpf(20f) else dpf(30f)
            val fm = textPaint.fontMetrics
            val baseline = height / 2f - (fm.ascent + fm.descent) / 2f
            canvas.drawText(glyph, width / 2f, baseline, textPaint)
        }
    }

    /**
     * Custom Canvas-drawn keycap extending TextView so existing tests/accessibility
     * can read `.text`, while rendering stays mathematically centered regardless of
     * OEM fonts or system font scale.
     */
    private inner class KeyCapView(context: Context, var spec: ImeKeySpec) : TextView(context) {
        var cursorSlideActive = false
            set(value) {
                if (field != value) {
                    field = value
                    invalidate()
                }
            }
        var activeOverride: String? = null
            set(value) {
                if (field != value) {
                    field = value
                    invalidate()
                }
            }

        private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val shadowPaint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val primaryPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textAlign = Paint.Align.CENTER
        }
        private val secondaryPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textAlign = Paint.Align.RIGHT
        }
        private val iconPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            strokeCap = Paint.Cap.ROUND
            strokeJoin = Paint.Join.ROUND
        }
        private val rect = RectF()
        private val path = Path()

        init {
            setWillNotDraw(false)
            isFocusable = true
            isClickable = true
            gravity = Gravity.CENTER
            includeFontPadding = false
            maxLines = 1
            syncMetadata()
        }

        fun updateSpec(next: ImeKeySpec) {
            spec = next
            activeOverride = null
            syncMetadata()
            invalidate()
        }

        fun syncMetadata() {
            tag = "ime-key-${spec.id}"
            text = when {
                spec.action == ImeKeyAction.Space && !english -> "空格"
                else -> spec.label
            }
            contentDescription = accessibilityLabel(spec)
        }

        override fun setPressed(pressed: Boolean) {
            val changed = isPressed != pressed
            super.setPressed(pressed)
            if (changed) invalidate()
        }

        override fun onDraw(canvas: Canvas) {
            val w = width.toFloat()
            val h = height.toFloat()
            if (w <= 0f || h <= 0f) return

            val isEnter = spec.action == ImeKeyAction.Enter
            val isShiftActive = spec.action == ImeKeyAction.Shift && currentLetterCase != ImeLetterCase.LOWER
            val radius = dpf(6f)
            val shadowHeight = dpf(1.2f)

            // Subtle bottom keycap rim for crisp visual separation.
            shadowPaint.color = palette.separator
            rect.set(0f, shadowHeight, w, h)
            canvas.drawRoundRect(rect, radius, radius, shadowPaint)

            val normalColor = when {
                isEnter -> palette.accent
                isShiftActive -> palette.key
                spec.auxiliary -> palette.auxiliary
                else -> palette.key
            }
            fillPaint.color = when {
                isPressed && isEnter -> blendPressed(palette.accent)
                isPressed -> palette.pressed
                else -> normalColor
            }
            rect.set(0f, 0f, w, (h - shadowHeight).coerceAtLeast(1f))
            canvas.drawRoundRect(rect, radius, radius, fillPaint)

            val cx = w / 2f
            val cy = rect.centerY()

            when (spec.action) {
                ImeKeyAction.Backspace -> {
                    drawBackspaceIcon(canvas, cx, cy, minOf(w, h))
                    return
                }
                ImeKeyAction.Shift -> {
                    drawShiftIcon(canvas, cx, cy, minOf(w, h))
                    return
                }
                ImeKeyAction.Language -> {
                    drawLanguageBadge(canvas, cx, cy, w, h)
                    return
                }
                else -> Unit
            }

            val displayText = when {
                cursorSlideActive && spec.action == ImeKeyAction.Space -> if (english) "◀ Cursor ▶" else "◀ 移动光标 ▶"
                activeOverride != null -> activeOverride!!
                spec.action == ImeKeyAction.Space -> if (english) "Space" else "空格"
                else -> spec.label
            }

            // Secondary hint drawn independently in upper-right corner (never pushes main glyph off-center).
            val secondary = if (activeOverride == null) spec.secondary else null
            if (secondary != null && h >= dpf(28f)) {
                secondaryPaint.color = palette.secondaryText
                secondaryPaint.alpha = 185
                secondaryPaint.textSize = minOf(dpf(9.5f), h * 0.21f).coerceAtLeast(dpf(7.5f))
                val sfm = secondaryPaint.fontMetrics
                canvas.drawText(secondary, w - dpf(4.5f), dpf(2.8f) - sfm.ascent, secondaryPaint)
            }

            primaryPaint.color = when {
                isEnter -> Color.WHITE
                cursorSlideActive && spec.action == ImeKeyAction.Space -> palette.accent
                else -> palette.text
            }
            primaryPaint.typeface = if (isEnter || spec.action == ImeKeyAction.Space) Typeface.DEFAULT_BOLD else Typeface.DEFAULT
            val isSingleChar = displayText.length == 1 && !spec.auxiliary && spec.action !is ImeKeyAction.Space
            val baseSize = when {
                isSingleChar -> minOf(dpf(21f), h * 0.46f)
                displayText.length <= 2 -> minOf(dpf(16f), h * 0.36f)
                else -> minOf(dpf(14f), h * 0.32f)
            }.coerceAtLeast(dpf(10.5f))
            primaryPaint.textSize = baseSize

            // Shrink horizontally if needed so wide labels ("123", "Return", "下一项") never clip.
            val maxTextWidth = (w - dpf(8f)).coerceAtLeast(dpf(10f))
            val measured = primaryPaint.measureText(displayText)
            if (measured > maxTextWidth && measured > 0f) {
                primaryPaint.textSize = (baseSize * (maxTextWidth / measured)).coerceAtLeast(dpf(9.5f))
            }

            val fm = primaryPaint.fontMetrics
            val opticalShift = if (secondary != null) dpf(2.2f) else 0f
            val baseline = cy + opticalShift - (fm.ascent + fm.descent) / 2f
            canvas.drawText(displayText, cx, baseline, primaryPaint)
        }

        private fun drawBackspaceIcon(canvas: Canvas, cx: Float, cy: Float, side: Float) {
            val scale = (side * 0.22f).coerceIn(dpf(7f), dpf(11.5f))
            iconPaint.color = palette.text
            iconPaint.style = Paint.Style.STROKE
            iconPaint.strokeWidth = dpf(1.8f)
            path.reset()
            path.moveTo(cx - scale * 1.15f, cy)
            path.lineTo(cx - scale * 0.45f, cy - scale * 0.78f)
            path.lineTo(cx + scale * 1.15f, cy - scale * 0.78f)
            path.lineTo(cx + scale * 1.15f, cy + scale * 0.78f)
            path.lineTo(cx - scale * 0.45f, cy + scale * 0.78f)
            path.close()
            canvas.drawPath(path, iconPaint)

            val xOff = scale * 0.34f
            val xCenter = cx + scale * 0.30f
            canvas.drawLine(xCenter - xOff, cy - xOff, xCenter + xOff, cy + xOff, iconPaint)
            canvas.drawLine(xCenter + xOff, cy - xOff, xCenter - xOff, cy + xOff, iconPaint)
        }

        private fun drawShiftIcon(canvas: Canvas, cx: Float, cy: Float, side: Float) {
            val s = (side * 0.21f).coerceIn(dpf(7f), dpf(11f))
            val filled = currentLetterCase != ImeLetterCase.LOWER
            val caps = currentLetterCase == ImeLetterCase.CAPS_LOCK
            val yOffset = if (caps) -dpf(1.8f) else 0f
            val center = cy + yOffset
            path.reset()
            path.moveTo(cx, center - s * 1.05f)
            path.lineTo(cx + s * 1.05f, center + s * 0.05f)
            path.lineTo(cx + s * 0.48f, center + s * 0.05f)
            path.lineTo(cx + s * 0.48f, center + s * 0.88f)
            path.lineTo(cx - s * 0.48f, center + s * 0.88f)
            path.lineTo(cx - s * 0.48f, center + s * 0.05f)
            path.lineTo(cx - s * 1.05f, center + s * 0.05f)
            path.close()
            iconPaint.color = if (filled) palette.accent else palette.text
            iconPaint.style = if (filled) Paint.Style.FILL_AND_STROKE else Paint.Style.STROKE
            iconPaint.strokeWidth = dpf(1.7f)
            canvas.drawPath(path, iconPaint)
            if (caps) {
                iconPaint.style = Paint.Style.STROKE
                iconPaint.strokeWidth = dpf(2.2f)
                canvas.drawLine(cx - s * 0.65f, center + s * 1.32f, cx + s * 0.65f, center + s * 1.32f, iconPaint)
            }
        }

        private fun drawLanguageBadge(canvas: Canvas, cx: Float, cy: Float, w: Float, h: Float) {
            val pinyin = currentLanguage == ImeTypingLanguage.PINYIN
            val main = if (pinyin) "中" else "英"
            val sub = if (pinyin) "/英" else "/中"
            val mainSize = minOf(dpf(15.5f), h * 0.36f, w * 0.36f).coerceAtLeast(dpf(10.5f))
            val subSize = (mainSize * 0.72f).coerceAtLeast(dpf(8.5f))

            primaryPaint.textAlign = Paint.Align.LEFT
            primaryPaint.typeface = Typeface.DEFAULT_BOLD
            primaryPaint.color = palette.text
            primaryPaint.textSize = mainSize
            val mainW = primaryPaint.measureText(main)

            secondaryPaint.textAlign = Paint.Align.LEFT
            secondaryPaint.alpha = 255
            secondaryPaint.color = palette.secondaryText
            secondaryPaint.textSize = subSize
            val subW = secondaryPaint.measureText(sub)

            val totalW = mainW + subW
            val startX = cx - totalW / 2f
            val fm = primaryPaint.fontMetrics
            val baseline = cy - (fm.ascent + fm.descent) / 2f
            canvas.drawText(main, startX, baseline, primaryPaint)
            canvas.drawText(sub, startX + mainW, baseline, secondaryPaint)

            // Restore alignments for subsequent uses.
            primaryPaint.textAlign = Paint.Align.CENTER
            secondaryPaint.textAlign = Paint.Align.RIGHT
        }

        private fun blendPressed(color: Int): Int {
            val r = ((Color.red(color) * 85) / 100).coerceIn(0, 255)
            val g = ((Color.green(color) * 85) / 100).coerceIn(0, 255)
            val b = ((Color.blue(color) * 85) / 100).coerceIn(0, 255)
            return Color.argb(255, r, g, b)
        }
    }

    private fun accessibilityLabel(spec: ImeKeySpec): String = when (spec.action) {
        ImeKeyAction.Backspace -> if (english) "Backspace" else "退格"
        ImeKeyAction.Shift -> if (spec.label == "⇪") {
            if (english) "Caps lock" else "大写锁定"
        } else if (english) "Shift" else "大小写"
        ImeKeyAction.Space -> if (english) "Space, hold to speak" else "空格，按住说话"
        ImeKeyAction.Language -> if (english) "Switch language" else "切换中英文"
        else -> spec.label
    }

    /** Owns key hit testing, multi-touch rollover, swipe gestures, timers; children remain accessible buttons. */
    private inner class KeyGrid(context: Context) : ViewGroup(context) {
        private val handler = Handler(Looper.getMainLooper())
        private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop.toFloat()
        private var rows = emptyList<List<ImeKeySpec>>()
        private val keys = linkedMapOf<KeyCapView, ImeKeySpec>()
        private fun specOf(view: KeyCapView?): ImeKeySpec? = if (view == null) null else keys[view]
        private var insetSecondRow = false
        private var pressed: KeyCapView? = null
        private var downX = 0f
        private var downY = 0f
        private var longPressed = false
        private var swipeUpSecondary = false
        private var cursorSliding = false
        private var cursorAnchorX = 0f
        private var backspaceSwiping = false
        private var repeating = false
        private var pointerActive = false
        private var voiceRejected = false
        private var activePointer = -1
        private val repeat = object : Runnable {
            override fun run() {
                if (pointerActive && specOf(pressed)?.action == ImeKeyAction.Backspace && !backspaceSwiping && isAttachedToWindow) {
                    repeating = true
                    onAction(ImeKeyAction.Backspace)
                    onFeedback()
                    handler.postDelayed(this, policy.repeatIntervalMs)
                }
            }
        }
        private val hold = Runnable {
            val spec = specOf(pressed)
            if (pointerActive && spec?.action == ImeKeyAction.Space && !cursorSliding && isAttachedToWindow) {
                longPressed = true
                // Set before invoking the host, whose callbacks may render synchronously.
                holdingVoice = true
                voiceRejected = !onVoiceStart()
                if (voiceRejected) {
                    holdingVoice = false
                    android.widget.Toast.makeText(context, voiceMessage.text, android.widget.Toast.LENGTH_SHORT).show()
                }
                renderVoice(voiceMessage.text.toString(), listening, processing)
            } else if (pointerActive && spec?.secondary != null && !swipeUpSecondary && isAttachedToWindow) {
                longPressed = true
                pressed?.activeOverride = spec.secondary
                showPreview(pressed, spec.secondary, accented = true)
                onFeedback()
            }
        }

        init {
            clipChildren = false
            clipToPadding = false
        }

        fun refreshKeyVisuals() {
            for (i in 0 until childCount) {
                (getChildAt(i) as? KeyCapView)?.apply {
                    syncMetadata()
                    invalidate()
                }
            }
        }

        fun setKeys(layout: List<List<ImeKeySpec>>, inset: Boolean) {
            val sameGeometry = insetSecondRow == inset &&
                rows.map { row -> row.map { it.id to it.weight } } == layout.map { row -> row.map { it.id to it.weight } }
            if (sameGeometry && childCount == layout.sumOf { it.size }) {
                rows = layout
                layout.flatten().forEachIndexed { index, spec ->
                    val key = getChildAt(index) as KeyCapView
                    keys[key] = spec
                    val preserveSecondary = key === pressed && (longPressed || swipeUpSecondary) && spec.secondary != null
                    key.updateSpec(spec)
                    if (preserveSecondary) key.activeOverride = spec.secondary
                }
                if (pressed != null && !longPressed && !swipeUpSecondary) showPreview(pressed)
                return
            }
            cancelTouch()
            removeAllViews()
            keys.clear()
            rows = layout
            insetSecondRow = inset
            layout.flatten().forEach { spec ->
                val key = KeyCapView(context, spec).apply {
                    setOnClickListener {
                        if (!listening && !processing) keys[this]?.let { onAction(it.action) }
                    }
                }
                keys[key] = spec
                addView(key)
            }
        }

        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
            val width = MeasureSpec.getSize(widthMeasureSpec)
            val height = MeasureSpec.getSize(heightMeasureSpec)
            val rowHeight = height.toFloat() / rows.size.coerceAtLeast(1)
            var index = 0
            rows.forEachIndexed { rowIndex, row ->
                val unit = width.toFloat() / (row.sumOf { it.weight.toDouble() }.toFloat() + if (insetSecondRow && rowIndex == 1) 1f else 0f)
                row.forEach { spec ->
                    val key = getChildAt(index++)
                    val keyWidth = (unit * spec.weight - dp(5)).toInt().coerceAtLeast(1)
                    val keyHeight = (rowHeight - dp(7)).toInt().coerceAtLeast(1)
                    key.measure(
                        MeasureSpec.makeMeasureSpec(keyWidth, MeasureSpec.EXACTLY),
                        MeasureSpec.makeMeasureSpec(keyHeight, MeasureSpec.EXACTLY),
                    )
                }
            }
            setMeasuredDimension(width, height)
        }

        override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
            val rowHeight = height.toFloat() / rows.size.coerceAtLeast(1)
            var index = 0
            rows.forEachIndexed { rowIndex, row ->
                val inset = insetSecondRow && rowIndex == 1
                val unit = width.toFloat() / (row.sumOf { it.weight.toDouble() }.toFloat() + if (inset) 1f else 0f)
                var x = if (inset) unit / 2 else 0f
                row.forEach { spec ->
                    val key = getChildAt(index++)
                    val keyLeft = (x + dp(2)).toInt()
                    val keyTop = (rowIndex * rowHeight + dp(3)).toInt()
                    key.layout(keyLeft, keyTop, keyLeft + key.measuredWidth, keyTop + key.measuredHeight)
                    x += unit * spec.weight
                }
            }
        }

        override fun onInterceptTouchEvent(event: MotionEvent) = true

        override fun onTouchEvent(event: MotionEvent): Boolean {
            if (event.actionMasked == MotionEvent.ACTION_CANCEL) {
                cancelTouch()
                return true
            }
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    cancelTouch()
                    if (listening || processing) return true
                    beginPointer(event.getPointerId(0), event.x, event.y)
                }
                MotionEvent.ACTION_POINTER_DOWN -> {
                    if (listening || processing || holdingVoice) return true
                    // Multi-touch rollover: commit the held text key immediately and track the new finger.
                    val newIndex = event.actionIndex
                    val newPointerId = event.getPointerId(newIndex)
                    val newX = event.getX(newIndex)
                    val newY = event.getY(newIndex)
                    val prevSpec = specOf(pressed)
                    val prevSecondary = swipeUpSecondary || longPressed
                    val prevCanRollover = pointerActive && !cursorSliding && !backspaceSwiping && !repeating &&
                        !voiceRejected && prevSpec != null && prevSpec.action is ImeKeyAction.Text
                    if (prevCanRollover) {
                        finishTouch()
                        if (prevSecondary && prevSpec.secondary != null) {
                            onAction(ImeKeyAction.Text(prevSpec.secondary))
                        } else {
                            onAction(prevSpec.action)
                        }
                        beginPointer(newPointerId, newX, newY)
                    } else if (!cursorSliding && !backspaceSwiping) {
                        cancelTouch()
                    }
                }
                MotionEvent.ACTION_MOVE -> {
                    if (!pointerActive) return true
                    val index = event.findPointerIndex(activePointer)
                    if (index < 0) return true
                    val x = event.getX(index)
                    val y = event.getY(index)
                    val spec = specOf(pressed)

                    if (holdingVoice) {
                        voiceCancelled = downY - y > dp(48)
                        renderVoice(voiceMessage.text.toString(), listening, processing)
                    } else if (spec?.action == ImeKeyAction.Space) {
                        val dx = x - downX
                        val dy = y - downY
                        if (!cursorSliding && !longPressed && abs(dx) > maxOf(touchSlop * 1.4f, dpf(14f)) && abs(dx) > abs(dy) * 1.2f) {
                            handler.removeCallbacks(hold)
                            cursorSliding = true
                            cursorAnchorX = downX
                            pressed?.cursorSlideActive = true
                        }
                        if (cursorSliding) {
                            val step = dpf(14f).coerceAtLeast(1f)
                            while (x - cursorAnchorX >= step) {
                                cursorAnchorX += step
                                onCursorMove(1)
                                onFeedback()
                            }
                            while (cursorAnchorX - x >= step) {
                                cursorAnchorX -= step
                                onCursorMove(-1)
                                onFeedback()
                            }
                        }
                    } else if (spec?.action == ImeKeyAction.Backspace) {
                        val dx = x - downX
                        val dy = y - downY
                        if (!repeating && !backspaceSwiping && -dx > dpf(38f) && abs(dx) > abs(dy) * 1.3f) {
                            handler.removeCallbacks(repeat)
                            backspaceSwiping = true
                            onFeedback()
                        }
                    } else if (!longPressed && !repeating) {
                        val dx = x - downX
                        val dy = y - downY
                        if (spec?.secondary != null && -dy > maxOf(dpf(26f), touchSlop * 1.8f) && abs(dy) > abs(dx) * 1.15f) {
                            if (!swipeUpSecondary) {
                                handler.removeCallbacks(hold)
                                swipeUpSecondary = true
                                pressed?.activeOverride = spec.secondary
                                showPreview(pressed, spec.secondary, accented = true)
                                onFeedback()
                            }
                        } else if (swipeUpSecondary) {
                            if (-dy < dpf(14f)) {
                                swipeUpSecondary = false
                                pressed?.activeOverride = null
                                showPreview(pressed)
                            }
                        } else {
                            val target = hit(x, y)
                            if (target != pressed) {
                                handler.removeCallbacks(hold)
                                handler.removeCallbacks(repeat)
                                pressed?.isPressed = false
                                pressed?.activeOverride = null
                                // Only letter-to-letter drags retarget; functional-key drags cancel.
                                pressed = if (specOf(pressed)?.action is ImeKeyAction.Text && specOf(target)?.action is ImeKeyAction.Text) target else null
                                pressed?.isPressed = true
                                showPreview(pressed)
                            }
                        }
                    }
                }
                MotionEvent.ACTION_POINTER_UP -> {
                    val liftedIndex = event.actionIndex
                    val liftedId = event.getPointerId(liftedIndex)
                    if (pointerActive && liftedId == activePointer) {
                        completeActivePointer(event.getX(liftedIndex), event.getY(liftedIndex))
                    }
                }
                MotionEvent.ACTION_UP -> {
                    if (!pointerActive) return true
                    val index = event.findPointerIndex(activePointer)
                    if (index < 0) {
                        finishTouch()
                        return true
                    }
                    completeActivePointer(event.getX(index), event.getY(index))
                }
            }
            return true
        }

        private fun beginPointer(pointerId: Int, x: Float, y: Float) {
            activePointer = pointerId
            pointerActive = true
            downX = x
            downY = y
            pressed = hit(x, y)
            pressed?.isPressed = true
            showPreview(pressed)
            parent?.requestDisallowInterceptTouchEvent(true)
            onFeedback()
            when (specOf(pressed)?.action) {
                ImeKeyAction.Backspace -> handler.postDelayed(repeat, ViewConfiguration.getLongPressTimeout().toLong())
                ImeKeyAction.Space -> handler.postDelayed(hold, ViewConfiguration.getLongPressTimeout().toLong())
                is ImeKeyAction.Text -> if (specOf(pressed)?.secondary != null) {
                    handler.postDelayed(hold, ViewConfiguration.getLongPressTimeout().toLong())
                }
                else -> Unit
            }
        }

        private fun completeActivePointer(x: Float, y: Float) {
            val spec = specOf(pressed)
            val wasVoice = holdingVoice
            val cancelled = voiceCancelled
            val didRepeat = repeating
            val wasLongPress = longPressed
            val wasSwipeSecondary = swipeUpSecondary
            val wasCursorSlide = cursorSliding
            val wasBackspaceSwipe = backspaceSwiping
            val rejected = voiceRejected
            val terminal = hit(x, y)
            val hadTarget = terminal == pressed && pressed != null
            finishTouch()
            when {
                wasVoice -> onVoiceEnd(cancelled)
                wasCursorSlide -> Unit
                wasBackspaceSwipe -> onDeleteWord()
                wasSwipeSecondary && spec?.secondary != null -> onAction(ImeKeyAction.Text(spec.secondary))
                !rejected && hadTarget && spec != null && !didRepeat -> {
                    if (wasLongPress && spec.secondary != null) onAction(ImeKeyAction.Text(spec.secondary))
                    else if (!wasLongPress) onAction(spec.action)
                }
            }
        }

        private fun showPreview(key: KeyCapView?, override: String? = null, accented: Boolean = false) {
            val spec = specOf(key)
            if (key == null || spec?.action !is ImeKeyAction.Text || holdingVoice) {
                preview.visibility = GONE
                return
            }
            val text = override ?: spec.label
            preview.show(text, accented)
            preview.x = (key.left + key.width / 2f - dp(27)).coerceIn(0f, (width - dp(54)).coerceAtLeast(0).toFloat())
            // Float above Row 1 into the candidate strip area instead of covering the key.
            preview.y = (key.top - dp(58)).toFloat()
            preview.visibility = VISIBLE
            preview.bringToFront()
        }

        private fun hit(x: Float, y: Float): KeyCapView? {
            if (x < 0 || y < 0 || x >= width || y >= height) return null
            // Include the visual gaps in the nearest key's target without overlapping rows.
            val rowIndex = (y * rows.size / height.coerceAtLeast(1)).toInt().coerceIn(0, rows.lastIndex.coerceAtLeast(0))
            val row = rows.getOrNull(rowIndex) ?: return null
            val offset = rows.take(rowIndex).sumOf { it.size }
            return row.indices.mapNotNull { getChildAt(offset + it) as? KeyCapView }
                .minByOrNull { abs(x - (it.left + it.right) / 2f) }
        }

        private fun finishTouch() {
            handler.removeCallbacks(hold)
            handler.removeCallbacks(repeat)
            pressed?.isPressed = false
            pressed?.activeOverride = null
            pressed?.cursorSlideActive = false
            preview.visibility = GONE
            pressed = null
            pointerActive = false
            activePointer = -1
            longPressed = false
            swipeUpSecondary = false
            cursorSliding = false
            backspaceSwiping = false
            repeating = false
            holdingVoice = false
            voiceCancelled = false
            voiceRejected = false
        }

        fun cancelTouch() {
            val wasVoice = holdingVoice
            finishTouch()
            if (wasVoice) onVoiceEnd(true)
        }

        override fun onDetachedFromWindow() {
            cancelTouch()
            super.onDetachedFromWindow()
        }
    }

    /** Real sample history, not a time-driven synthetic sine wave. */
    private inner class LevelView(context: Context) : View(context) {
        private val samples = FloatArray(25)
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = palette.accent; strokeCap = Paint.Cap.ROUND }
        fun push(value: Float) {
            samples.copyInto(samples, 0, 1)
            samples[samples.lastIndex] = if (value.isFinite()) value.coerceIn(0f, 1f) else 0f
            invalidate()
        }
        fun clear() { samples.fill(0f); invalidate() }
        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            paint.strokeWidth = dp(3).toFloat()
            val spacing = width.toFloat() / samples.size
            samples.forEachIndexed { index, value ->
                val amplitude = if (value < 0.02f) 0f else value
                val halfHeight = (height * 0.42f * amplitude).coerceAtLeast(dp(1).toFloat())
                val x = spacing * (index + 0.5f)
                canvas.drawLine(x, height / 2f - halfHeight, x, height / 2f + halfHeight, paint)
            }
        }
    }
}
