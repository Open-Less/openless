package com.openless.app

import android.animation.ValueAnimator
import android.app.Activity
import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import java.lang.ref.WeakReference

/** Notices belong to the current Activity window; other contexts use the system fallback. */
object OpenLessNativeNotice {
    private val main = Handler(Looper.getMainLooper())
    private const val HOST_TAG = "openless.native.notice.host"

    class Pending internal constructor(private val context: Context, private val message: CharSequence, private val duration: Int, private val dark: Boolean?) {
        fun show() = OpenLessNativeNotice.show(context, message, duration, dark)
    }
    fun makeText(context: Context, message: CharSequence, duration: Int, dark: Boolean? = null) = Pending(context, message, duration, dark)

    fun show(context: Context, message: CharSequence, duration: Int = Toast.LENGTH_SHORT, dark: Boolean? = null) {
        val activity = (context as? Activity)?.let { WeakReference(it) }
        val fallback = context.applicationContext
        main.post {
            val owner = activity?.get()
            if (owner == null) {
                Toast.makeText(fallback, message, duration).show()
                return@post
            }
            if (owner.isFinishing || owner.isDestroyed) return@post
            val decor = owner.window?.decorView as? ViewGroup
            if (decor == null || !decor.isAttachedToWindow || !decor.hasWindowFocus()) {
                Toast.makeText(fallback, message, duration).show()
                return@post
            }
            val density = owner.resources.displayMetrics.density
            fun dp(value: Int) = (value * density + 0.5f).toInt()
            val night = dark ?: when (owner.getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE).getString("theme_mode", null)) {
                "light" -> false
                "dark" -> true
                else -> (owner.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
            }
            val host = (decor.findViewWithTag<View>(HOST_TAG) as? LinearLayout) ?: LinearLayout(owner).apply {
                tag = HOST_TAG
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER_HORIZONTAL
                importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
                val statusTop = if (Build.VERSION.SDK_INT >= 23) decor.rootWindowInsets?.systemWindowInsetTop ?: 0 else 0
                setPadding(dp(14), statusTop + dp(14), dp(14), 0)
                decor.addView(this, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            }
            val card = TextView(owner).apply {
                text = message
                textSize = 14f
                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                setTextColor(if (night) Color.WHITE else Color.rgb(30, 30, 34))
                setPadding(dp(18), dp(14), dp(18), dp(14))
                background = GradientDrawable().apply {
                    setColor(if (night) Color.rgb(44, 44, 46) else Color.WHITE)
                    cornerRadius = dp(20).toFloat()
                    setStroke(dp(1), if (night) Color.rgb(75, 75, 78) else Color.rgb(225, 225, 228))
                }
                elevation = dp(8).toFloat()
            }
            val params = LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { bottomMargin = dp(8) }
            host.addView(card, params)
            val animated = Build.VERSION.SDK_INT >= 26 && ValueAnimator.areAnimatorsEnabled()
            if (animated) {
                card.alpha = 0f
                card.translationY = -dp(12).toFloat()
                card.animate().alpha(1f).translationY(0f).setDuration(220).start()
            }
            card.announceForAccessibility(message)
            main.postDelayed({
                if (card.parent === host) host.removeView(card)
                if (host.childCount == 0 && host.parent === decor) decor.removeView(host)
            }, if (duration == Toast.LENGTH_LONG) 3500L else 2200L)
        }
    }
}
