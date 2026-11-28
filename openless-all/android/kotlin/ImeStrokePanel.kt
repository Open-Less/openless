package com.openless.app

import android.content.Context
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.text.SpannableString
import android.text.Spanned
import android.text.style.RelativeSizeSpan
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/**
 * 笔画 / 数字 / 符号面板（独立 View 组件）。
 *
 * 纯展示与手势：不读 Service、JNI、InputConnection、SharedPreferences；
 * 所有业务动作都通过 [onCommand] / [onText] / [onPunctuationPage] 回调交给 root
 * （StrokeInputController 适配层）决定。命令字面量与 root 侧约定：
 * `h,s,p,n,z,*,segment,script,space,voice,symbols,numbers,backspace,enter,clear,back,prev,next,toggle_symbols`；
 * 标点/冒号/分号/数字等直接上屏内容走 [onText]。
 *
 * 几何结构（与旧 StrokeInputController 语义一致）：
 * - 主面板：左标点 rail（上一组/下一组 + 当前组标点）、中间 4×3 九键、右动作列；
 * - 数字页：4×5（@ 1 2 3 退格 / : 4 5 6 回车 / , 7 8 9 语音 / + 0 . 符号 返回）；
 * - 符号页：12 项符号分 3 行 4 列 + 第 4 行 上一页/页码/下一页/数字/返回（页码不可点）。
 *
 * 候选区使用共享 [ImeCandidateStrip]（固定高度区域），展开视图叠加在 body 之上，
 * 不挤压键盘高度；展开时隐藏下方网格/rail 的无障碍节点。
 */
