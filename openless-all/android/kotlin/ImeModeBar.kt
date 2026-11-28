package com.openless.app

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.LinearLayout
import android.widget.TextView

/** Each mode is a real accessible control, rather than one canvas hit region. */
internal class ImeModeBar(
    context: Context,
    palette: ImePalette,
    labels: List<String>,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
) : LinearLayout(context) {
    init {
        orientation = HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        val density = resources.displayMetrics.density
        val inset = (3 * density).toInt()
        setPadding(inset, inset, inset, inset)
        background = GradientDrawable().apply {
            cornerRadius = 18 * density
            setColor(palette.auxiliary)
        }
        labels.forEachIndexed { index, label ->
            val selected = index == selectedIndex
            addView(TextView(context).apply {
                text = label
                textSize = 12f
                gravity = Gravity.CENTER
                includeFontPadding = false
                setSingleLine(true)
                setTextColor(if (selected) palette.text else palette.secondaryText)
                setTypeface(Typeface.DEFAULT, if (selected) Typeface.BOLD else Typeface.NORMAL)
                background = GradientDrawable().apply {
                    cornerRadius = 15 * density
                    setColor(if (selected) palette.key else Color.TRANSPARENT)
                }
                isSelected = selected
                isFocusable = true
                contentDescription = label
                tag = "ime-mode-$index"
                accessibilityDelegate = object : View.AccessibilityDelegate() {
                    override fun onInitializeAccessibilityNodeInfo(host: View, info: AccessibilityNodeInfo) {
                        super.onInitializeAccessibilityNodeInfo(host, info)
                        info.className = "android.widget.Button"
                        info.isSelected = selected
                    }
                }
                setOnClickListener { if (!selected) onSelect(index) }
            }, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f))
        }
    }
}
