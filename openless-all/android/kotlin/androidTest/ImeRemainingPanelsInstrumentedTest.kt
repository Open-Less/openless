package com.openless.app

import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * 剩余面板的 View 级仪器测试：ImeActionButton / ImeCandidateStrip / ImeStrokePanel / ImeClipboardPanel。
 *
 * - 不启动 Activity，不触达 Service/JNI/InputConnection/native；全部用 fake 回调。
 * - 固定 measure/layout（默认 1080×900）后用真实 MotionEvent 序列驱动手势路径。
 * - 触摸钮上的手势直接向该按钮 [View.dispatchTouchEvent] 发送（坐标是按钮本地坐标），
 *   面板级定位用 `offsetDescendantRectToMyCoords`。
 * - 不 sleep、不等待长按定时器：`ImeActionButton` 的 hold/repeat 依赖 `isAttachedToWindow`，
 *   本测试不附着窗口，因此只覆盖点击/上滑/取消/移出等触摸路径（限制见文件末注释）。
 *
 * 断言对象是**目标语义**（与 root 已确认的 production 修订一致，例如标点/符号翻页后点击命中最新值、
 * 符号页保留退格/回车/语音动作键、笔画键标签同时含数字与笔画字形且至少两行）。因此本文件在
 * production 修订合入前可能失败，属于预期。
 */
@RunWith(AndroidJUnit4::class)
class ImeRemainingPanelsInstrumentedTest {

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val palette = ImePalette.resolve(false)

    private fun onMain(block: () -> Unit) = instrumentation.runOnMainSync(block)

    private fun dp(value: Int): Int = (value * context.resources.displayMetrics.density).toInt()

    // ---- 通用布局/事件辅助 ----