internal class ImeStrokePanel(
    context: Context,
    private val palette: ImePalette,
    header: View,
    private val englishUi: Boolean,
    private val onCommand: (String) -> Unit,
    private val onText: (String) -> Unit,
    private val onPunctuationPage: (Int) -> Unit,
    private val onFeedback: () -> Unit = {},
    interactionPolicy: ImeInteractionPolicy = ImeInteractionPolicy.DEFAULT,
) : LinearLayout(context) {

    internal data class Candidate(val label: String, val select: () -> Unit)

    private enum class Geometry { STROKE, NUMBER, SYMBOL }

    private var policy = interactionPolicy
    private val strip = ImeCandidateStrip(context, palette, englishUi) { expanded -> setExpandedOverlay(expanded) }
    private val clearPreedit = ImeActionButton(
        context,
        palette,
        "✕",
        description = ui("清除输入", "Clear input"),
        onAction = { onCommand("clear") },
        onFeedback = onFeedback,
        interactionPolicy = policy,
    ).apply {
        textSize = 15f
        setTextColor(palette.secondaryText)
    }
    private val divider = View(context).apply { setBackgroundColor(palette.separator) }
    private val bodyHost = FrameLayout(context)
    private val body = LinearLayout(context).apply { orientation = VERTICAL }

    private var geometry = Geometry.STROKE
    private var preeditText: CharSequence = ""
    private var traditional = false
    private var symbolPage = 0
    private var symbolPageCount = 1
    private var symbols: List<String> = emptyList()
    private var punctuation: List<String> = emptyList()
    private var blocked = false
    private var expandedOverlay: View? = null
    private var lastCandidates: List<Candidate> = emptyList()

    private val keys = mutableListOf<ImeActionButton>()
    private var punctuationKeys = emptyList<ImeActionButton>()
    private var symbolKeys = emptyList<ImeActionButton>()
    private var symbolPageLabel: TextView? = null
    private var scriptKey: ImeActionButton? = null

    init {
        orientation = VERTICAL
        setBackgroundColor(palette.surface)
        setPadding(dp(4), dp(4), dp(4), dp(4))
        addView(header, LayoutParams(LayoutParams.MATCH_PARENT, dp(40)))

        val candidateRow = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
        candidateRow.addView(strip, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f))
        clearPreedit.visibility = INVISIBLE
        candidateRow.addView(clearPreedit, LayoutParams(dp(38), LayoutParams.MATCH_PARENT))
        addView(candidateRow, LayoutParams(LayoutParams.MATCH_PARENT, dp(42)))
        addView(divider, LayoutParams(LayoutParams.MATCH_PARENT, dp(1)))

        bodyHost.addView(body, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        addView(bodyHost, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))

        strip.setEmptyHint(ui("候选显示在这里", "Candidates appear here"))
        rebuildBody()
    }

    /**
     * 单一渲染入口。同一几何形态下只更新标签/选中态/候选与 preedit，
     * 不重建键盘；形态切换（笔画↔数字↔符号）或标点页容量变化时先取消旧触摸再重建。
     */
    fun render(
        preedit: CharSequence,
        traditional: Boolean,
        numberMode: Boolean,
        symbolMode: Boolean,
        symbolPage: Int,
        symbolPageCount: Int,
        symbols: List<String>,
        punctuation: List<String>,
    ) {
        val nextGeometry = when {
            symbolMode -> Geometry.SYMBOL
            numberMode -> Geometry.NUMBER
            else -> Geometry.STROKE
        }
        val punctuationCapacityChanged =
            nextGeometry == Geometry.STROKE && punctuation.size != this.punctuation.size

        this.preeditText = preedit
        this.traditional = traditional
        this.symbolPage = symbolPage
        this.symbolPageCount = symbolPageCount.coerceAtLeast(1)
        this.symbols = symbols
        this.punctuation = punctuation

        if (nextGeometry != geometry || punctuationCapacityChanged) {
            cancelInteractions()
            geometry = nextGeometry
            rebuildBody()
        } else {
            updateDynamicContent()
        }
        updatePreedit()
        updateScriptState()
        refreshStrip()
        refreshBlockedVisuals()
    }

    /** 候选列表由 root 查询得到，这里只负责呈现；不触发键盘重建。 */
    fun submitCandidates(items: List<Candidate>) {
        lastCandidates = items
        refreshStrip()
    }

    /** 下发统一互动策略：值变化时先中止旧触摸/计时器，再更新已存在与后续新建的全部按键。 */
    fun applyInteractionPolicy(policy: ImeInteractionPolicy) {
        if (policy == this.policy) return
        cancelInteractions()
        this.policy = policy
        keys.forEach { it.applyInteractionPolicy(policy) }
        clearPreedit.applyInteractionPolicy(policy)
        scriptKey?.applyInteractionPolicy(policy)
    }

    /** 取消所有进行中的触摸/长按/重复动作（设置变化、面板重建、隐藏前调用）。 */
    fun cancelInteractions() {
        keys.forEach { it.cancelInteraction() }
        clearPreedit.cancelInteraction()
        strip.cancelInteractions()
    }

    /** 屏蔽/恢复全部交互（例如正在录音、字段敏感或输入连接失效时）。 */
    fun setBlocked(blocked: Boolean) {
        this.blocked = blocked
        if (blocked) {
            closeExpandedOverlay()
            keys.forEach { it.cancelInteraction() }
            clearPreedit.cancelInteraction()
        }
        refreshBlockedVisuals()
        if (!blocked) updateDynamicContent()
    }

    // ---- 几何构建 ----

    private fun rebuildBody() {
        closeExpandedOverlay()
        keys.clear()
        punctuationKeys = emptyList()
        symbolKeys = emptyList()
        symbolPageLabel = null
        scriptKey = null
        body.removeAllViews()
        when (geometry) {
            Geometry.STROKE -> buildStrokeBody()
            Geometry.NUMBER -> buildNumberBody()
            Geometry.SYMBOL -> buildSymbolBody()
        }
    }

    private fun buildStrokeBody() {
        val row = LinearLayout(context).apply { gravity = Gravity.CENTER }
        row.addView(buildPunctuationRail(), LayoutParams(0, LayoutParams.MATCH_PARENT, 0.16f))
        row.addView(buildStrokeGrid(), LayoutParams(0, LayoutParams.MATCH_PARENT, 0.65f))
        row.addView(buildStrokeActions(), LayoutParams(0, LayoutParams.MATCH_PARENT, 0.19f))
        body.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
    }

    private fun buildPunctuationRail(): LinearLayout {
        val rail = LinearLayout(context).apply {
            orientation = VERTICAL
            gravity = Gravity.CENTER
            setPadding(dp(1), dp(1), dp(1), dp(1))
            background = rounded(palette.auxiliary, dp(6))
        }
        rail.addView(
            commandKey("▲", ui("上一组标点", "Previous punctuation set"), "", labelSize = 12f, explicitAction = { onPunctuationPage(-1) }),
            LayoutParams(LayoutParams.MATCH_PARENT, dp(20)),
        )
        val marks = punctuationList()
        val buttons = marks.mapIndexed { index, mark ->
            keyButton(
                label = mark,
                description = ui("标点 $mark", "Punctuation $mark"),
                onAction = { punctuationList().getOrNull(index)?.let(onText) },
                labelSize = 17f,
            ).also { key ->
                rail.addView(key, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
                })
            }
        }
        punctuationKeys = buttons
        rail.addView(
            commandKey("▼", ui("下一组标点", "Next punctuation set"), "", labelSize = 12f, explicitAction = { onPunctuationPage(1) }),
            LayoutParams(LayoutParams.MATCH_PARENT, dp(20)),
        )
        return rail
    }

    private fun buildStrokeGrid(): LinearLayout {
        val grid = LinearLayout(context).apply {
            orientation = VERTICAL
            gravity = Gravity.CENTER
        }
        val rows = listOf(
            listOf(
                commandKey("1\n一", ui("横", "Horizontal stroke"), "h", swipeDigit = "1"),
                commandKey("2\n丨", ui("竖", "Vertical stroke"), "s", swipeDigit = "2"),
                commandKey("3\n丿", ui("撇", "Left falling stroke"), "p", swipeDigit = "3"),
            ),
            listOf(
                commandKey("4\n丶", ui("点", "Dot stroke"), "n", swipeDigit = "4"),
                commandKey("5\n乙", ui("折", "Turning stroke"), "z", swipeDigit = "5"),
                commandKey("6\n通配", ui("通配符", "Wildcard"), "*", swipeDigit = "6"),
            ),
            listOf(
                commandKey("7\n分词", ui("分词", "Segment word"), "segment", swipeDigit = "7"),
                textKey("8\n：", ui("冒号", "Colon"), ":", swipeDigit = "8"),
                textKey("9\n；", ui("分号", "Semicolon"), ";", swipeDigit = "9"),
            ),
            listOf(
                scriptButton(),
                spaceButton(),
                commandKey("符号", ui("符号", "Symbols"), "symbols", labelSize = 15f),
            ),
        )
        rows.forEach { rowKeys ->
            val row = LinearLayout(context).apply { gravity = Gravity.CENTER }
            rowKeys.forEach { key ->
                row.addView(key, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
                })
            }
            grid.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        }
        return grid
    }

    private fun buildStrokeActions(): LinearLayout {
        val actions = LinearLayout(context).apply {
            orientation = VERTICAL
            gravity = Gravity.CENTER
        }
        val cells = listOf(
            commandKey("←", ui("退格", "Backspace"), "backspace", repeat = true, labelSize = 22f),
            commandKey("↵", ui("回车", "Enter"), "enter", labelSize = 22f),
            commandKey("清除", ui("清除笔画编码", "Clear stroke code"), "clear", labelSize = 15f),
            commandKey("123", ui("数字与符号", "Numbers and symbols"), "numbers", labelSize = 15f),
        )
        cells.forEach { key ->
            actions.addView(key, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f).apply {
                setMargins(dp(1), dp(1), dp(1), dp(1))
            })
        }
        return actions
    }

    private fun buildNumberBody() {
        val column = LinearLayout(context).apply { orientation = VERTICAL }
        val rows = listOf(
            listOf("@", "1", "2", "3") to commandKey("退格", ui("退格", "Backspace"), "backspace", repeat = true, labelSize = 15f),
            listOf(":", "4", "5", "6") to commandKey("回车", ui("回车", "Enter"), "enter", labelSize = 15f),
            listOf(",", "7", "8", "9") to commandKey("语音", ui("语音输入", "Voice input"), "voice", labelSize = 15f),
            listOf("+", "0", ".") to commandKey("符号", ui("符号", "Symbols"), "toggle_symbols", labelSize = 15f),
        )
        rows.forEach { (content, last) ->
            val row = LinearLayout(context).apply { gravity = Gravity.CENTER }
            content.forEach { label ->
                row.addView(
                    textKey(label, ui("输入 $label", "Type $label"), label, labelSize = 20f),
                    LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply { setMargins(dp(1), dp(1), dp(1), dp(1)) },
                )
            }
            val trailing = if (content.size == 3) {
                // 最后一行是「+ 0 . 符号 返回」，符号之后还有返回键。
                listOf(last, commandKey("返回", ui("返回笔画", "Back to stroke"), "back", labelSize = 15f))
            } else {
                listOf(last)
            }
            trailing.forEach { key ->
                row.addView(key, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
                })
            }
            column.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        }
        body.addView(column, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
    }

    private fun buildSymbolBody() {
        val column = LinearLayout(context).apply { orientation = VERTICAL }
        val items = symbolItems()
        val built = mutableListOf<ImeActionButton>()
        for (rowIndex in 0 until 3) {
            val row = LinearLayout(context).apply { gravity = Gravity.CENTER }
            for (columnIndex in 0 until 4) {
                val index = rowIndex * 4 + columnIndex
                val label = items.getOrNull(index).orEmpty()
                val key = keyButton(
                    label = label,
                    description = if (label.isEmpty()) "" else ui("输入 $label", "Type $label"),
                    onAction = { symbolItems().getOrNull(index)?.takeIf { it.isNotEmpty() }?.let(onText) },
                    labelSize = 19f,
                ).apply {
                    if (label.isEmpty()) {
                        visibility = INVISIBLE
                        isEnabled = false
                    }
                }
                built.add(key)
                row.addView(key, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
                })
            }
            val command = listOf("backspace", "enter", "voice")[rowIndex]
            val label = listOf("退格", "回车", "语音")[rowIndex]
            row.addView(commandKey(label, label, command, repeat = rowIndex == 0, labelSize = 14f),
                LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply { setMargins(dp(1), dp(1), dp(1), dp(1)) })
            column.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        }
        symbolKeys = built

        val navRow = LinearLayout(context).apply { gravity = Gravity.CENTER }
        val navCells = listOf(
            commandKey("▲", ui("上一页", "Previous page"), "prev", labelSize = 13f),
            null,
            commandKey("▼", ui("下一页", "Next page"), "next", labelSize = 13f),
            commandKey("数字", ui("数字", "Numbers"), "toggle_symbols", labelSize = 14f),
            commandKey("返回", ui("返回笔画", "Back to stroke"), "back", labelSize = 14f),
        )
        navCells.forEach { key ->
            if (key != null) {
                navRow.addView(key, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
                })
            } else {
                symbolPageLabel = TextView(context).apply {
                    text = pageIndicatorText()
                    gravity = Gravity.CENTER
                    setTextColor(palette.secondaryText)
                    textSize = 13f
                    isClickable = false
                    isFocusable = false
                    contentDescription = pageIndicatorDescription()
                }
                navRow.addView(symbolPageLabel, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply {
                    setMargins(dp(1), dp(1), dp(1), dp(1))
                })
            }
        }
        column.addView(navRow, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        body.addView(column, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
    }

    // ---- 动态更新 ----

    private fun updateDynamicContent() {
        when (geometry) {
            Geometry.STROKE -> {
                val marks = punctuationList()
                punctuationKeys.forEachIndexed { index, key ->
                    val label = marks.getOrNull(index).orEmpty()
                    key.text = label
                    key.contentDescription = if (label.isEmpty()) "" else ui("标点 $label", "Punctuation $label")
                    key.visibility = if (label.isEmpty()) INVISIBLE else VISIBLE
                    key.isEnabled = label.isNotEmpty() && isEnabled && !blocked
                }
            }
            Geometry.NUMBER -> Unit
            Geometry.SYMBOL -> {
                val items = symbolItems()
                symbolKeys.forEachIndexed { index, key ->
                    val label = items.getOrNull(index).orEmpty()
                    key.text = label
                    key.contentDescription = if (label.isEmpty()) "" else ui("输入 $label", "Type $label")
                    key.visibility = if (label.isEmpty()) INVISIBLE else VISIBLE
                    // 稀疏页 -> 更满页（同 geometry 不重建）时，新出现的非空槽必须恢复可点。
                    key.isEnabled = label.isNotEmpty() && isEnabled && !blocked
                }
                symbolPageLabel?.let { indicator ->
                    indicator.text = pageIndicatorText()
                    indicator.contentDescription = pageIndicatorDescription()
                }
            }
        }
    }

    private fun updatePreedit() {
        clearPreedit.visibility = if (preeditText.isEmpty()) INVISIBLE else VISIBLE
    }

    private fun updateScriptState() {
        scriptKey?.let { key ->
            key.isSelected = traditional
            key.setTextColor(if (traditional) palette.accent else palette.text)
            key.setTypeface(key.typeface, if (traditional) Typeface.BOLD else Typeface.NORMAL)
            key.contentDescription = if (traditional) {
                ui("繁体输出，已开启", "Traditional output, on")
            } else {
                ui("简体输出，点击切换繁体", "Simplified output, tap for traditional")
            }
        }
    }

    private fun refreshStrip() {
        strip.submit(
            lastCandidates.map { ImeCandidateStrip.Item(it.label, it.select) },
            if (preeditText.isEmpty()) null else preeditText,
        )
    }

    /**
     * 单一交互可用性入口：父级 `isEnabled`、`blocked`、以及"空槽不可点"三者同时生效。
     * 供 setEnabled / setBlocked / render 后统一调用。
     */
    private fun refreshBlockedVisuals() {
        val interactive = isEnabled && !blocked
        keys.forEach { key ->
            key.isEnabled = interactive && key.text.isNotEmpty()
            key.alpha = if (interactive) 1f else 0.45f
        }
        clearPreedit.isEnabled = interactive
        clearPreedit.alpha = if (interactive) 1f else 0.45f
        strip.setBlocked(!interactive)
    }

    // ---- 候选展开叠加 ----

    private fun setExpandedOverlay(expanded: Boolean) {
        body.importantForAccessibility = if (expanded) {
            IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        } else {
            IMPORTANT_FOR_ACCESSIBILITY_AUTO
        }
        if (expanded) {
            val overlay = strip.expandedView
            if (expandedOverlay !== overlay) {
                expandedOverlay?.let { bodyHost.removeView(it) }
                overlay.layoutParams = FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT)
                bodyHost.addView(overlay)
                expandedOverlay = overlay
            } else {
                overlay.bringToFront()
            }
        } else if (expandedOverlay != null) {
            expandedOverlay?.let { bodyHost.removeView(it) }
            expandedOverlay = null
        }
    }

    private fun closeExpandedOverlay() {
        if (expandedOverlay != null) setExpandedOverlay(false)
        strip.closeExpanded()
    }

    // ---- 小组件 ----

    private fun commandKey(
        label: String,
        description: String,
        command: String,
        swipeDigit: String? = null,
        repeat: Boolean = false,
        labelSize: Float = 16f,
        explicitAction: (() -> Unit)? = null,
    ): ImeActionButton = keyButton(
        label = label,
        description = description,
        onAction = explicitAction ?: { if (command.isNotEmpty()) onCommand(command) },
        onSwipeUp = swipeDigit?.let { digit -> { onText(digit) } },
        repeat = repeat,
        labelSize = labelSize,
    )

    private fun textKey(label: String, description: String, text: String, swipeDigit: String? = null, labelSize: Float = 16f): ImeActionButton =
        keyButton(
            label = label,
            description = description,
            onAction = { onText(text) },
            onSwipeUp = swipeDigit?.let { digit -> { onText(digit) } },
            labelSize = labelSize,
        )

    private fun scriptButton(): ImeActionButton {
        val key = keyButton(
            label = "繁",
            description = ui("简体输出，点击切换繁体", "Simplified output, tap for traditional"),
            onAction = { onCommand("script") },
            labelSize = 17f,
        )
        scriptKey = key
        return key
    }

    private fun spaceButton(): ImeActionButton = keyButton(
        label = ui("空格", "space"),
        description = ui("空格，长按说话", "Space, hold to speak"),
        onAction = { onCommand("space") },
        onSwipeUp = { onText("0") },
        onHold = { onCommand("voice") },
        holdDelay = 900L,
        labelSize = 15f,
    )

    private fun keyButton(
        label: String,
        description: String,
        onAction: () -> Unit,
        onSwipeUp: (() -> Unit)? = null,
        onHold: (() -> Unit)? = null,
        repeat: Boolean = false,
        holdDelay: Long = 500L,
        labelSize: Float = 16f,
    ): ImeActionButton {
        val button = ImeActionButton(
            context,
            palette,
            label,
            description = description,
            onAction = onAction,
            onHold = onHold,
            repeatOnHold = repeat,
            onSwipeUp = onSwipeUp,
            holdDelayMs = holdDelay,
            onFeedback = onFeedback,
            interactionPolicy = policy,
        )
        button.textSize = labelSize
        if ('\n' in label) {
            button.maxLines = 2
            button.text = SpannableString(label).apply {
                setSpan(RelativeSizeSpan(0.5f), 0, label.indexOf('\n'), Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            }
        }
        button.setTextColor(palette.text)
        keys.add(button)
        return button
    }

    private fun punctuationList(): List<String> =
        punctuation.ifEmpty { listOf(",", "°", "?", "!", "~") }

    private fun symbolItems(): List<String> = symbols

    private fun pageIndicatorText(): String = "${(symbolPage + 1).coerceAtMost(symbolPageCount)}/$symbolPageCount"

    private fun pageIndicatorDescription(): String =
        ui("第 ${symbolPage + 1} 页，共 $symbolPageCount 页", "Page ${symbolPage + 1} of $symbolPageCount")

    private fun rounded(color: Int, radius: Int): GradientDrawable = GradientDrawable().apply {
        setColor(color)
        cornerRadius = radius.toFloat()
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private fun ui(zh: String, en: String): String = if (englishUi) en else zh

    override fun setEnabled(enabled: Boolean) {
        super.setEnabled(enabled)
        // 父级禁用必须同时收起子交互：只 enable 非空槽，并同步 clearPreedit 与候选条。
        refreshBlockedVisuals()
    }

    override fun onDetachedFromWindow() {
        cancelInteractions()
        super.onDetachedFromWindow()
    }
}
