package com.openless.app

import android.content.Context
import android.graphics.drawable.GradientDrawable
import android.text.TextUtils
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/** Local editing and clipboard presentation. Storage and editor access belong to the adapter. */
internal class ImeClipboardPanel(
    context: Context,
    private val palette: ImePalette,
    header: View,
    private val englishUi: Boolean,
    private val onCommand: (String) -> Unit,
    private val onCategory: (String) -> Unit,
    private val onFeedback: () -> Unit = {},
    interactionPolicy: ImeInteractionPolicy = ImeInteractionPolicy.DEFAULT,
) : LinearLayout(context) {
    data class Item(
        val text: String,
        val favorite: Boolean,
        val inDictionary: Boolean,
        val select: () -> Unit,
        val toggleFavorite: () -> Unit,
        val remove: () -> Unit,
        val toggleDictionary: () -> Unit,
    )

    private val content = FrameLayout(context)
    private val editing = LinearLayout(context)
    private val history = LinearLayout(context)
    private val recentRows = LinearLayout(context)
    private val historyRows = LinearLayout(context)
    private val filters = LinearLayout(context)
    private val historyScroll = ScrollView(context)
    private val selectionKeys = ArrayList<TextView>()
    private var generation = 0L
    private var blocked = false
    private var targetToken: Long? = null
    private var policy = interactionPolicy

    init {
        orientation = VERTICAL
        setBackgroundColor(palette.surface)
        addView(header, LayoutParams(LayoutParams.MATCH_PARENT, dp(40)))
        editing.orientation = VERTICAL
        val grid = LinearLayout(context).apply { orientation = VERTICAL }
        val commands = listOf(
            listOf("range", "left", "right", "up", "down"),
            listOf("all", "copy", "paste", "history", "backspace"),
        )
        commands.forEach { rowCommands ->
            val row = LinearLayout(context)
            rowCommands.forEach { command ->
                val key = ImeActionButton(
                    context, palette, commandLabel(command), commandDescription(command),
                    onAction = { if (!blocked) onCommand(command) },
                    onHold = if (command == "history") ({ if (!blocked) onCommand("edit") }) else null,
                    repeatOnHold = command == "backspace" || command in listOf("left", "right", "up", "down"),
                    onFeedback = onFeedback,
                    interactionPolicy = policy,
                ).apply {
                    tag = "ime-edit-$command"
                    textSize = if (command == "history") 13f else 17f
                    maxLines = 2
                    if (command in listOf("range", "left", "right", "up", "down")) selectionKeys += this
                }
                row.addView(key, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f).apply { setMargins(dp(3), dp(3), dp(3), dp(3)) })
            }
            grid.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        }
        editing.addView(grid, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        recentRows.orientation = VERTICAL
        editing.addView(recentRows, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        content.addView(editing, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))

        history.orientation = VERTICAL
        filters.gravity = Gravity.CENTER_VERTICAL
        val filterScroll = HorizontalScrollView(context).apply {
            isHorizontalScrollBarEnabled = false
            addView(filters, ViewGroup.LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        }
        val toolbar = LinearLayout(context)
        toolbar.addView(filterScroll, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f))
        toolbar.addView(button(ui("清空", "Clear"), "ime-history-clear") { onCommand("clear_history") }, LayoutParams(dp(56), LayoutParams.MATCH_PARENT))
        toolbar.addView(button("✕", "ime-history-close") { onCommand("close_history") }.apply {
            contentDescription = ui("关闭剪贴板历史", "Close clipboard history")
        }, LayoutParams(dp(44), LayoutParams.MATCH_PARENT))
        history.addView(toolbar, LayoutParams(LayoutParams.MATCH_PARENT, dp(42)))
        historyRows.orientation = VERTICAL
        historyScroll.apply {
            isFillViewport = true
            addView(historyRows, ViewGroup.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        }
        history.addView(historyScroll, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        content.addView(history, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        addView(content, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
    }

    fun render(
        editorToken: Long,
        historyVisible: Boolean,
        selection: Boolean,
        category: String,
        entries: List<Item>,
        recent: List<Item>,
    ) {
        if (targetToken != editorToken) cancelInteractions()
        targetToken = editorToken
        val token = ++generation
        editing.visibility = if (historyVisible) GONE else VISIBLE
        history.visibility = if (historyVisible) VISIBLE else GONE
        selectionKeys.forEach { key ->
            key.isSelected = selection
            key.setTextColor(if (selection) palette.accent else palette.text)
        }
        recentRows.removeAllViews()
        for (index in 0 until 3) {
            val item = recent.getOrNull(index)
            val row = if (item == null) TextView(context).apply {
                text = if (index == 0) ui("暂无剪贴板记录", "No clipboard history") else ""
                gravity = Gravity.CENTER_VERTICAL
                setTextColor(palette.secondaryText)
                textSize = 13f
                setPadding(dp(12), 0, dp(12), 0)
            } else button("${index + 1}  ${item.text.take(1000)}", "ime-clip-recent-$index") {
                if (token == generation) item.select()
            }.apply {
                gravity = Gravity.CENTER_VERTICAL
                textSize = 15f
                setSingleLine(true)
                ellipsize = TextUtils.TruncateAt.END
            }
            recentRows.addView(row, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        }
        if (!historyVisible) { setBlocked(blocked); return }
        filters.removeAllViews()
        listOf("ALL" to ui("全部", "All"), "RECENT" to ui("最近", "Recent"), "TEXT" to ui("文本", "Text"),
            "NUMBER" to ui("数字", "Number"), "LINK" to ui("链接", "Link")).forEach { (id, label) ->
            filters.addView(button(label, "ime-clip-filter-$id") { if (token == generation) onCategory(id) }.apply {
                isSelected = id == category
                setTextColor(if (isSelected) palette.accent else palette.secondaryText)
                minWidth = dp(52)
                setPadding(dp(10), 0, dp(10), 0)
            }, LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT))
        }
        val oldScroll = historyScroll.scrollY
        historyRows.removeAllViews()
        if (entries.isEmpty()) historyRows.addView(TextView(context).apply {
            text = ui("暂无剪贴板记录", "No clipboard history")
            gravity = Gravity.CENTER
            setTextColor(palette.secondaryText)
            setPadding(0, dp(20), 0, 0)
        })
        entries.forEachIndexed { index, item ->
            val card = LinearLayout(context).apply {
                orientation = VERTICAL
                background = GradientDrawable().apply { cornerRadius = dp(8).toFloat(); setColor(palette.key) }
            }
            card.addView(button(item.text.take(2000), "ime-clip-item-$index") {
                if (token == generation) item.select()
            }.apply {
                maxLines = 2
                ellipsize = TextUtils.TruncateAt.END
                textSize = 15f
                gravity = Gravity.START or Gravity.CENTER_VERTICAL
            }, LayoutParams(LayoutParams.MATCH_PARENT, dp(48)))
            val actions = LinearLayout(context)
            listOf(
                Triple(if (item.favorite) ui("★ 已收藏", "★ Saved") else ui("☆ 收藏", "☆ Save"), "favorite", item.toggleFavorite),
                Triple(if (item.inDictionary) ui("移出词典", "Remove word") else ui("加入词典", "Add word"), "dictionary", item.toggleDictionary),
                Triple(ui("删除", "Delete"), "delete", item.remove),
            ).forEach { (label, actionId, action) ->
                actions.addView(button(label, "ime-clip-$actionId-$index") { if (token == generation) action() }.apply {
                    textSize = 12f
                    if (actionId == "delete") setTextColor(palette.error)
                }, LayoutParams(0, dp(38), 1f))
            }
            card.addView(actions)
            historyRows.addView(card, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT).apply {
                setMargins(dp(6), dp(3), dp(6), dp(3))
            })
        }
        historyScroll.post { if (token == generation) historyScroll.scrollTo(0, oldScroll) }
        setBlocked(blocked)
    }

    /** 下发统一互动策略：值变化时先中止旧触摸/计时器，再更新已存在与后续新建的全部按键。 */
    fun applyInteractionPolicy(policy: ImeInteractionPolicy) {
        if (policy == this.policy) return
        cancelInteractions()
        this.policy = policy
        walk(content) { view ->
            if (view is ImeActionButton) view.applyInteractionPolicy(policy)
        }
    }

    fun setBlocked(value: Boolean) {
        blocked = value
        if (value) walk(content) { if (it is ImeActionButton) it.cancelInteraction() }
        walk(content) { view ->
            if (view is ImeActionButton) {
                view.isEnabled = !value
                view.alpha = if (value) 0.45f else 1f
            }
        }
    }

    fun cancelInteractions() {
        ++generation
        walk(content) { if (it is ImeActionButton) it.cancelInteraction() }
    }

    override fun onDetachedFromWindow() {
        cancelInteractions()
        super.onDetachedFromWindow()
    }

    private fun button(label: String, id: String, action: () -> Unit) = ImeActionButton(
        context, palette, label, onAction = { if (!blocked) action() },
        onFeedback = onFeedback,
        interactionPolicy = policy,
    ).apply {
        tag = id
        textSize = 13f
        setPadding(dp(10), 0, dp(10), 0)
    }

    private fun walk(view: View, action: (View) -> Unit) {
        action(view)
        if (view is ViewGroup) for (index in 0 until view.childCount) walk(view.getChildAt(index), action)
    }

    private fun commandLabel(id: String): String = when (id) {
        "range" -> ui("选择", "Range")
        "left" -> "←"
        "right" -> "→"
        "up" -> "↑"
        "down" -> "↓"
        "all" -> ui("全选", "All")
        "copy" -> ui("复制", "Copy")
        "paste" -> ui("粘贴", "Paste")
        "history" -> ui("历史\n长按编辑", "History\nHold to edit")
        else -> "⌫"
    }
    private fun commandDescription(id: String): String = when (id) {
        "left" -> ui("光标左移", "Move cursor left")
        "right" -> ui("光标右移", "Move cursor right")
        "up" -> ui("光标上移", "Move cursor up")
        "down" -> ui("光标下移", "Move cursor down")
        "backspace" -> ui("退格", "Backspace")
        else -> commandLabel(id).replace('\n', ' ')
    }
    private fun ui(zh: String, en: String) = if (englishUi) en else zh
    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
}
