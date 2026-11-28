package com.openless.app

import android.animation.ObjectAnimator
import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.PorterDuff
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView

/** Shared chrome. Navigation gestures are confined to the header, never letter keys. */
internal class ImeKeyboardHeader(
    context: Context,
    private val palette: ImePalette,
    modeBar: View,
    bitmap: Bitmap?,
    onSettings: () -> Unit,
    onKeyboardSettings: () -> Unit,
    private val onModeStep: (Int) -> Unit,
    private val onHide: () -> Unit,
) : LinearLayout(context) {
    val indicator: View
    private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop.toFloat()
    private val gesture = ImeGestureSession(touchSlop)
    private var dispatchGeneration = 0L
    private var pulse: ObjectAnimator? = null
    private var ready = true

    init {
        orientation = HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(10), dp(1), dp(6), dp(1))
        val brand = LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL
            isFocusable = true
            contentDescription = "OpenLess 设置 / OpenLess settings"
            setOnClickListener { onSettings() }
            setOnLongClickListener { onKeyboardSettings(); true }
        }
        if (bitmap != null) {
            val height = dp(19)
            brand.addView(ImageView(context).apply {
                setImageBitmap(bitmap)
                setColorFilter(palette.text, PorterDuff.Mode.SRC_IN)
                scaleType = ImageView.ScaleType.FIT_CENTER
            }, LayoutParams((height.toFloat() * bitmap.width / bitmap.height.coerceAtLeast(1)).toInt(), height))
        } else {
            brand.addView(TextView(context).apply {
                text = "OpenLess"
                textSize = 16f
                setTypeface(Typeface.DEFAULT, Typeface.BOLD)
                setTextColor(palette.text)
            })
        }
        indicator = View(context).apply {
            importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            background = GradientDrawable().apply { shape = GradientDrawable.OVAL }
        }
        brand.addView(indicator, LayoutParams(dp(9), dp(9)).apply { marginStart = dp(5) })
        addView(brand, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT).apply { marginEnd = dp(8) })
        addView(modeBar, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f))
    }

    fun renderLink(color: Int, healthy: Boolean) {
        ready = healthy
        (indicator.background as GradientDrawable).apply {
            setColor(color)
            // The local/raw white status dot needs contrast on the light canvas.
            setStroke(if (Color.luminance(color) > 0.75 && Color.luminance(palette.surface) > 0.5) dp(1) else 0, palette.separator)
        }
        pulse?.duration = if (ready) 1800L else 900L
        ensurePulse()
    }

    private fun ensurePulse() {
        if (!isAttachedToWindow || !isShown || !ValueAnimator.areAnimatorsEnabled()) {
            pulse?.cancel()
            pulse = null
            indicator.alpha = 1f
            return
        }
        if (pulse?.isRunning == true) return
        pulse = ObjectAnimator.ofFloat(indicator, View.ALPHA, 1f, 0.4f).apply {
            duration = if (ready) 1800L else 900L
            repeatMode = ValueAnimator.REVERSE
            repeatCount = ValueAnimator.INFINITE
            start()
        }
    }

    override fun onInterceptTouchEvent(event: MotionEvent): Boolean {
        if (event.pointerCount != 1 || event.actionMasked == MotionEvent.ACTION_CANCEL) {
            cancelInteractions()
            return false
        }
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                ++dispatchGeneration
                gesture.begin(event.x, event.y, horizontal = true, down = true)
            }
            MotionEvent.ACTION_MOVE -> if (gesture.move(event.x, event.y) != ImeGestureSession.Axis.NONE) {
                parent?.requestDisallowInterceptTouchEvent(true)
            }
        }
        return gesture.axis != ImeGestureSession.Axis.NONE
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.pointerCount != 1 || event.actionMasked == MotionEvent.ACTION_CANCEL) {
            cancelInteractions()
            return true
        }
        when (event.actionMasked) {
            MotionEvent.ACTION_MOVE -> gesture.move(event.x, event.y)
            MotionEvent.ACTION_UP -> {
                val direction = gesture.release(event.x, event.y, maxOf(width / 3f, touchSlop), dp(100).toFloat())
                val generation = dispatchGeneration
                // Finish Android dispatch before changing panels; a hidden, detached
                // or target-invalidated header cannot run a queued navigation action.
                post {
                    if (generation == dispatchGeneration && isAttachedToWindow && isShown) {
                        when (direction) {
                            ImeGestureSession.Direction.LEFT -> onModeStep(1)
                            ImeGestureSession.Direction.RIGHT -> onModeStep(-1)
                            ImeGestureSession.Direction.DOWN -> onHide()
                            else -> Unit
                        }
                    }
                }
            }
        }
        return true
    }

    fun cancelInteractions() {
        ++dispatchGeneration
        gesture.cancel()
        pulse?.cancel()
        pulse = null
        indicator.alpha = 1f
        fun cancel(view: View) {
            view.isPressed = false
            view.cancelPendingInputEvents()
            view.cancelLongPress()
            if (view is ViewGroup) for (index in 0 until view.childCount) cancel(view.getChildAt(index))
        }
        cancel(this)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        ensurePulse()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        // Android may call this during the View constructor; animation state is nullable.
        if (visibility != VISIBLE) { pulse?.cancel(); pulse = null }
        else if (isAttachedToWindow) ensurePulse()
    }

    override fun onDetachedFromWindow() {
        cancelInteractions()
        pulse?.cancel()
        pulse = null
        super.onDetachedFromWindow()
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
}
