package com.openless.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.widget.TextView
import kotlin.math.abs

/**
 * 通用动作键（剪贴板 / 笔画 / 语音 footer 共用）。
 *
 * 交互契约：
 * - 轻点（UP 在当前边界内）→ [onAction] 恰好一次。
 * - 按住超过 [holdDelayMs] → 有 [onHold] 时触发一次；否则 [repeatOnHold] 时按固定间隔重复
 *   [onAction]。hold / repeat 一旦触发，本次 UP 不再 [onAction]（互斥）。
 * - 有 [onSwipeUp] 的键：明确的向上手势允许终点越出**上边界**（横向需在合理范围内），
 *   中途即取消 hold timer，UP 时按当前终点解析：仍满足上滑 → 只触发 [onSwipeUp]；
 *   终点不再满足 → 不操作（不会退化成点击）。
 * - 无 [onSwipeUp] 的普通功能键：一旦移出边界，UP 不操作。
 * - ACTION_CANCEL / 多指：只清理手势，绝不执行任何动作。
 * - 未附着窗口时不启动/不续排定时器；detach 时清理全部定时器与待处理输入事件。
 * - performClick / performLongClick 供无障碍使用，与触摸路径共享同一互斥语义；
 *   若框架在触摸 tracking 中调用 performLongClick，会停掉 timer 并置 holdFired，
 *   避免随后的 UP 再次点击。
 *
 * 只做展示与手势分派：不读 Service/JNI/偏好，也不主动播放触觉或声音。
 */
