package com.openless.app

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import kotlin.math.abs
import kotlin.math.min

/**
 * 新版独立语音面板与其主按钮。
 *
 * 只做展示与手势判定：不持有 Service、不读 InputConnection、不触达 JNI，
 * 所有业务动作都通过构造参数里的回调交给宿主。
 *
 * 手势（均在 UP 时提交一次，阈值取系统 touch slop 与 28dp 的较大值）：
 * - 轻点 → TAP
 * - 上滑 → RAW
 * - 左滑 → QUICK_NOTE
 * - 右滑 → CLOUD_NOTE
 * - 下滑 → CANCEL（仅录音中；非录音时忽略）
 * ACTION_CANCEL / 多指只清除当前手势，不产生任何手势回调。
 */
internal enum class ImeVoiceGesture { TAP, RAW, QUICK_NOTE, CLOUD_NOTE, REVIEW_CLOUD, CANCEL }

internal class ImeVoiceButton(
    context: Context,
    private val palette: ImePalette,
    private val onGesture: (ImeVoiceGesture) -> Unit,
) : View(context) {
    var isRecording: Boolean = false
        set(value) {
            if (field == value) return
            field = value
            if (!value) {
                samples.fill(0f)
                sampleCount = 0
            }
            pressedState = false
            invalidate()
        }

    var isProcessing: Boolean = false
        set(value) {
            if (field == value) return
            field = value
            invalidate()
        }

    var rawModeActive: Boolean = false
        set(value) {
            if (field == value) return
            field = value
            invalidate()
        }

    var quickNoteActive: Boolean = false
        set(value) {
            if (field == value) return
            field = value
            invalidate()
        }

    var cloudNoteActive: Boolean = false
        set(value) {
            if (field == value) return
            field = value
            invalidate()
        }

    /** 由宿主按真实采集电平写入（0..1）；写入即进入环形历史，无写入则保持静止。 */
    var audioLevel: Float = 0f
        set(value) {
            val level = if (value.isFinite()) value.coerceIn(0f, 1f) else 0f
            field = level
            if (isRecording) pushSample(level)
            invalidate()
        }

    private val samples = FloatArray(SAMPLE_COUNT)
    private var sampleCount = 0
    private var pressedState = false
    private var processingPhase = 0

    private var downX = 0f
    private var downY = 0f
    private var pointerId = -1
    private var tracking = false
    private var gestureCommitted = false

    private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val strokePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val glyphPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val glyphStroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
    }

    init {
        setWillNotDraw(false)
        isClickable = true
        isFocusable = true
        contentDescription = "语音输入 / Voice input"
    }

    private fun pushSample(value: Float) {
        samples.copyInto(samples, 0, 1)
        samples[samples.lastIndex] = value
        if (sampleCount < SAMPLE_COUNT) sampleCount++
    }

    // --- 手势 ---

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!isEnabled) { clearGesture(); return true }
        if (event.pointerCount != 1) {
            // 多指：只清除当前手势，绝不误提交。
            clearGesture()
            return true
        }
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                clearGesture()
                downX = event.x
                downY = event.y
                pointerId = event.getPointerId(0)
                tracking = true
                pressedState = true
                invalidate()
                return true
            }

            MotionEvent.ACTION_MOVE -> return tracking

            MotionEvent.ACTION_UP -> {
                if (!tracking || event.getPointerId(0) != pointerId) {
                    clearGesture()
                    return true
                }
                val gesture = resolveGesture(event.x - downX, event.y - downY)
                clearGesture()
                if (gesture != null && !gestureCommitted) {
                    gestureCommitted = true
                    if (gesture == ImeVoiceGesture.TAP) performClick() else onGesture(gesture)
                }
                return true
            }

            MotionEvent.ACTION_CANCEL -> {
                clearGesture()
                return true
            }
        }
        return super.onTouchEvent(event)
    }

    override fun performClick(): Boolean {
        if (!isEnabled) return false
        super.performClick()
        onGesture(ImeVoiceGesture.TAP)
        return true
    }

    private fun resolveGesture(dx: Float, dy: Float): ImeVoiceGesture? {
        val threshold = maxOf(ViewConfiguration.get(context).scaledTouchSlop.toFloat(), dp(28).toFloat())
        val horizontal = abs(dx)
        val vertical = abs(dy)
        return when {
            horizontal < threshold && vertical < threshold -> ImeVoiceGesture.TAP
            vertical >= horizontal && dy < 0f -> if (isRecording && cloudNoteActive) ImeVoiceGesture.REVIEW_CLOUD else ImeVoiceGesture.RAW
            vertical >= horizontal && dy > 0f -> if (isRecording) ImeVoiceGesture.CANCEL else null
            dx < 0f -> ImeVoiceGesture.QUICK_NOTE
            else -> ImeVoiceGesture.CLOUD_NOTE
        }
    }

    private fun clearGesture() {
        tracking = false
        gestureCommitted = false
        pointerId = -1
        pressedState = false
        invalidate()
    }

    override fun onDetachedFromWindow() {
        clearGesture()
        super.onDetachedFromWindow()
    }

    // --- 绘制 ---

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val cx = width / 2f
        val cy = height / 2f
        val radius = min(width, height) / 2f - dp(5)
        if (radius <= 0f) return

        fillPaint.color = if (pressedState) palette.pressed else palette.key
        canvas.drawCircle(cx, cy, radius, fillPaint)

        strokePaint.color = ringColor()
        strokePaint.strokeWidth = dp(3).toFloat()
        canvas.drawCircle(cx, cy, radius - dp(2), strokePaint)

        when {
            isProcessing -> drawProcessing(canvas, cx, cy, radius)
            isRecording || sampleCount > 0 -> drawWaveform(canvas, cx, cy, radius)
            else -> drawMicrophone(canvas, cx, cy, radius)
        }
    }

    private fun ringColor(): Int = when {
        cloudNoteActive -> palette.error
        rawModeActive || quickNoteActive || isRecording -> palette.accent
        else -> palette.separator
    }

    private fun drawMicrophone(canvas: Canvas, cx: Float, cy: Float, radius: Float) {
        val bodyWidth = radius * 0.34f
        val bodyHeight = radius * 0.56f
        glyphPaint.color = palette.text
        val body = RectF(
            cx - bodyWidth / 2f,
            cy - radius * 0.30f,
            cx + bodyWidth / 2f,
            cy - radius * 0.30f + bodyHeight,
        )
        canvas.drawRoundRect(body, bodyWidth / 2f, bodyWidth / 2f, glyphPaint)

        glyphStroke.color = palette.text
        glyphStroke.strokeWidth = dp(2).toFloat()
        val cradle = RectF(
            cx - bodyWidth * 0.85f,
            cy - radius * 0.18f,
            cx + bodyWidth * 0.85f,
            cy + radius * 0.22f,
        )
        canvas.drawArc(cradle, 0f, 180f, false, glyphStroke)
        canvas.drawLine(cx, cy + radius * 0.22f, cx, cy + radius * 0.34f, glyphStroke)
    }

    private fun drawWaveform(canvas: Canvas, cx: Float, cy: Float, radius: Float) {
        if (sampleCount == 0) return
        val span = radius * 1.28f
        val step = span / SAMPLE_COUNT
        val startX = cx - span / 2f + step / 2f
        val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = palette.accent
            style = Paint.Style.STROKE
            strokeCap = Paint.Cap.ROUND
            strokeWidth = (step * 0.5f).coerceAtLeast(dp(2).toFloat())
        }
        val maxHalf = radius * 0.52f
        for (index in 0 until SAMPLE_COUNT) {
            val value = samples[index]
            val half = (value * maxHalf).coerceIn(dp(1).toFloat(), maxHalf)
            val x = startX + step * index
            canvas.drawLine(x, cy - half, x, cy + half, stroke)
        }
    }

    private fun drawProcessing(canvas: Canvas, cx: Float, cy: Float, radius: Float) {
        val animatorsEnabled = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            ValueAnimator.areAnimatorsEnabled()
        } else {
            true
        }
        val dotRadius = dp(5).toFloat()
        val gap = radius * 0.42f
        glyphPaint.color = palette.accent
        for (index in 0 until 3) {
            val alpha = if (animatorsEnabled) {
                val distance = (processingPhase - index + 3) % 3
                when (distance) {
                    0 -> 255
                    1 -> 150
                    else -> 80
                }
            } else {
                170
            }
            glyphPaint.alpha = alpha
            val x = cx + (index - 1) * gap
            canvas.drawCircle(x, cy, dotRadius, glyphPaint)
        }
        glyphPaint.alpha = 255
        if (animatorsEnabled && isAttachedToWindow) {
            processingPhase = (processingPhase + 1) % 3
            postInvalidateDelayed(120L)
        }
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private companion object {
        const val SAMPLE_COUNT = 21
    }
}