    private fun layout(view: View, width: Int = 1080, height: Int = 900) {
        val widthSpec = View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY)
        val heightSpec = View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY)
        view.measure(widthSpec, heightSpec)
        view.layout(0, 0, width, height)
    }

    private fun host(view: View, width: Int = 720, height: Int = 200): FrameLayout {
        val host = FrameLayout(context)
        host.addView(
            view,
            FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT),
        )
        layout(host, width, height)
        return host
    }

    private fun dispatch(view: View, action: Int, x: Float, y: Float) {
        val now = SystemClock.uptimeMillis()
        val event = MotionEvent.obtain(now, now, action, x, y, 0)
        view.dispatchTouchEvent(event)
        event.recycle()
    }

    private fun tap(view: View) {
        val x = view.width / 2f
        val y = view.height / 2f
        dispatch(view, MotionEvent.ACTION_DOWN, x, y)
        dispatch(view, MotionEvent.ACTION_UP, x, y)
    }

    private fun pressThenUp(view: View, upX: Float, upY: Float, moves: List<Pair<Float, Float>> = emptyList()) {
        val startX = view.width / 2f
        val startY = view.height / 2f
        dispatch(view, MotionEvent.ACTION_DOWN, startX, startY)
        moves.forEach { (x, y) -> dispatch(view, MotionEvent.ACTION_MOVE, x, y) }
        dispatch(view, MotionEvent.ACTION_UP, upX, upY)
    }

    private fun walk(root: View, action: (View) -> Unit) {
        action(root)
        if (root is ViewGroup) {
            for (index in 0 until root.childCount) walk(root.getChildAt(index), action)
        }
    }

    private fun allButtons(root: View): List<ImeActionButton> {
        val result = mutableListOf<ImeActionButton>()
        walk(root) { if (it is ImeActionButton) result.add(it) }
        return result
    }

    /** 按文本包含关系定位（production 不用 tag 时唯一稳定的定位方式）。 */
    private fun findKey(root: View, contains: String): ImeActionButton {
        var found: ImeActionButton? = null
        walk(root) { view ->
            if (found == null && view is ImeActionButton && view.text.toString().contains(contains)) {
                found = view
            }
        }
        assertNotNull("找不到包含「$contains」的按键", found)
        return found!!
    }

    private fun findTagged(root: View, tag: String): View {
        var found: View? = null
        walk(root) { if (found == null && it.tag == tag) found = it }
        assertNotNull("找不到 tag=$tag", found)
        return found!!
    }

    private fun findTextMatching(root: View, regex: Regex): TextView {
        var found: TextView? = null
        walk(root) { view ->
            if (found == null && view is TextView && regex.matches(view.text.toString().trim())) {
                found = view
            }
        }
        assertNotNull("找不到匹配 $regex 的文本", found)
        return found!!
    }

    // ================= ImeActionButton =================

    private fun buildButton(
        label: String = "k",
        onSwipeUp: (() -> Unit)? = null,
    ): Pair<ImeActionButton, MutableList<String>> {
        val events = mutableListOf<String>()
        val button = ImeActionButton(
            context,
            palette,
            label,
            description = label,
            onAction = { events.add("click") },
            onSwipeUp = onSwipeUp?.let { { events.add("swipe") } },
        )
        host(button, width = dp(120), height = dp(80))
        return button to events
    }

    @Test
    fun plainKeySwipedPastTopEdgeDoesNotClick() {
        val (button, events) = buildButton()
        onMain {
            pressThenUp(button, upX = button.width / 2f, upY = -dp(40).toFloat(), moves = listOf(
                button.width / 2f to -dp(40).toFloat(),
            ))
        }
        assertEquals(emptyList<String>(), events)
    }

    @Test
    fun swipeKeyPastTopEdgeFiresSwipeOnceAndNeverClicks() {
        val (button, events) = buildButton(onSwipeUp = {})
        onMain {
            pressThenUp(button, upX = button.width / 2f, upY = -dp(40).toFloat(), moves = listOf(
                button.width / 2f to -dp(40).toFloat(),
            ))
        }
        assertEquals(listOf("swipe"), events)
    }

    @Test
    fun cancelEventNeverClicks() {
        val (button, events) = buildButton()
        onMain {
            dispatch(button, MotionEvent.ACTION_DOWN, button.width / 2f, button.height / 2f)
            dispatch(button, MotionEvent.ACTION_CANCEL, button.width / 2f, button.height / 2f)
        }
        assertEquals(emptyList<String>(), events)
    }

    @Test
    fun movingOutsideThenBackInsideDoesNotClick() {
        val (button, events) = buildButton()
        val centerX = button.width / 2f
        val centerY = button.height / 2f
        onMain {
            dispatch(button, MotionEvent.ACTION_DOWN, centerX, centerY)
            dispatch(button, MotionEvent.ACTION_MOVE, button.width + dp(60).toFloat(), centerY)
            dispatch(button, MotionEvent.ACTION_MOVE, centerX, centerY)
            dispatch(button, MotionEvent.ACTION_UP, centerX, centerY)
        }
        assertEquals(emptyList<String>(), events)
    }

    // ================= ImeCandidateStrip =================

    private fun buildStrip(onExpansion: (Boolean) -> Unit = {}): Pair<ImeCandidateStrip, FrameLayout> {
        val strip = ImeCandidateStrip(context, palette, englishUi = false, onExpansionChanged = onExpansion)
        val host = host(strip, width = 1080, height = dp(42))
        return strip to host
    }

    @Test
    fun sameLabelReusesViewAndBindsLatestCallback() {
        val stripAndHost = buildStrip()
        val strip = stripAndHost.first
        val selected = mutableListOf<String>()
        onMain {
            strip.submit(listOf(0, 1, 2).map { index ->
                ImeCandidateStrip.Item("same-$index", select = { selected.add("old-$index") })
            })
            val reused = findTagged(strip, "ime-cand-0")
            strip.submit(listOf(0, 1, 2).map { index ->
                ImeCandidateStrip.Item("same-$index", select = { selected.add("new-$index") })
            })
            val after = findTagged(strip, "ime-cand-0")
            assertSame(reused, after)
            reused.performClick()
        }
        assertEquals(listOf("new-0"), selected)
    }

    @Test
    fun replacedCandidateViewCannotFireNewSlotAction() {
        val strip = buildStrip().first
        val selected = mutableListOf<String>()
        lateinit var stale: View
        onMain {
            strip.submit(listOf(ImeCandidateStrip.Item("alpha", select = { selected.add("alpha") })))
            stale = findTagged(strip, "ime-cand-0")
            strip.submit(listOf(ImeCandidateStrip.Item("beta", select = { selected.add("beta") })))
            // 槽位换了 View：旧引用点击必须无效。
            stale.performClick()
        }
        assertEquals(emptyList<String>(), selected)
        onMain {
            findTagged(strip, "ime-cand-0").performClick()
        }
        assertEquals(listOf("beta"), selected)
    }

    @Test
    fun closingExpandedStopsExpandedCandidateClicks() {
        val strip = buildStrip().first
        val selected = mutableListOf<String>()
        lateinit var expandedItem: View
        onMain {
            strip.submit(listOf(ImeCandidateStrip.Item("one", select = { selected.add("one") })))
            findTagged(strip, "ime-cand-expand").performClick()
        }
        onMain {
            expandedItem = findTagged(strip.expandedView, "ime-cand-expanded-0")
            expandedItem.performClick()
            assertEquals(listOf("one"), selected)
            strip.closeExpanded()
            expandedItem.performClick()
        }
        assertEquals(listOf("one"), selected)
    }

    @Test
    fun blockedStripIgnoresCandidateAndExpansion() {
        val strip = buildStrip().first
        val selected = mutableListOf<String>()
        onMain {
            strip.submit(listOf(ImeCandidateStrip.Item("one", select = { selected.add("one") })))
            strip.setBlocked(true)
            findTagged(strip, "ime-cand-0").performClick()
            findTagged(strip, "ime-cand-expand").performClick()
            assertFalse(findTagged(strip, "ime-cand-expand").isEnabled)
        }
        assertEquals(emptyList<String>(), selected)
        onMain {
            strip.setBlocked(false)
            findTagged(strip, "ime-cand-0").performClick()
        }
        assertEquals(listOf("one"), selected)
    }

    // ================= ImeStrokePanel =================

    private val commands = mutableListOf<String>()
    private val texts = mutableListOf<String>()
    private val punctuationPages = mutableListOf<Int>()
    private lateinit var strokePanel: ImeStrokePanel

    private fun buildStrokePanel() {
        commands.clear()
        texts.clear()
        punctuationPages.clear()
        strokePanel = ImeStrokePanel(
            context,
            palette,
            View(context),
            englishUi = false,
            onCommand = { commands.add(it) },
            onText = { texts.add(it) },
            onPunctuationPage = { punctuationPages.add(it) },
        )
        layout(strokePanel, 1080, 900)
    }

    private fun strokeRender(
        preedit: CharSequence = "",
        traditional: Boolean = false,
        numberMode: Boolean = false,
        symbolMode: Boolean = false,
        symbolPage: Int = 0,
        symbolPageCount: Int = 1,
        symbols: List<String> = emptyList(),
        punctuation: List<String> = listOf(",", "°", "?", "!", "~"),
    ) {
        strokePanel.render(
            preedit,
            traditional,
            numberMode,
            symbolMode,
            symbolPage,
            symbolPageCount,
            symbols,
            punctuation,
        )
        layout(strokePanel, 1080, 900)
    }

    @Test
    fun strokeGridEmitsExpectedCommandsAndTexts() {
        onMain {
            buildStrokePanel()
            strokeRender()
            findKey(strokePanel, "一").performClick()
            findKey(strokePanel, "丨").performClick()
            findKey(strokePanel, "丿").performClick()
            findKey(strokePanel, "丶").performClick()
            findKey(strokePanel, "乙").performClick()
            findKey(strokePanel, "通配").performClick()
            findKey(strokePanel, "分词").performClick()
            findKey(strokePanel, "繁").performClick()
            findKey(strokePanel, "空格").performClick()
            findKey(strokePanel, "←").performClick()
            findKey(strokePanel, "↵").performClick()
            findKey(strokePanel, "清除").performClick()
            findKey(strokePanel, "123").performClick()
            findKey(strokePanel, "：").performClick()
            findKey(strokePanel, "；").performClick()
        }
        assertEquals(
            listOf("h", "s", "p", "n", "z", "*", "segment", "script", "space", "backspace", "enter", "clear", "numbers"),
            commands,
        )
        assertEquals(listOf(":", ";"), texts)
    }

    @Test
    fun digitKeysSwipeUpEmitDigits() {
        onMain {
            buildStrokePanel()
            strokeRender()
            val digitOne = findKey(strokePanel, "一")
            val digitNine = findKey(strokePanel, "；")
            listOf(digitOne to "1", digitNine to "9").forEach { (key, digit) ->
                val x = key.width / 2f
                dispatch(key, MotionEvent.ACTION_DOWN, x, key.height / 2f)
                dispatch(key, MotionEvent.ACTION_MOVE, x, -dp(40).toFloat())
                dispatch(key, MotionEvent.ACTION_UP, x, -dp(40).toFloat())
                assertTrue("swipe up 应产生 $digit", texts.contains(digit))
            }
        }
        assertTrue(texts.contains("1"))
        assertTrue(texts.contains("9"))
        assertFalse(commands.contains("h"))
    }

    @Test
    fun punctuationPageFlipUsesLatestValues() {
        val firstPage = listOf("、", "。", "，")
        val secondPage = listOf("〈", "〉", "「")
        onMain {
            buildStrokePanel()
            strokeRender(punctuation = firstPage)
            val railKey = findKey(strokePanel, "、")
            strokeRender(punctuation = secondPage)
            // 同一几何只更新标签：旧引用点击必须命中最新一组的第一个值。
            railKey.performClick()
        }
        assertEquals(listOf("〈"), texts)
    }

    @Test
    fun numberModeEmitsCommands() {
        onMain {
            buildStrokePanel()
            strokeRender(numberMode = true)
            findKey(strokePanel, "退格").performClick()
            findKey(strokePanel, "回车").performClick()
            findKey(strokePanel, "语音").performClick()
            findKey(strokePanel, "符号").performClick()
            findKey(strokePanel, "返回").performClick()
            findKey(strokePanel, "7").performClick()
        }
        assertEquals(listOf("backspace", "enter", "voice", "toggle_symbols", "back"), commands)
        assertEquals(listOf("7"), texts)
    }

    @Test
    fun symbolModeKeepsActionKeysAndUsesLatestPage() {
        val firstPage = listOf("α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ")
        val secondPage = listOf("A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L")
        onMain {
            buildStrokePanel()
            strokeRender(symbolMode = true, symbolPage = 0, symbolPageCount = 2, symbols = firstPage)
            val firstSymbol = findKey(strokePanel, "α")
            strokeRender(symbolMode = true, symbolPage = 1, symbolPageCount = 2, symbols = secondPage)
            firstSymbol.performClick()
            // root 修订后符号页每行保留一个动作键：3 行 × 5（4 符号 + 1 动作）+ footer 5。
            findKey(strokePanel, "退格").performClick()
            findKey(strokePanel, "回车").performClick()
            findKey(strokePanel, "语音").performClick()
            findKey(strokePanel, "▲").performClick()
            findKey(strokePanel, "▼").performClick()
            findKey(strokePanel, "返回").performClick()
        }
        assertEquals(listOf("A"), texts)
        assertEquals(listOf("backspace", "enter", "voice", "prev", "next", "back"), commands)
        val indicator = findTextMatching(strokePanel, Regex("\\d+/\\d+"))
        assertFalse("页码指示器不可点击", indicator.isClickable)
    }

    @Test
    fun strokeDigitLabelsExposeDigitAndStrokeGlyphOnTwoLines() {
        onMain {
            buildStrokePanel()
            strokeRender()
            val one = findKey(strokePanel, "一")
            assertTrue("标签应同时含数字 1 与笔画字形 一", one.text.toString().contains("1"))
            assertTrue("笔画键至少两行以容纳数字提示", one.maxLines >= 2 || one.lineCount >= 2)
        }
    }

    @Test
    fun clearPreeditTouchIsCanceledByCancelInteractions() {
        onMain {
            buildStrokePanel()
            strokeRender(preedit = "横竖")
            val clearKey = findKey(strokePanel, "✕")
            assertEquals(View.VISIBLE, clearKey.visibility)
            dispatch(clearKey, MotionEvent.ACTION_DOWN, clearKey.width / 2f, clearKey.height / 2f)
            strokePanel.cancelInteractions()
            dispatch(clearKey, MotionEvent.ACTION_UP, clearKey.width / 2f, clearKey.height / 2f)
        }
        assertFalse("cancelInteractions 应取消 preedit 清除键的进行中触摸", commands.contains("clear"))
    }

    @Test
    fun blockedStrokePanelIgnoresClicks() {
        onMain {
            buildStrokePanel()
            strokeRender()
            strokePanel.setBlocked(true)
            findKey(strokePanel, "一").performClick()
            findKey(strokePanel, "←").performClick()
            strokePanel.setBlocked(false)
            findKey(strokePanel, "一").performClick()
        }
        assertEquals(listOf("h"), commands)
    }

    @Test
    fun setBlockedCancelsInFlightKeyTouch() {
        onMain {
            buildStrokePanel()
            strokeRender()
            val key = findKey(strokePanel, "一")
            dispatch(key, MotionEvent.ACTION_DOWN, key.width / 2f, key.height / 2f)
            strokePanel.setBlocked(true)
            dispatch(key, MotionEvent.ACTION_UP, key.width / 2f, key.height / 2f)
            strokePanel.setBlocked(false)
        }
        assertEquals(emptyList<String>(), commands)
    }

    @Test
    fun candidateExpansionKeepsPanelHeight() {
        onMain {
            buildStrokePanel()
            strokeRender()
            strokePanel.submitCandidates(
                (0 until 20).map { ImeStrokePanel.Candidate("候选$it") {} },
            )
            layout(strokePanel, 1080, 900)
            val before = strokePanel.height
            findTagged(strokePanel, "ime-cand-expand").performClick()
            layout(strokePanel, 1080, 900)
            assertEquals("展开候选不得改变面板总高度", before, strokePanel.height)
        }
    }

    // ================= ImeClipboardPanel =================

    private val clipboardCommands = mutableListOf<String>()
    private val categories = mutableListOf<String>()
    private val selections = mutableListOf<String>()
    private lateinit var clipboardPanel: ImeClipboardPanel

    private fun clipItem(text: String): ImeClipboardPanel.Item = ImeClipboardPanel.Item(
        text = text,
        favorite = false,
        inDictionary = false,
        select = { selections.add(text) },
        toggleFavorite = { selections.add("favorite:$text") },
        remove = { selections.add("remove:$text") },
        toggleDictionary = { selections.add("dictionary:$text") },
    )

    private fun buildClipboardPanel() {
        clipboardCommands.clear()
        categories.clear()
        selections.clear()
        clipboardPanel = ImeClipboardPanel(
            context,
            palette,
            View(context),
            englishUi = false,
            onCommand = { clipboardCommands.add(it) },
            onCategory = { categories.add(it) },
        )
        layout(clipboardPanel, 1080, 900)
    }

    private fun clipRender(
        editorToken: Long = 1L,
        historyVisible: Boolean = false,
        selection: Boolean = false,
        category: String = "ALL",
        entries: List<ImeClipboardPanel.Item> = emptyList(),
        recent: List<ImeClipboardPanel.Item> = emptyList(),
    ) {
        clipboardPanel.render(editorToken, historyVisible, selection, category, entries, recent)
        layout(clipboardPanel, 1080, 900)
    }

    @Test
    fun clipboardEditingCommandsOnlyDispatchCommands() {
        onMain {
            buildClipboardPanel()
            clipRender(recent = listOf(clipItem("recent-a")))
            findTagged(clipboardPanel, "ime-edit-range").performClick()
            findTagged(clipboardPanel, "ime-edit-copy").performClick()
            findTagged(clipboardPanel, "ime-edit-paste").performClick()
            findTagged(clipboardPanel, "ime-edit-backspace").performClick()
            findTagged(clipboardPanel, "ime-clip-recent-0").performClick()
        }
        assertEquals(listOf("range", "copy", "paste", "backspace"), clipboardCommands)
        assertEquals(listOf("recent-a"), selections)
    }

    @Test
    fun clipboardSelectionStateMarksMoveKeysAndHistoryHoldEdits() {
        onMain {
            buildClipboardPanel()
            clipRender(selection = true)
            val left = findTagged(clipboardPanel, "ime-edit-left") as TextView
            assertTrue("选择态应高亮方向键", left.isSelected)
            findTagged(clipboardPanel, "ime-edit-history").performLongClick()
        }
        assertEquals(listOf("edit"), clipboardCommands)
    }

    @Test
    fun clipboardHistoryFiltersSelectsAndClears() {
        onMain {
            buildClipboardPanel()
            clipRender(historyVisible = true, category = "ALL", entries = listOf(clipItem("history-a"), clipItem("history-b")))
            findTagged(clipboardPanel, "ime-clip-filter-TEXT").performClick()
            findTagged(clipboardPanel, "ime-clip-item-1").performClick()
            findTagged(clipboardPanel, "ime-clip-favorite-0").performClick()
            findTagged(clipboardPanel, "ime-history-clear").performClick()
            findTagged(clipboardPanel, "ime-history-close").performClick()
        }
        assertEquals(listOf("TEXT"), categories)
        assertEquals(listOf("history-b", "favorite:history-a"), selections)
        assertEquals(listOf("clear_history", "close_history"), clipboardCommands)
    }

    @Test
    fun clipboardBlockedIgnoresEverything() {
        onMain {
            buildClipboardPanel()
            clipRender(historyVisible = true, entries = listOf(clipItem("blocked-a")))
            clipboardPanel.setBlocked(true)
            findTagged(clipboardPanel, "ime-clip-item-0").performClick()
            findTagged(clipboardPanel, "ime-edit-copy").performClick()
            findTagged(clipboardPanel, "ime-history-clear").performClick()
            clipboardPanel.setBlocked(false)
            findTagged(clipboardPanel, "ime-edit-copy").performClick()
        }
        assertEquals(emptyList<String>(), selections)
        assertEquals(listOf("copy"), clipboardCommands)
    }

    @Test
    fun clipboardStaleGenerationItemDoesNothing() {
        onMain {
            buildClipboardPanel()
            clipRender(historyVisible = true, entries = listOf(clipItem("first-a")))
            val staleItem = findTagged(clipboardPanel, "ime-clip-item-0")
            // 同一 editorToken 但新一轮 render：generation 递增，旧 View 引用必须失效。
            clipRender(historyVisible = true, entries = listOf(clipItem("second-a")))
            staleItem.performClick()
        }
        assertEquals(emptyList<String>(), selections)
    }

    @Test
    fun clipboardCancelInteractionsThenRenderRecovers() {
        onMain {
            buildClipboardPanel()
            clipRender(historyVisible = true, entries = listOf(clipItem("before-cancel")))
            clipboardPanel.cancelInteractions()
            clipRender(historyVisible = true, entries = listOf(clipItem("after-cancel")))
            findTagged(clipboardPanel, "ime-clip-item-0").performClick()
        }
        assertEquals(listOf("after-cancel"), selections)
    }

    @Test
    fun longPressActionIsDiscoverableForAccessibility() {
        var accessible = false
        var edits = 0
        onMain {
            val key = ImeActionButton(context, palette, "Edit", onAction = {}, onHold = { edits++ })
            accessible = key.isLongClickable && key.performLongClick()
        }
        assertTrue(accessible)
        assertEquals(1, edits)
    }
}

/*
 * 限制（未覆盖，需真机/附着窗口场景）：
 * 1) ImeActionButton 的 hold/repeat 定时器在 `!isAttachedToWindow` 时主动跳过（见 production guard），
 *    本测试不附着窗口，因此不覆盖长按语音（900ms）/连发退格；这些由根 Service 的仪器测试或真机覆盖。
 * 2) 不验证触感、音效、窗口插入与 IME 连接；不覆盖横屏/大屏的具体像素密度差异。
 * 3) 笔画键视觉截断（span 数字提示）用 `maxLines/lineCount + text 完整性` 近似断言，不是像素级截图比对。
 * 4) `ImeStrokePanel` 的符号页动作键（退格/回车/语音）与翻页后最新值断言针对 root 修订后的目标语义；
 *    在修订合入前会失败，这是预期的红灯。
 */
