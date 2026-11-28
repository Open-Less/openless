package com.openless.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * 通用候选条：横向候选 + 快捷粘贴胶囊 + 网格化展开面板（支持全部/单字筛选）。
 *
 * 契约：
 * - 条本身高度由父布局给定（约 42dp），候选子 View 严格贴合父高度，绝不垂直溢出。
 * - 展开层通过 [expandedView] 暴露，由调用方叠加到 body（FrameLayout）里，不改变键盘总高度。
 * - [submit] 复用 label 与 forget 能力相同的原 View，并更新最新回调；被替换/移除的旧 View 不再生效。
 * - 展开按钮使用 Canvas 矢量箭头，不受各厂商字体基线偏差影响。
 * - 全程只做展示与回调，不读 Service/JNI/偏好。
 */
internal class ImeCandidateStrip(
    context: Context,
    private val palette: ImePalette,
    private val englishUi: Boolean,
    private val onExpansionChanged: (Boolean) -> Unit = {},
) : LinearLayout(context) {

    data class Item(
        val label: String,
        val select: () -> Unit,
        val forget: (() -> Unit)? = null,
    )

    /** 展开层：真实 ScrollView + 筛选栏 + 4 列网格候选，调用方负责添加进 body。 */
    val expandedView: View

    private class Slot(val index: Int) {
        var item: Item? = null
        var view: TextView? = null
    }

    private val scroll = HorizontalScrollView(context)
    private val row = LinearLayout(context)
    private val expand = ExpandChevronButton(context)
    private val expandedContent = LinearLayout(context)
    private val filterBar = LinearLayout(context)
    private val filterAll = TextView(context)
    private val filterSingle = TextView(context)
    private val grid = CandidateGrid(context)
    private val expandedScroll = ScrollView(context)

    private val rowSlots = mutableListOf<Slot>()
    private val expandedSlots = mutableListOf<Slot>()
    private var preeditView: TextView? = null
    private var hintView: TextView? = null
    private var chipContainer: LinearLayout? = null

    private var items: List<Item> = emptyList()
    private var preedit: CharSequence? = null
    private var emptyHintText: String? = null
    private var quickChipLabel: String? = null
    private var quickChipPaste: (() -> Unit)? = null
    private var quickChipDismiss: (() -> Unit)? = null
    private var singleCharOnly = false
    private var isBlocked = false
    private var interactionDisabled = false
    private var isExpanded = false
    private var hadItems = false

    init {
        orientation = HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(6), 0, dp(2), 0)
        minimumHeight = dp(40)
        clipChildren = false
        clipToPadding = false

        row.orientation = HORIZONTAL
        row.gravity = Gravity.CENTER_VERTICAL
        scroll.apply {
            isHorizontalScrollBarEnabled = false
            isFillViewport = true
            overScrollMode = View.OVER_SCROLL_NEVER
            addView(row, ViewGroup.LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        }
        addView(scroll, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f))

        expand.apply {
            tag = "ime-cand-expand"
            contentDescription = expandDescription(false)
            isClickable = true
            isFocusable = true
            isEnabled = false
            alpha = 0.35f
            setOnClickListener {
                if (!isBlocked && !interactionDisabled) setExpanded(!isExpanded)
            }
        }
        addView(expand, LayoutParams(dp(44), LayoutParams.MATCH_PARENT))

        styleFilterTab(filterAll, if (englishUi) "All" else "全部", selected = true) {
            if (singleCharOnly) {
                singleCharOnly = false
                updateFilterTabs()
                reconcileExpanded()
            }
        }
        styleFilterTab(filterSingle, if (englishUi) "Single" else "单字", selected = false) {
            if (!singleCharOnly) {
                singleCharOnly = true
                updateFilterTabs()
                reconcileExpanded()
            }
        }
        filterBar.apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(8), dp(4), dp(8), dp(4))
            addView(filterAll, LayoutParams(LayoutParams.WRAP_CONTENT, dp(30)).apply { marginEnd = dp(8) })
            addView(filterSingle, LayoutParams(LayoutParams.WRAP_CONTENT, dp(30)))
        }
        expandedContent.apply {
            orientation = VERTICAL
            addView(filterBar, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
            addView(View(context).apply { setBackgroundColor(palette.separator) }, LayoutParams(LayoutParams.MATCH_PARENT, dp(1)))
            addView(grid, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        }

        expandedScroll.apply {
            visibility = GONE
            isClickable = true
            isFillViewport = true
            setBackgroundColor(palette.surface)
            addView(expandedContent, ViewGroup.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        }
        expandedView = expandedScroll
    }

    // --- 对外 API ---

    fun submit(items: List<Item>, preedit: CharSequence? = null, emptyHint: String? = null) {
        this.items = items
        this.preedit = preedit
        if (emptyHint != null) emptyHintText = emptyHint
        interactionDisabled = false
        if (items.isEmpty()) singleCharOnly = false

        val wasEmpty = !hadItems
        renderRow()
        hadItems = items.isNotEmpty()
        if (wasEmpty && items.isNotEmpty()) scroll.scrollTo(0, 0)

        if (isExpanded) {
            if (items.isEmpty() || isBlocked) setExpanded(false) else {
                updateFilterTabs()
                reconcileExpanded()
            }
        }
        applyInteractiveStates()
    }

    fun setEmptyHint(text: String) {
        emptyHintText = text
        if (items.isEmpty() && preedit.isNullOrEmpty()) updateHint()
    }

    fun setQuickChip(label: String?, onPaste: () -> Unit, onDismiss: () -> Unit) {
        quickChipLabel = label?.takeIf { it.isNotBlank() }
        quickChipPaste = if (quickChipLabel != null) onPaste else null
        quickChipDismiss = if (quickChipLabel != null) onDismiss else null
        if (items.isEmpty() && preedit.isNullOrEmpty()) updateHint()
    }

    fun setBlocked(blocked: Boolean) {
        if (isBlocked == blocked) return
        isBlocked = blocked
        if (blocked) {
            setExpanded(false)
            row.isPressed = false
            expand.isPressed = false
            cancelPendingInputEvents()
            cancelPendingOn(row)
            cancelPendingOn(grid)
        }
        applyInteractiveStates()
    }

    fun closeExpanded() {
        setExpanded(false)
    }

    fun cancelInteractions() {
        setExpanded(false)
        interactionDisabled = true
        row.isPressed = false
        expand.isPressed = false
        cancelPendingInputEvents()
        cancelPendingOn(row)
        cancelPendingOn(grid)
        applyInteractiveStates()
    }

    // --- 行渲染（槽位复用） ---

    private fun renderRow() {
        updatePreedit()
        reconcileRow()
        updateHint()
    }

    private fun updatePreedit() {
        val text = preedit
        val existing = preeditView
        if (text.isNullOrEmpty()) {
            if (existing != null) {
                row.removeView(existing)
                preeditView = null
            }
            return
        }
        if (existing != null) {
            existing.text = text
            existing.contentDescription = text.toString()
            return
        }
        val view = preeditTextView(text)
        preeditView = view
        row.addView(view, 0, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
    }

    private fun reconcileRow() {
        while (rowSlots.size > items.size) {
            val slot = rowSlots.removeAt(rowSlots.lastIndex)
            slot.view?.let { row.removeView(it) }
            slot.item = null
            slot.view = null
        }
        items.forEachIndexed { index, item ->
            val slot = rowSlots.getOrNull(index)
            if (slot == null) {
                val created = Slot(index)
                created.item = item
                created.view = createCandidateView(created, expanded = false)
                row.addView(created.view, rowCandidateParams())
                rowSlots.add(created)
            } else {
                val reusable = slot.view != null && slot.item?.label == item.label &&
                    (slot.item?.forget != null) == (item.forget != null)
                if (reusable) {
                    slot.item = item
                    updateCandidateView(slot, expanded = false)
                } else {
                    slot.view?.let { row.removeView(it) }
                    slot.item = item
                    slot.view = createCandidateView(slot, expanded = false)
                    row.addView(slot.view, rowCandidateParams())
                }
            }
        }
        reorderRow()
    }

    private fun rowCandidateParams(): LayoutParams = LayoutParams(
        LayoutParams.WRAP_CONTENT,
        LayoutParams.MATCH_PARENT,
    ).apply {
        topMargin = dp(4)
        bottomMargin = dp(4)
        marginEnd = dp(2)
    }

    private fun reorderRow() {
        var expected = if (preeditView != null) 1 else 0
        rowSlots.forEach { slot ->
            val view = slot.view ?: return@forEach
            if (row.indexOfChild(view) != expected) {
                row.removeView(view)
                row.addView(view, expected, rowCandidateParams())
            }
            expected++
        }
    }

    private fun updateHint() {
        val show = items.isEmpty() && preedit.isNullOrEmpty()
        if (!show) {
            hintView?.let { row.removeView(it) }
            hintView = null
            chipContainer?.let { row.removeView(it) }
            chipContainer = null
            return
        }
        val chipLabel = quickChipLabel
        if (chipLabel != null) {
            hintView?.let { row.removeView(it) }
            hintView = null
            chipContainer?.let { row.removeView(it) }
            val container = buildQuickChipView(chipLabel)
            chipContainer = container
            row.addView(container, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
            return
        }
        chipContainer?.let { row.removeView(it) }
        chipContainer = null
        val text = emptyHintText ?: defaultHint()
        val existing = hintView
        if (existing == null) {
            val view = hintTextView(text)
            hintView = view
            row.addView(view, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        } else {
            existing.text = text
        }
    }

    private fun buildQuickChipView(label: String): LinearLayout {
        val pill = LinearLayout(context).apply {
            orientation = HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(10), 0, dp(4), 0)
            background = GradientDrawable().apply {
                cornerRadius = dp(14).toFloat()
                setColor(palette.auxiliary)
            }
        }
        val pasteBtn = TextView(context).apply {
            text = label
            textSize = 13f
            gravity = Gravity.CENTER_VERTICAL
            includeFontPadding = false
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            maxWidth = dp(220)
            setTextColor(palette.accent)
            setTypeface(Typeface.DEFAULT, Typeface.BOLD)
            tag = "ime-cand-quick-chip"
            contentDescription = label
            isClickable = true
            isFocusable = true
            setOnClickListener {
                if (!isBlocked && !interactionDisabled) quickChipPaste?.invoke()
            }
        }
        val closeBtn = TextView(context).apply {
            text = "✕"
            textSize = 11f
            gravity = Gravity.CENTER
            includeFontPadding = false
            setTextColor(palette.secondaryText)
            setPadding(dp(8), 0, dp(6), 0)
            contentDescription = if (englishUi) "Dismiss suggestion" else "关闭快捷粘贴"
            isClickable = true
            isFocusable = true
            setOnClickListener {
                if (!isBlocked && !interactionDisabled) quickChipDismiss?.invoke()
            }
        }
        pill.addView(pasteBtn, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        pill.addView(closeBtn, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        return LinearLayout(context).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(4), dp(5), dp(4), dp(5))
            addView(pill, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        }
    }

    private fun updateCandidateView(slot: Slot, expanded: Boolean) {
        val view = slot.view ?: return
        val item = slot.item ?: return
        view.text = item.label
        view.tag = if (expanded) "ime-cand-expanded-${slot.index}" else "ime-cand-${slot.index}"
        view.contentDescription = item.label
        val isPrimary = slot.index == 0 && !expanded
        view.setTextColor(if (isPrimary) palette.accent else palette.text)
        view.setTypeface(Typeface.DEFAULT, if (isPrimary) Typeface.BOLD else Typeface.NORMAL)
        view.background = if (isPrimary) {
            GradientDrawable().apply {
                cornerRadius = dp(8).toFloat()
                setColor(Color.argb(30, Color.red(palette.accent), Color.green(palette.accent), Color.blue(palette.accent)))
            }
        } else null
        applyCandidateInteractive(view, slot, expanded)
    }

    private fun createCandidateView(slot: Slot, expanded: Boolean): TextView {
        val view = TextView(context).apply {
            textSize = if (expanded) 17f else 18f
            gravity = Gravity.CENTER
            includeFontPadding = false
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            setPadding(dp(12), 0, dp(12), 0)
            minWidth = dp(42)
            isClickable = true
            isFocusable = true
        }
        view.setOnClickListener {
            if (!isSlotActive(slot, view, expanded)) return@setOnClickListener
            slot.item?.select()
        }
        view.setOnLongClickListener {
            if (!isSlotActive(slot, view, expanded)) return@setOnLongClickListener true
            val forget = slot.item?.forget
            if (forget != null) {
                forget()
                true
            } else {
                false
            }
        }
        updateCandidateView(slot.apply { this.view = view }, expanded)
        return view
    }

    // --- 展开层 ---

    private fun filteredExpandedItems(): List<Pair<Int, Item>> {
        val indexed = items.mapIndexed { index, item -> index to item }
        if (!singleCharOnly) return indexed
        val singles = indexed.filter { (_, item) -> item.label.codePointCount(0, item.label.length) == 1 }
        return singles.ifEmpty { indexed }
    }

    private fun reconcileExpanded() {
        val visible = filteredExpandedItems()
        val hasSingleChars = items.any { it.label.codePointCount(0, it.label.length) == 1 }
        val hasMultiChars = items.any { it.label.codePointCount(0, it.label.length) > 1 }
        filterBar.visibility = if (hasSingleChars && hasMultiChars) VISIBLE else GONE

        while (expandedSlots.size > visible.size) {
            val slot = expandedSlots.removeAt(expandedSlots.lastIndex)
            slot.view?.let { grid.removeView(it) }
            slot.item = null
            slot.view = null
        }
        visible.forEachIndexed { pos, (originalIndex, item) ->
            val slot = expandedSlots.getOrNull(pos)
            if (slot == null) {
                val created = Slot(originalIndex)
                created.item = item
                created.view = createCandidateView(created, expanded = true)
                grid.addView(created.view)
                expandedSlots.add(created)
            } else {
                val reusable = slot.index == originalIndex && slot.view != null && slot.item?.label == item.label &&
                    (slot.item?.forget != null) == (item.forget != null)
                if (reusable) {
                    slot.item = item
                    updateCandidateView(slot, expanded = true)
                } else {
                    slot.view?.let { grid.removeView(it) }
                    val replacement = Slot(originalIndex)
                    replacement.item = item
                    replacement.view = createCandidateView(replacement, expanded = true)
                    expandedSlots[pos] = replacement
                    grid.addView(replacement.view)
                }
            }
        }
        reorderGrid()
        applyInteractiveStates()
    }

    private fun reorderGrid() {
        expandedSlots.forEachIndexed { pos, slot ->
            val view = slot.view ?: return@forEachIndexed
            if (grid.indexOfChild(view) != pos) {
                grid.removeView(view)
                grid.addView(view, pos)
            }
        }
    }

    private fun styleFilterTab(tab: TextView, label: String, selected: Boolean, onClick: () -> Unit) {
        tab.apply {
            text = label
            textSize = 12f
            gravity = Gravity.CENTER
            includeFontPadding = false
            setPadding(dp(12), 0, dp(12), 0)
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
        }
        applyFilterTabVisual(tab, selected)
    }

    private fun updateFilterTabs() {
        applyFilterTabVisual(filterAll, !singleCharOnly)
        applyFilterTabVisual(filterSingle, singleCharOnly)
    }

    private fun applyFilterTabVisual(tab: TextView, selected: Boolean) {
        tab.setTextColor(if (selected) Color.WHITE else palette.secondaryText)
        tab.setTypeface(Typeface.DEFAULT, if (selected) Typeface.BOLD else Typeface.NORMAL)
        tab.background = GradientDrawable().apply {
            cornerRadius = dp(14).toFloat()
            setColor(if (selected) palette.accent else palette.auxiliary)
        }
    }

    // --- 状态 ---

    private fun applyInteractiveStates() {
        val enabled = !isBlocked && !interactionDisabled
        expand.isEnabled = items.isNotEmpty() && enabled
        expand.alpha = if (expand.isEnabled) 1f else 0.35f
        expand.importantForAccessibility = if (enabled) {
            IMPORTANT_FOR_ACCESSIBILITY_YES
        } else {
            IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        rowSlots.forEach { slot -> slot.view?.let { applyCandidateInteractive(it, slot, false) } }
        expandedSlots.forEach { slot -> slot.view?.let { applyCandidateInteractive(it, slot, true) } }
    }

    private fun applyCandidateInteractive(view: TextView, slot: Slot, expanded: Boolean) {
        val enabled = !isBlocked && !interactionDisabled
        view.isEnabled = enabled
        view.isClickable = enabled
        view.isLongClickable = enabled && slot.item?.forget != null
        view.alpha = if (enabled) 1f else 0.4f
        view.importantForAccessibility = if (enabled) {
            IMPORTANT_FOR_ACCESSIBILITY_YES
        } else {
            IMPORTANT_FOR_ACCESSIBILITY_NO
        }
        if (!enabled) view.isPressed = false
        if (expanded) view.tag = "ime-cand-expanded-${slot.index}"
    }

    private fun isSlotActive(slot: Slot, view: View, expanded: Boolean): Boolean {
        if (isBlocked || interactionDisabled) return false
        if (slot.view !== view) return false
        if (expanded) {
            if (!isExpanded || expandedScroll.visibility != View.VISIBLE) return false
            if (view.parent !== grid) return false
        } else if (view.parent !== row) {
            return false
        }
        val current = slot.item ?: return false
        return items.getOrNull(slot.index) === current
    }

    private fun setExpanded(show: Boolean) {
        val wanted = show && !isBlocked && !interactionDisabled && items.isNotEmpty()
        if (wanted == isExpanded) return
        isExpanded = wanted
        expandedScroll.visibility = if (wanted) View.VISIBLE else View.GONE
        expand.expanded = wanted
        expand.contentDescription = expandDescription(wanted)
        if (wanted) {
            updateFilterTabs()
            reconcileExpanded()
            expandedScroll.scrollTo(0, 0)
        } else {
            cancelPendingOn(grid)
        }
        applyInteractiveStates()
        onExpansionChanged(wanted)
    }

    // --- 小工具 ---

    private fun preeditTextView(value: CharSequence): TextView = TextView(context).apply {
        text = value
        textSize = 14f
        gravity = Gravity.CENTER_VERTICAL
        includeFontPadding = false
        maxLines = 1
        ellipsize = TextUtils.TruncateAt.END
        maxWidth = dp(160)
        setTextColor(palette.accent)
        setPadding(dp(8), 0, dp(8), 0)
        tag = "ime-cand-preedit"
        contentDescription = value.toString()
    }

    private fun hintTextView(value: String): TextView = TextView(context).apply {
        text = value
        textSize = 13f
        gravity = Gravity.CENTER_VERTICAL
        includeFontPadding = false
        maxLines = 1
        setTextColor(palette.secondaryText)
        setPadding(dp(10), 0, dp(10), 0)
        tag = "ime-cand-hint"
    }

    private fun defaultHint(): String = if (englishUi) "Hold space to speak" else "按住空格说话"

    private fun expandDescription(expanded: Boolean): String = when {
        englishUi && expanded -> "Collapse candidates"
        englishUi -> "Expand candidates"
        expanded -> "收起候选"
        else -> "展开候选"
    }

    private fun cancelPendingOn(group: ViewGroup) {
        for (index in 0 until group.childCount) {
            val child = group.getChildAt(index)
            child.isPressed = false
            child.cancelPendingInputEvents()
            if (child is ViewGroup) cancelPendingOn(child)
        }
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()
    private fun dpf(value: Float): Float = value * resources.displayMetrics.density

    /** Canvas-drawn expand/collapse chevron so alignment never depends on OEM Unicode glyphs. */
    private inner class ExpandChevronButton(context: Context) : View(context) {
        var expanded: Boolean = false
            set(value) {
                if (field != value) {
                    field = value
                    invalidate()
                }
            }
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = palette.secondaryText
            style = Paint.Style.STROKE
            strokeCap = Paint.Cap.ROUND
            strokeJoin = Paint.Join.ROUND
        }
        private val chevron = Path()

        override fun onDraw(canvas: Canvas) {
            val cx = width / 2f
            val cy = height / 2f
            val halfW = dpf(6f)
            val halfH = dpf(3.2f)
            paint.strokeWidth = dpf(2f)
            paint.color = if (isEnabled) palette.secondaryText else palette.separator
            chevron.reset()
            if (expanded) {
                chevron.moveTo(cx - halfW, cy + halfH)
                chevron.lineTo(cx, cy - halfH)
                chevron.lineTo(cx + halfW, cy + halfH)
            } else {
                chevron.moveTo(cx - halfW, cy - halfH)
                chevron.lineTo(cx, cy + halfH)
                chevron.lineTo(cx + halfW, cy - halfH)
            }
            canvas.drawPath(chevron, paint)
        }
    }

    /** 展开层的 4 列整齐网格布局：长词自动跨 2/4 列，带柔和分隔线。 */
    private inner class CandidateGrid(context: Context) : ViewGroup(context) {
        private val columns = 4
        private val dividerPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = palette.separator
            strokeWidth = dpf(0.6f)
        }
        private val cellSpans = ArrayList<Int>()
        private var rowCount = 0

        init {
            setWillNotDraw(false)
        }

        private fun rowHeightPx(): Int = dp(46)

        private fun spanFor(child: View, colWidth: Int): Int {
            child.measure(
                MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED),
                MeasureSpec.makeMeasureSpec(rowHeightPx(), MeasureSpec.EXACTLY),
            )
            val desired = child.measuredWidth
            return when {
                desired <= colWidth -> 1
                desired <= colWidth * 2 -> 2
                else -> columns
            }
        }

        override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
            val available = MeasureSpec.getSize(widthMeasureSpec).coerceAtLeast(columns)
            val colWidth = available / columns
            val rowH = rowHeightPx()
            cellSpans.clear()
            var col = 0
            var rows = if (childCount == 0) 0 else 1
            for (index in 0 until childCount) {
                val child = getChildAt(index)
                val span = spanFor(child, colWidth).coerceAtMost(columns)
                if (col + span > columns) {
                    rows++
                    col = 0
                }
                cellSpans.add(span)
                child.measure(
                    MeasureSpec.makeMeasureSpec(colWidth * span, MeasureSpec.EXACTLY),
                    MeasureSpec.makeMeasureSpec(rowH, MeasureSpec.EXACTLY),
                )
                col += span
            }
            rowCount = rows
            setMeasuredDimension(available, resolveSize(rows * rowH, heightMeasureSpec))
        }

        override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
            val colWidth = width / columns
            val rowH = rowHeightPx()
            var col = 0
            var rowIdx = 0
            for (index in 0 until childCount) {
                val child = getChildAt(index)
                val span = cellSpans.getOrElse(index) { 1 }
                if (col + span > columns) {
                    rowIdx++
                    col = 0
                }
                val x = col * colWidth
                val y = rowIdx * rowH
                val w = if (col + span == columns) width - x else colWidth * span
                child.layout(x, y, x + w, y + rowH)
                col += span
            }
        }

        override fun onDraw(canvas: Canvas) {
            super.onDraw(canvas)
            val rowH = rowHeightPx().toFloat()
            for (r in 1..rowCount) {
                val y = r * rowH
                canvas.drawLine(0f, y, width.toFloat(), y, dividerPaint)
            }
            for (index in 0 until childCount) {
                val child = getChildAt(index)
                if (child.right < width - dp(2)) {
                    val x = child.right.toFloat()
                    canvas.drawLine(x, child.top + dpf(10f), x, child.bottom - dpf(10f), dividerPaint)
                }
            }
        }
    }
}