internal class ImeVoicePanel(
    context: Context,
    private val palette: ImePalette,
    header: View,
    private val englishUi: Boolean,
    private val onVoiceGesture: (ImeVoiceGesture) -> Unit,
    private val onUndo: () -> Unit,
    private val onEdit: () -> Unit,
    private val onEnter: () -> Unit,
    private val onBackspace: () -> Unit,
    private val onAt: () -> Unit,
    private val onRestart: () -> Unit,
    private val onFeedback: () -> Unit = {},
    interactionPolicy: ImeInteractionPolicy = ImeInteractionPolicy.DEFAULT,
) : LinearLayout(context) {

    val status: TextView
    val modeHint: TextView
    val warning: TextView
    val voiceButton: ImeVoiceButton
    val undo: TextView
    val edit: TextView
    val resultRow: LinearLayout
    val cancel: TextView

    private val body: FrameLayout
    private val modeHintRow: LinearLayout
    private val handler = Handler(Looper.getMainLooper())
    private var policy = interactionPolicy
    private var backspaceRepeating = false
    private var backspacePressed = false
    private var suppressFooterClickFeedback = false
    private val footerKeys = ArrayList<TextView>()
    private val modeKeys = ArrayList<TextView>()

    init {
        orientation = VERTICAL
        setBackgroundColor(palette.surface)

        addView(header, LayoutParams(LayoutParams.MATCH_PARENT, dp(40)))

        status = textView("", 13f, palette.secondaryText).apply {
            contentDescription = if (englishUi) "Voice status" else "语音状态"
        }
        addView(status, LayoutParams(LayoutParams.MATCH_PARENT, dp(30)))

        modeHint = textView(defaultModeHint(), 11f, palette.secondaryText)
        modeHintRow = LinearLayout(context).apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER
            addView(modeHint, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        }
        addView(modeHintRow, LayoutParams(LayoutParams.MATCH_PARENT, dp(22)))

        val chips = LinearLayout(context).apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER
            setPadding(dp(8), dp(2), dp(8), dp(2))
        }
        fun chipParams() = LayoutParams(LayoutParams.WRAP_CONTENT, dp(28)).apply {
            marginStart = dp(4)
            marginEnd = dp(4)
        }
        chips.addView(modeChip(if (englishUi) "RAW" else "RAW") { onVoiceGesture(ImeVoiceGesture.RAW) }, chipParams())
        chips.addView(modeChip(if (englishUi) "Quick note" else "速记") {
            onVoiceGesture(ImeVoiceGesture.QUICK_NOTE)
        }, chipParams())
        chips.addView(modeChip(if (englishUi) "Cloud note" else "云笔记") {
            onVoiceGesture(ImeVoiceGesture.CLOUD_NOTE)
        }, chipParams())
        cancel = chip(if (englishUi) "Cancel" else "取消") { onVoiceGesture(ImeVoiceGesture.CANCEL) }
        cancel.contentDescription = if (englishUi) "Cancel dictation" else "取消听写"
        cancel.isEnabled = false
        cancel.alpha = 0.4f
        chips.addView(cancel, chipParams())
        addView(chips, LayoutParams(LayoutParams.MATCH_PARENT, dp(34)))

        body = FrameLayout(context)
        voiceButton = ImeVoiceButton(context, palette, onVoiceGesture).apply {
            contentDescription = if (englishUi) "Voice input" else "语音输入"
        }
        body.addView(
            voiceButton,
            FrameLayout.LayoutParams(dp(150), dp(150), Gravity.CENTER),
        )

        warning = textView(defaultWarning(), 12f, palette.error).apply {
            gravity = Gravity.CENTER
            visibility = GONE
            isClickable = true
            isFocusable = true
            setPadding(dp(8), dp(2), dp(8), dp(2))
            contentDescription = defaultWarning()
            setOnClickListener { onRestart() }
        }
        body.addView(
            warning,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.TOP or Gravity.CENTER_HORIZONTAL,
            ),
        )

        undo = actionText(if (englishUi) "Undo" else "撤销", onUndo)
        edit = actionText(if (englishUi) "Edit" else "编辑", onEdit)
        resultRow = LinearLayout(context).apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER
            visibility = GONE
            setPadding(dp(8), dp(2), dp(8), dp(2))
            background = GradientDrawable().apply {
                cornerRadius = dp(14).toFloat()
                setColor(palette.auxiliary)
            }
            addView(undo, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT))
            addView(edit, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT))
        }
        body.addView(
            resultRow,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL,
            ),
        )
        addView(body, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))

        val footer = LinearLayout(context).apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER
            setPadding(dp(6), dp(3), dp(6), dp(3))
        }
        footer.addView(footerKey("@", onAt, repeatOnHold = false), footerParams(1f))
        footer.addView(
            footerKey(if (englishUi) "Enter" else "换行", onEnter, repeatOnHold = false),
            footerParams(1.6f),
        )
        footer.addView(
            footerKey("⌫", onBackspace, repeatOnHold = true),
            footerParams(1.6f),
        )
        addView(footer, LayoutParams(LayoutParams.MATCH_PARENT, dp(46)))

        body.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> resizeVoiceButton() }
        resultRow.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> resizeVoiceButton() }
        warning.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> resizeVoiceButton() }
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        // 220dp 级别的小屏：收起提示行，把高度让给中心按钮，避免与 footer 重叠。
        val compact = height < dp(300)
        val wanted = if (compact) View.GONE else View.VISIBLE
        if (modeHintRow.visibility != wanted) {
            modeHintRow.visibility = wanted
            requestLayout()
        }
        resizeVoiceButton()
    }

    private fun resizeVoiceButton() {
        val bodyHeight = body.height
        if (bodyHeight <= 0) return
        val resultHeight = if (resultRow.visibility == View.VISIBLE) resultRow.height else 0
        val warningHeight = if (warning.visibility == View.VISIBLE) warning.height else 0
        val available = bodyHeight - dp(8) - resultHeight - warningHeight
        val size = available.coerceIn(0, dp(190))
        if (size <= 0) return
        val params = voiceButton.layoutParams as? FrameLayout.LayoutParams ?: return
        if (params.width != size || params.height != size) {
            params.width = size
            params.height = size
            voiceButton.layoutParams = params
        }
    }

    fun renderAvailability(
        recording: Boolean,
        processing: Boolean,
        cancellable: Boolean = recording || processing,
        canUseVoiceModes: Boolean = recording || !processing,
    ) {
        cancel.isEnabled = cancellable
        modeKeys.forEach { key ->
            if (!canUseVoiceModes) { key.isPressed = false; key.cancelPendingInputEvents() }
            key.isEnabled = canUseVoiceModes
            key.alpha = if (canUseVoiceModes) 1f else 0.4f
        }
        val editingAllowed = !recording && !processing
        if (!editingAllowed) stopBackspaceRepeat()
        footerKeys.forEach { key ->
            key.isEnabled = editingAllowed
            key.isPressed = false
            key.alpha = if (editingAllowed) 1f else 0.4f
        }
        cancel.alpha = if (cancel.isEnabled) 1f else 0.4f
        voiceButton.contentDescription = when {
            recording -> if (englishUi) "Finish dictation" else "结束听写"
            processing -> if (englishUi) "Processing dictation" else "正在处理听写"
            else -> if (englishUi) "Start dictation" else "开始听写"
        }
    }

    /** 下发统一互动策略：值变化时先中止旧 repeat/按下态；相同值不影响进行中的交互。 */
    fun applyInteractionPolicy(policy: ImeInteractionPolicy) {
        if (policy == this.policy) return
        stopBackspaceRepeat()
        footerKeys.forEach { key ->
            key.isPressed = false
            key.cancelPendingInputEvents()
        }
        suppressFooterClickFeedback = false
        this.policy = policy
    }

    private fun defaultModeHint(): String = if (englishUi) {
        "Swipe ↑ RAW · ← Quick note · → Cloud · ↓ Cancel"
    } else {
        "上滑 RAW · 左滑速记 · 右滑云笔记 · 下滑取消"
    }

    private fun defaultWarning(): String = if (englishUi) {
        "Voice unavailable. Tap to retry"
    } else {
        "语音暂不可用，点击重试"
    }

    private fun textView(value: String, size: Float, color: Int): TextView = TextView(context).apply {
        text = value
        textSize = size
        gravity = Gravity.CENTER
        setTextColor(color)
        includeFontPadding = false
        maxLines = 1
    }

    private fun modeChip(label: String, action: () -> Unit): TextView = chip(label, action).also { modeKeys += it }

    private fun chip(label: String, action: () -> Unit): TextView = textView(label, 12f, palette.text).apply {
        setPadding(dp(14), 0, dp(14), 0)
        background = GradientDrawable().apply {
            cornerRadius = dp(15).toFloat()
            setColor(palette.auxiliary)
        }
        isClickable = true
        isFocusable = true
        contentDescription = label
        setOnClickListener { action() }
    }

    private fun actionText(label: String, action: () -> Unit): TextView =
        textView(label, 13f, palette.accent).apply {
            setPadding(dp(14), dp(6), dp(14), dp(6))
            isClickable = true
            isFocusable = true
            contentDescription = label
            setOnClickListener { action() }
        }

    private fun footerKey(label: String, action: () -> Unit, repeatOnHold: Boolean): TextView =
        textView(label, 17f, palette.text).apply {
            footerKeys.add(this)
            gravity = Gravity.CENTER
            background = GradientDrawable().apply {
                cornerRadius = dp(6).toFloat()
                setColor(palette.key)
            }
            isClickable = true
            isFocusable = true
            contentDescription = label
            setOnClickListener {
                // 触摸路径的 DOWN 已反馈过（suppress 时不再重复）；无障碍 click 没有 DOWN，这里补一次。
                if (!suppressFooterClickFeedback) onFeedback()
                action()
            }
            setOnTouchListener { view, event ->
                if (!view.isEnabled) return@setOnTouchListener false
                if (event.pointerCount != 1 || event.actionMasked == MotionEvent.ACTION_CANCEL) {
                    stopBackspaceRepeat()
                    view.isPressed = false
                    suppressFooterClickFeedback = false
                    return@setOnTouchListener true
                }
                when (event.actionMasked) {
                    MotionEvent.ACTION_DOWN -> {
                        stopBackspaceRepeat()
                        view.isPressed = true
                        backspacePressed = true
                        backspaceRepeating = false
                        onFeedback()
                        if (repeatOnHold) {
                            handler.postDelayed(backspaceRepeat(action), ViewConfiguration.getLongPressTimeout().toLong())
                        }
                        true
                    }
                    MotionEvent.ACTION_MOVE -> {
                        if (event.x < 0 || event.y < 0 || event.x >= view.width || event.y >= view.height) {
                            stopBackspaceRepeat()
                            view.isPressed = false
                        }
                        true
                    }
                    MotionEvent.ACTION_UP -> {
                        val inside = event.x >= 0 && event.y >= 0 && event.x < view.width && event.y < view.height
                        val commit = backspacePressed && !backspaceRepeating && inside
                        stopBackspaceRepeat()
                        view.isPressed = false
                        if (commit) {
                            suppressFooterClickFeedback = true
                            view.performClick()
                            suppressFooterClickFeedback = false
                        }
                        true
                    }
                    else -> true
                }
            }
        }

    private fun backspaceRepeat(action: () -> Unit): Runnable = object : Runnable {
        override fun run() {
            if (!backspacePressed || !isAttachedToWindow) return
            backspaceRepeating = true
            action()
            onFeedback()
            // 动作可能同步触发宿主 applyInteractionPolicy(不同值)/禁用/隐藏：排期前重新校验。
            if (backspacePressed && isAttachedToWindow && isShown) {
                handler.postDelayed(this, policy.repeatIntervalMs)
            }
        }
    }

    private fun stopBackspaceRepeat() {
        handler.removeCallbacksAndMessages(null)
        backspacePressed = false
        backspaceRepeating = false
    }

    private fun footerParams(weight: Float) = LayoutParams(0, dp(38), weight).apply {
        marginStart = dp(3)
        marginEnd = dp(3)
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    override fun onDetachedFromWindow() {
        stopBackspaceRepeat()
        super.onDetachedFromWindow()
    }
}
