package com.openless.app

import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * 两个收敛点的真实 View callback 测试（不只看文字）：
 * 1) 父级 `isEnabled=false` 时，clear / 候选 select 都不能触发业务回调；重新启用后恢复；
 * 2) 稀疏 symbols → 同 geometry 更新为更满 symbols 时，新出现的非空槽恢复可点且回调一次。
 *
 * 不附着窗口、不 sleep、不跑定时器 tick；断言在主线程外。
 */
@RunWith(AndroidJUnit4::class)
class ImePanelInteractionEdgeCasesInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val palette = ImePalette.resolve(false)

    private fun onMain(block: () -> Unit) = instrumentation.runOnMainSync(block)

    private fun layout(view: View, width: Int = 1080, height: Int = 900) {
        view.measure(
            View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY),
        )
        view.layout(0, 0, width, height)
    }

    private fun findView(root: View, predicate: (View) -> Boolean): View? {
        if (predicate(root)) return root
        if (root is ViewGroup) {
            for (index in 0 until root.childCount) {
                findView(root.getChildAt(index), predicate)?.let { return it }
            }
        }
        return null
    }

    private fun dispatch(view: View, action: Int, x: Float, y: Float) {
        val now = SystemClock.uptimeMillis()
        val event = MotionEvent.obtain(now, now, action, x, y, 0)
        view.dispatchTouchEvent(event)
        event.recycle()
    }

    @Test
    fun parentDisabledBlocksClearAndCandidateSelectWithRealCallbacks() {
        val commands = mutableListOf<String>()
        val texts = mutableListOf<String>()
        val selections = mutableListOf<String>()
        val pages = mutableListOf<Int>()
        var feedback = 0
        var clearClickedWhenDisabled = true
        var businessClicksWhenDisabled = 0
        var businessClicksAfterEnable = 0
        var selectWhenDisabled = 0
        var selectAfterEnable = 0

        onMain {
            val panel = ImeStrokePanel(
                context,
                palette,
                View(context),
                englishUi = false,
                onCommand = { commands += it },
                onText = { texts += it },
                onPunctuationPage = { pages += it },
                onFeedback = { feedback++ },
            )
            layout(panel)
            panel.submitCandidates(listOf(ImeStrokePanel.Candidate("候") { selections += "候" }))

            val clearKey = findView(panel) { it is ImeActionButton && it.text == "✕" } as ImeActionButton?
            val businessKey = findView(panel) {
                it is ImeActionButton && it.text.isNotEmpty() && it.text != "✕"
            } as ImeActionButton?
            val candidate = findView(panel) { it.contentDescription == "候" }

            panel.isEnabled = false
            clearClickedWhenDisabled = clearKey?.performClick() ?: false
            businessKey?.performClick()
            businessClicksWhenDisabled = commands.size + texts.size + pages.size
            candidate?.performClick()
            selectWhenDisabled = selections.size

            // 恢复父级后，同一批 View 应重新可用（证明禁用不是永久破坏）。
            panel.isEnabled = true
            businessKey?.performClick()
            businessClicksAfterEnable = commands.size + texts.size + pages.size
            candidate?.performClick()
            selectAfterEnable = selections.size
        }

        assertTrue("禁用时 clear 的 performClick 应返回 false", !clearClickedWhenDisabled)
        assertEquals("禁用时 clear 不得触发 onCommand(\"clear\")", 0, commands.count { it == "clear" })
        assertEquals("禁用时任何业务键都不得回调", 0, businessClicksWhenDisabled)
        assertEquals("禁用时候选 select 不得触发", 0, selectWhenDisabled)
        assertEquals("恢复后业务键应回调一次", 1, businessClicksAfterEnable)
        assertEquals("恢复后候选 select 应回调一次", 1, selectAfterEnable)
        assertEquals(listOf("候"), selections)
    }

    @Test
    fun sparseSymbolPageThenFullPageRestoresClickableSlots() {
        val texts = mutableListOf<String>()
        var feedback = 0
        var slotWasDisabledInitially = false
        var sameViewReused = false
        var slotEnabledAfterUpdate = false
        var textsAfterClick = 0

        onMain {
            val panel = ImeStrokePanel(
                context,
                palette,
                View(context),
                englishUi = false,
                onCommand = {},
                onText = { texts += it },
                onPunctuationPage = {},
                onFeedback = { feedback++ },
            )
            layout(panel)
            // 稀疏符号页：只有 3 项，其余槽为空 -> 构建时 INVISIBLE + disabled。
            panel.render("", false, numberMode = true, symbolMode = true, symbolPage = 0, symbolPageCount = 2,
                symbols = listOf("A", "B", "C"), punctuation = emptyList())
            val sparseSlot = findView(panel) { it is ImeActionButton && it.text.isEmpty() } as ImeActionButton?
            slotWasDisabledInitially = sparseSlot != null && !sparseSlot.isEnabled

            // 同 geometry（不重建）更新为满 12 项：原空槽变 "D" 且必须恢复可点。
            panel.render("", false, numberMode = true, symbolMode = true, symbolPage = 1, symbolPageCount = 2,
                symbols = listOf("A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L"),
                punctuation = emptyList())
            val slotAfter = findView(panel) { it is ImeActionButton && it.text == "D" } as ImeActionButton?
            sameViewReused = sparseSlot != null && sparseSlot === slotAfter
            slotEnabledAfterUpdate = slotAfter?.isEnabled == true
            slotAfter?.performClick()
            textsAfterClick = texts.size
        }

        assertTrue("稀疏页的空槽初始应为 disabled", slotWasDisabledInitially)
        assertTrue("同 geometry 更新应复用同一个 View（不重建）", sameViewReused)
        assertTrue("新非空槽必须恢复 enabled", slotEnabledAfterUpdate)
        assertEquals("点击新非空槽应触发 onText 一次", 1, textsAfterClick)
        assertEquals(listOf("D"), texts)
        assertEquals("无障碍 click 应反馈一次", 1, feedback)
    }
}
