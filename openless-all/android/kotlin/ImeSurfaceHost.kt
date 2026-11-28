package com.openless.app

import android.content.Context
import android.view.View
import android.view.ViewGroup
import android.widget.ScrollView

/** Stable system input-view root; only its presentation children are replaced. */
internal class ImeSurfaceHost(context: Context) : ViewGroup(context) {
    private val draftScroll = ScrollView(context).apply {
        isFillViewport = false
        isVerticalScrollBarEnabled = true
        visibility = GONE
        tag = "ime-draft-scroll"
    }
    private var keyboard: View? = null
    private var draft: View? = null
    private var draftKey: Any? = null
    private var resetDraftScroll = false

    init {
        clipChildren = true
        addView(draftScroll, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        tag = "ime-surface-host"
    }

    fun render(keyboard: View, draft: View?, draftKey: Any? = draft) {
        if (this.keyboard !== keyboard) {
            this.keyboard?.let(::removeView)
            (keyboard.parent as? ViewGroup)?.removeView(keyboard)
            this.keyboard = keyboard
            addView(keyboard, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        }
        if (this.draft !== draft) {
            draftScroll.removeAllViews()
            if (draft != null) {
                (draft.parent as? ViewGroup)?.removeView(draft)
                draftScroll.addView(draft, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
            }
            this.draft = draft
        }
        resetDraftScroll = resetDraftScroll || this.draftKey !== draftKey
        this.draftKey = draftKey
        draftScroll.visibility = if (draft == null) GONE else VISIBLE
        requestLayout()
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val width = MeasureSpec.getSize(widthMeasureSpec)
        val requestedLimit = MeasureSpec.getSize(heightMeasureSpec)
        val screenLimit = resources.displayMetrics.heightPixels.coerceAtLeast(1)
        val limit = if (MeasureSpec.getMode(heightMeasureSpec) == MeasureSpec.UNSPECIFIED || requestedLimit == 0) {
            screenLimit
        } else requestedLimit
        // Reserve editor space against the screen, not this measured window.
        // Subtracting it from every EXACT pass would shrink a wrap-content IME
        // repeatedly as WindowManager acknowledges the previous measured size.
        val editorMargin = minOf(dp(48), screenLimit / 5)
        val available = minOf(limit, (screenLimit - editorMargin).coerceAtLeast(0))
        val widthSpec = MeasureSpec.makeMeasureSpec(width, MeasureSpec.EXACTLY)
        val keys = keyboard
        if (keys == null) {
            setMeasuredDimension(width, 0)
            return
        }
        keys.measure(widthSpec, MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED))
        val preferredKeys = keys.measuredHeight
        val draftBudget = (available - minOf(preferredKeys, dp(220))).coerceAtLeast(0)
        val desiredDraft = if (draft != null) {
            draftScroll.measure(widthSpec, MeasureSpec.makeMeasureSpec(draftBudget, MeasureSpec.AT_MOST))
            draftScroll.measuredHeight
        } else 0
        val size = ImeSurfaceLayout.resolve(preferredKeys, desiredDraft, available, dp(220))
        if (keys.measuredHeight != size.keyboard) keys.measure(widthSpec, MeasureSpec.makeMeasureSpec(size.keyboard, MeasureSpec.EXACTLY))
        if (draft != null && draftScroll.measuredHeight != size.draft) {
            draftScroll.measure(widthSpec, MeasureSpec.makeMeasureSpec(size.draft, MeasureSpec.EXACTLY))
        }
        setMeasuredDimension(width, keys.measuredHeight + if (draft != null) draftScroll.measuredHeight else 0)
    }

    override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
        val draftHeight = if (draft != null) draftScroll.measuredHeight else 0
        if (draft != null) draftScroll.layout(0, 0, width, draftHeight)
        keyboard?.let { it.layout(0, draftHeight, width, draftHeight + it.measuredHeight) }
        if (resetDraftScroll) {
            draftScroll.scrollTo(0, 0)
            resetDraftScroll = false
        }
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
}