internal class ImeActionButton(
    context: Context,
    private val palette: ImePalette,
    label: String,
    description: String = label,
    accent: Boolean = false,
    private val onAction: () -> Unit,
    private val onHold: (() -> Unit)? = null,
    private val repeatOnHold: Boolean = false,
    private val onSwipeUp: (() -> Unit)? = null,
    private val holdDelayMs: Long = 500L,
    private val onFeedback: () -> Unit = {},
    interactionPolicy: ImeInteractionPolicy = ImeInteractionPolicy.DEFAULT,
) : TextView(context) {

    private val handler = Handler(Looper.getMainLooper())
    private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop

    private var tracking = false
    private var pointerId = -1
    private var downX = 0f
    private var downY = 0f
    private var movedOutside = false
    private var swipeArmed = false
    private var holdFired = false
    private var repeating = false
    private var policy = interactionPolicy

    private val holdRunnable = object : Runnable {
        override fun run() {
            if (!tracking || !isAttachedToWindow || !isShown || !isEnabled) return
            if (repeatOnHold) {
                repeating = true
                holdFired = true
                onAction()
                onFeedback()
                if (tracking && isAttachedToWindow && isShown && isEnabled) handler.postDelayed(this, policy.repeatIntervalMs)
            } else {
                val hold = onHold ?: return
                holdFired = true
                onFeedback()
                hold()
            }
        }
    }

    init {
        text = label
        textSize = 17f
        gravity = Gravity.CENTER
        includeFontPadding = false
        maxLines = 1
        setTextColor(if (accent) WHITE else palette.text)
        background = stateBackground(accent)
        setPadding(dp(3), 0, dp(3), 0)
        isClickable = true
        isLongClickable = onHold != null || repeatOnHold
        isFocusable = true
        contentDescription = description
    }

    private val customMainPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { textAlign = Paint.Align.CENTER }
    private val customSubPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { textAlign = Paint.Align.CENTER }

    override fun onDraw(canvas: Canvas) {
        val raw = text?.toString().orEmpty()
        if ('\n' !in raw || width <= 0 || height <= 0) {
            super.onDraw(canvas)
            return
        }
        // Custom centered two-line rendering (e.g. "1\n一", "历史\n长按编辑") so
        // RelativeSizeSpan line metrics never push the main glyph off-center.
        background?.apply {
            setBounds(0, 0, width, height)
            draw(canvas)
        }
        val split = raw.indexOf('\n')
        val first = raw.substring(0, split)
        val second = raw.substring(split + 1)
        val isDigitHint = first.length == 1 && first[0].isDigit()
        val w = width.toFloat()
        val h = height.toFloat()
        val cx = w / 2f

        if (isDigitHint) {
            customSubPaint.color = palette.secondaryText
            customSubPaint.alpha = 190
            customSubPaint.textSize = minOf(dpf(10f), h * 0.22f).coerceAtLeast(dpf(8f))
            val sfm = customSubPaint.fontMetrics
            canvas.drawText(first, cx, dpf(3f) - sfm.ascent, customSubPaint)

            customMainPaint.color = currentTextColor
            customMainPaint.typeface = typeface ?: Typeface.DEFAULT
            val baseSize = minOf(textSize, h * 0.42f).coerceAtLeast(dpf(11f))
            customMainPaint.textSize = fitWidth(customMainPaint, second, baseSize, w - dpf(6f))
            val fm = customMainPaint.fontMetrics
            val cy = h * 0.58f
            canvas.drawText(second, cx, cy - (fm.ascent + fm.descent) / 2f, customMainPaint)
        } else {
            val baseMain = minOf(textSize, h * 0.34f).coerceAtLeast(dpf(10f))
            customMainPaint.color = currentTextColor
            customMainPaint.typeface = typeface ?: Typeface.DEFAULT
            customMainPaint.textSize = fitWidth(customMainPaint, first, baseMain, w - dpf(6f))
            val mfm = customMainPaint.fontMetrics

            val baseSub = minOf(dpf(10f), h * 0.22f).coerceAtLeast(dpf(8f))
            customSubPaint.color = palette.secondaryText
            customSubPaint.alpha = 220
            customSubPaint.textSize = fitWidth(customSubPaint, second, baseSub, w - dpf(6f))
            val sfm = customSubPaint.fontMetrics

            val gap = dpf(2f)
            val totalH = (mfm.descent - mfm.ascent) + gap + (sfm.descent - sfm.ascent)
            val top = (h - totalH) / 2f
            canvas.drawText(first, cx, top - mfm.ascent, customMainPaint)
            canvas.drawText(second, cx, top + (mfm.descent - mfm.ascent) + gap - sfm.ascent, customSubPaint)
        }
    }

    private fun fitWidth(paint: Paint, label: String, startSize: Float, maxWidth: Float): Float {
        paint.textSize = startSize
        val measured = paint.measureText(label)
        if (measured <= maxWidth || measured <= 0f || maxWidth <= 0f) return startSize
        return (startSize * (maxWidth / measured)).coerceAtLeast(dpf(8f))
    }

    private fun dpf(value: Float): Float = value * resources.displayMetrics.density

    private fun stateBackground(accent: Boolean): StateListDrawable {
        val normal = GradientDrawable().apply {
            cornerRadius = dp(6).toFloat()
            setColor(if (accent) palette.accent else palette.key)
        }
        val pressed = GradientDrawable().apply {
            cornerRadius = dp(6).toFloat()
            setColor(if (accent) palette.accent else palette.pressed)
        }
        val disabled = GradientDrawable().apply {
            cornerRadius = dp(6).toFloat()
            setColor(palette.auxiliary)
        }
        return StateListDrawable().apply {
            addState(intArrayOf(android.R.attr.state_pressed, android.R.attr.state_enabled), pressed)
            addState(intArrayOf(android.R.attr.state_enabled), normal)
            addState(intArrayOf(), disabled)
        }
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.pointerCount != 1) {
            cancelInteraction()
            return true
        }
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (!isEnabled) {
                    cancelInteraction()
                    return true
                }
                cancelInteraction()
                tracking = true
                pointerId = event.getPointerId(0)
                downX = event.x
                downY = event.y
                movedOutside = false
                swipeArmed = false
                holdFired = false
                repeating = false
                isPressed = true
                onFeedback()
                if (isAttachedToWindow) handler.postDelayed(holdRunnable, holdDelayMs)
                return true
            }

            MotionEvent.ACTION_MOVE -> {
                if (!tracking || event.getPointerId(0) != pointerId) return true
                val dx = event.x - downX
                val dy = event.y - downY
                if (inside(event.x, event.y)) {
                    if (!movedOutside && !swipeArmed) isPressed = true
                    return true
                }
                // 越界：有上滑能力且仍是明确向上手势时，允许越出上边界并取消 hold。
                if (onSwipeUp != null && qualifiesSwipe(dx, dy)) {
                    swipeArmed = true
                    isPressed = false
                    handler.removeCallbacks(holdRunnable)
                } else {
                    movedOutside = true
                    isPressed = false
                    handler.removeCallbacks(holdRunnable)
                }
                return true
            }

            MotionEvent.ACTION_UP -> {
                if (!tracking || event.getPointerId(0) != pointerId) {
                    cancelInteraction()
                    return true
                }
                val dx = event.x - downX
                val dy = event.y - downY
                val wasHold = holdFired
                val wasRepeat = repeating
                val wasOutside = movedOutside
                val wasSwipeArmed = swipeArmed
                cancelInteraction()
                if (!isEnabled) return true
                if (wasHold || wasRepeat) return true
                val swipe = onSwipeUp
                if (!wasOutside && swipe != null && qualifiesSwipe(dx, dy)) {
                    swipe()
                    return true
                }
                if (wasOutside || wasSwipeArmed) return true
                if (!inside(event.x, event.y)) return true
                commitClick(withFeedback = false)
                return true
            }

            MotionEvent.ACTION_CANCEL -> {
                cancelInteraction()
                return true
            }
        }
        return super.onTouchEvent(event)
    }

    override fun performClick(): Boolean {
        // 框架/无障碍可能在手指仍按下时调用 click：先结束当前手势，避免 UP 再提交一次。
        val wasTracking = tracking
        if (wasTracking) cancelInteraction()
        if (!isEnabled) return false
        // 触摸路径的 DOWN 已反馈过；无障碍 click 没有 DOWN，这里补一次反馈。
        commitClick(withFeedback = !wasTracking)
        return true
    }

    override fun performLongClick(): Boolean {
        if (!isEnabled) return false
        val wasTracking = tracking
        // 框架/无障碍可能在手指仍按下时调用长按：停 timer 并标记，避免 UP 再 click。
        if (wasTracking) handler.removeCallbacks(holdRunnable)
        return commitHold(withFeedback = !wasTracking)
    }

    /** 统一点击提交：触摸路径 DOWN 已反馈，故 withFeedback=false；无障碍路径补一次。 */
    private fun commitClick(withFeedback: Boolean) {
        // 保留 TextView 自身的 click 语义（点击提示音等），再分发动作。
        super.performClick()
        if (withFeedback) onFeedback()
        onAction()
    }

    /** 统一长按提交；触摸路径 DOWN 已反馈，故不重复，无障碍路径补一次。 */
    private fun commitHold(withFeedback: Boolean): Boolean {
        val hold = onHold
        if (hold != null) {
            if (tracking) holdFired = true
            if (withFeedback) onFeedback()
            hold()
            return true
        }
        if (repeatOnHold) {
            if (tracking) holdFired = true
            if (withFeedback) onFeedback()
            onAction()
            return true
        }
        return false
    }

    /** 下发统一互动策略：值变化时先中止旧计时器/当前触摸；相同值不动进行中的交互。 */
    fun applyInteractionPolicy(policy: ImeInteractionPolicy) {
        if (policy == this.policy) return
        cancelInteraction()
        this.policy = policy
    }

    /** 取消当前手势与全部定时器（模式切换、父容器重建等场景调用）。 */
    fun cancelInteraction() {
        handler.removeCallbacksAndMessages(null)
        tracking = false
        pointerId = -1
        movedOutside = false
        swipeArmed = false
        holdFired = false
        repeating = false
        isPressed = false
    }

    override fun onDetachedFromWindow() {
        cancelInteraction()
        super.onDetachedFromWindow()
    }

    private fun inside(x: Float, y: Float): Boolean = x >= 0f && y >= 0f && x < width && y < height

    /**
     * 明确向上：纵向主导、超过阈值，且横向偏移仍在合理范围。
     * 允许终点越出上边界（y<0），但左右不能飘太远。
     */
    private fun qualifiesSwipe(dx: Float, dy: Float): Boolean {
        val threshold = swipeThreshold()
        val horizontalLimit = maxOf(width.toFloat(), dp(72).toFloat())
        return dy < -threshold && abs(dy) > abs(dx) && abs(dx) <= horizontalLimit
    }

    private fun swipeThreshold(): Float = maxOf(touchSlop * 2f, dp(28).toFloat())

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private companion object {
        const val WHITE = 0xFFFFFFFF.toInt()
    }
}
