package com.openless.app

import android.graphics.PointF
import android.graphics.Rect
import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.ScrollView
import android.widget.TextView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * 真实 View 事件级别的仪器测试：不启动 Activity，不触达 native/Service/JNI。
 *
 * - 构造纯 `ImeTypingPanel`，伪造回调；固定 1080x900 测量/布局。
 * - 键盘按键通过 tag `ime-key-{id}` 定位，`offsetDescendantRectToMyCoords` 换算到面板坐标，
 *   再对 `panel.dispatchTouchEvent` 发送完整 DOWN/MOVE/UP/CANCEL 序列。
 * - 候选/展开是普通可点击 View，用 `performClick()`/`performLongClick()` 走 Android 自身的
 *   点击分发，不依赖源码字符串匹配。
 * - 不等待长按计时器（KeyGrid 的 hold/repeat 依赖 `isAttachedToWindow` 且测试不附着窗口），
 *   全程不使用 Thread.sleep。
 *
 * 断言覆盖：轻点单次提交、取消无输入、字母拖拽重定向、功能键拖拽取消、多指取消、
 * 候选点击/长按遗忘不误选、展开不改变面板高度、层切换替换按键、所有模型按键均渲染。
 */
@RunWith(AndroidJUnit4::class)
class ImeTypingPanelInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val palette = ImePalette.resolve(false)

    private lateinit var panel: ImeTypingPanel
    private val actions = mutableListOf<ImeKeyAction>()
    private val selections = mutableListOf<String>()
    private val voiceEnds = mutableListOf<Boolean>()
    private val cursorMoves = mutableListOf<Int>()
    private var deleteWordCount = 0
    private var clearPreeditCount = 0
    private var commitRawCount = 0
    private var forgotCount = 0
    private var feedbackCount = 0
    private var voiceStarts = 0

    private fun onMain(block: () -> Unit) = instrumentation.runOnMainSync(block)

    private fun buildPanel() {
        actions.clear()
        selections.clear()
        voiceEnds.clear()
        cursorMoves.clear()
        deleteWordCount = 0
        clearPreeditCount = 0
        commitRawCount = 0
        forgotCount = 0
        feedbackCount = 0
        voiceStarts = 0
        panel = ImeTypingPanel(
            context,
            palette,
            View(context),
            onAction = { actions += it },
            onFeedback = { feedbackCount++ },
            // 语音启动一律拒绝：本测试只验证键盘输入路径。
            onVoiceStart = { voiceStarts++; false },
            onVoiceEnd = { voiceEnds += it },
            onClearPreedit = { clearPreeditCount++ },
            onCommitRawPreedit = { commitRawCount++ },
            onCursorMove = { cursorMoves += it },
            onDeleteWord = { deleteWordCount++ },
        )
        renderKeyboard()
    }

    private fun renderKeyboard(
        layer: ImeKeyboardLayer = ImeKeyboardLayer.LETTERS,
        language: ImeTypingLanguage = ImeTypingLanguage.ENGLISH,
        letterCase: ImeLetterCase = ImeLetterCase.LOWER,
        enterLabel: String = "Enter",
    ) {
        panel.renderKeyboard(layer, language, letterCase, enterLabel, false)
        layoutPanel()
    }

    private fun layoutPanel(width: Int = 1080, height: Int = 900) {
        panel.measure(
            View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY),
        )
        panel.layout(0, 0, width, height)
    }

    // --- 视图查找与坐标 ---

    private fun findView(root: View, predicate: (View) -> Boolean): View? {
        if (predicate(root)) return root
        if (root is ViewGroup) {
            for (index in 0 until root.childCount) {
                findView(root.getChildAt(index), predicate)?.let { return it }
            }
        }
        return null
    }

    private fun taggedKey(id: String): View? = findView(panel) { it.tag == "ime-key-$id" }

    private fun keyCenter(tag: String): PointF {
        val key = taggedKey(tag) ?: throw AssertionError("未找到按键 tag=$tag")
        val rect = Rect(0, 0, key.width, key.height)
        panel.offsetDescendantRectToMyCoords(key, rect)
        return PointF(rect.exactCenterX(), rect.exactCenterY())
    }

    private fun countKeys(): Int {
        var count = 0
        fun walk(view: View) {
            if ((view.tag as? String)?.startsWith("ime-key-") == true) count++
            if (view is ViewGroup) {
                for (index in 0 until view.childCount) walk(view.getChildAt(index))
            }
        }
        walk(panel)
        return count
    }

    // --- 事件发送 ---

    private fun dispatch(action: Int, point: PointF) {
        val now = SystemClock.uptimeMillis()
        val event = MotionEvent.obtain(now, now, action, point.x, point.y, 0)
        panel.dispatchTouchEvent(event)
        event.recycle()
    }

    private fun dispatchTwoFinger(point: PointF) {
        val now = SystemClock.uptimeMillis()
        val properties = arrayOf(
            MotionEvent.PointerProperties().apply {
                id = 0
                toolType = MotionEvent.TOOL_TYPE_FINGER
            },
            MotionEvent.PointerProperties().apply {
                id = 1
                toolType = MotionEvent.TOOL_TYPE_FINGER
            },
        )
        val coords = arrayOf(
            MotionEvent.PointerCoords().apply {
                x = point.x
                y = point.y
                pressure = 1f
                size = 1f
            },
            MotionEvent.PointerCoords().apply {
                x = point.x + 12f
                y = point.y
                pressure = 1f
                size = 1f
            },
        )
        val event = MotionEvent.obtain(
            now,
            now,
            MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT),
            2,
            properties,
            coords,
            0,
            0,
            1f,
            1f,
            0,
            0,
            InputDevice.SOURCE_TOUCHSCREEN,
            0,
        )
        panel.dispatchTouchEvent(event)
        event.recycle()
    }

    private fun tapKey(tag: String) {
        val point = keyCenter(tag)
        dispatch(MotionEvent.ACTION_DOWN, point)
        dispatch(MotionEvent.ACTION_UP, point)
    }

    private fun pressKey(tag: String, thenMoveTo: String?) {
        val start = keyCenter(tag)
        dispatch(MotionEvent.ACTION_DOWN, start)
        if (thenMoveTo != null) {
            val end = keyCenter(thenMoveTo)
            dispatch(MotionEvent.ACTION_MOVE, end)
            dispatch(MotionEvent.ACTION_UP, end)
        } else {
            dispatch(MotionEvent.ACTION_UP, start)
        }
    }

    // --- 键盘基本提交 ---

    @Test
    fun letterTapCommitsExactlyOnce() {
        onMain { buildPanel(); tapKey("letters_r1_c1") }
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Text("q")), actions)
    }

    @Test
    fun enterTapCommitsOnceWithConfiguredLabel() {
        var defaultEnterActions: List<ImeKeyAction> = emptyList()
        var searchEnterActions: List<ImeKeyAction> = emptyList()
        var searchLabel: String? = null
        onMain {
            buildPanel()
            tapKey("enter")
            defaultEnterActions = actions.toList()
        }
        onMain {
            buildPanel()
            renderKeyboard(enterLabel = "搜索")
            searchLabel = (taggedKey("enter") as? TextView)?.text?.toString()
            tapKey("enter")
            searchEnterActions = actions.toList()
        }
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Enter), defaultEnterActions)
        assertEquals("搜索", searchLabel)
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Enter), searchEnterActions)
    }

    @Test
    fun spaceTapCommitsSpaceAndNeverStartsVoice() {
        onMain { buildPanel(); tapKey("space") }
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Space), actions)
        assertEquals(0, voiceStarts)
        assertTrue("轻点空格不应结束语音会话", voiceEnds.isEmpty())
    }

    @Test
    fun backspaceTapCommitsOnceWithoutRepeat() {
        onMain { buildPanel(); tapKey("backspace") }
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Backspace), actions)
    }

    @Test
    fun functionalTapCommitsOnce() {
        onMain { buildPanel(); tapKey("shift") }
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Shift), actions)
    }

    // --- 取消与拖拽 ---

    @Test
    fun cancelActionProducesNoInput() {
        onMain {
            buildPanel()
            val point = keyCenter("letters_r1_c1")
            dispatch(MotionEvent.ACTION_DOWN, point)
            dispatch(MotionEvent.ACTION_CANCEL, point)
        }
        assertTrue("ACTION_CANCEL 后不应有任何提交", actions.isEmpty())
    }

    @Test
    fun letterDragRetargetsToFinalKeyOnly() {
        onMain { buildPanel(); pressKey("letters_r1_c1", thenMoveTo = "letters_r1_c2") }
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Text("w")), actions)
    }

    @Test
    fun functionalKeyDragCancelsInsteadOfRetargeting() {
        onMain { buildPanel(); pressKey("shift", thenMoveTo = "letters_r3_c1") }
        assertTrue("功能键拖到字母键应取消，不应提交", actions.isEmpty())
    }

    @Test
    fun multiPointerRolloverCommitsBothKeysInOrder() {
        onMain {
            buildPanel()
            val first = keyCenter("letters_r1_c1")
            val second = keyCenter("letters_r1_c2")
            val now = SystemClock.uptimeMillis()
            val down = MotionEvent.obtain(now, now, MotionEvent.ACTION_DOWN, first.x, first.y, 0)
            panel.dispatchTouchEvent(down)
            down.recycle()

            val props = arrayOf(
                MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_FINGER },
                MotionEvent.PointerProperties().apply { id = 1; toolType = MotionEvent.TOOL_TYPE_FINGER },
            )
            val coords = arrayOf(
                MotionEvent.PointerCoords().apply { x = first.x; y = first.y; pressure = 1f; size = 1f },
                MotionEvent.PointerCoords().apply { x = second.x; y = second.y; pressure = 1f; size = 1f },
            )
            val ptrDown = MotionEvent.obtain(
                now, now + 10,
                MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT),
                2, props, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0,
            )
            panel.dispatchTouchEvent(ptrDown)
            ptrDown.recycle()

            val ptrUp = MotionEvent.obtain(
                now, now + 20,
                MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT),
                2, props, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0,
            )
            panel.dispatchTouchEvent(ptrUp)
            ptrUp.recycle()
        }
        assertEquals(
            listOf<ImeKeyAction>(ImeKeyAction.Text("q"), ImeKeyAction.Text("w")),
            actions,
        )
    }

    @Test
    fun swipeUpOnLetterKeyCommitsSecondaryDigit() {
        onMain {
            buildPanel()
            val start = keyCenter("letters_r1_c1")
            val up = PointF(start.x, start.y - 120f)
            dispatch(MotionEvent.ACTION_DOWN, start)
            dispatch(MotionEvent.ACTION_MOVE, up)
            dispatch(MotionEvent.ACTION_UP, up)
        }
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Text("1")), actions)
    }

    @Test
    fun spaceHorizontalSlideMovesCursorWithoutInsertingSpace() {
        onMain {
            buildPanel()
            val start = keyCenter("space")
            dispatch(MotionEvent.ACTION_DOWN, start)
            dispatch(MotionEvent.ACTION_MOVE, PointF(start.x + 120f, start.y))
            dispatch(MotionEvent.ACTION_UP, PointF(start.x + 120f, start.y))
        }
        assertTrue("空格滑动不应提交空格", actions.isEmpty())
        assertTrue("空格右滑应触发右移光标", cursorMoves.isNotEmpty() && cursorMoves.all { it == 1 })
    }

    @Test
    fun backspaceSwipeLeftTriggersWordDeleteWithoutPlainBackspace() {
        onMain {
            buildPanel()
            val start = keyCenter("backspace")
            val left = PointF(start.x - 160f, start.y)
            dispatch(MotionEvent.ACTION_DOWN, start)
            dispatch(MotionEvent.ACTION_MOVE, left)
            dispatch(MotionEvent.ACTION_UP, left)
        }
        assertTrue("退格左滑不应提交普通退格", actions.isEmpty())
        assertEquals(1, deleteWordCount)
    }

    @Test
    fun preeditBarTapCommitsRawAndClearButtonClearsEncoding() {
        onMain {
            buildPanel()
            panel.submitCandidates(listOf(ImeTypingPanel.Candidate("你好", select = {})), preedit = "ni hao")
            layoutPanel()
            findView(panel) { it.tag == "ime-cand-preedit" }?.performClick()
            findView(panel) { it.tag == "ime-preedit-clear" }?.performClick()
        }
        assertEquals(1, commitRawCount)
        assertEquals(1, clearPreeditCount)
    }

    @Test
    fun quickClipboardChipPastesAndDismisses() {
        var pasted = 0
        var dismissed = 0
        onMain {
            buildPanel()
            panel.setClipboardSuggestion("验证码: 123456", onPaste = { pasted++ }, onDismiss = { dismissed++ })
            layoutPanel()
            findView(panel) { it.tag == "ime-cand-quick-chip" }?.performClick()
        }
        assertEquals(1, pasted)
        assertEquals(0, dismissed)
    }

    @Test
    fun casingUpdateDuringTouchKeepsTheCurrentPointer() {
        onMain {
            buildPanel()
            val point = keyCenter("letters_r1_c1")
            dispatch(MotionEvent.ACTION_DOWN, point)
            renderKeyboard(letterCase = ImeLetterCase.SHIFT)
            dispatch(MotionEvent.ACTION_UP, point)
        }
        assertEquals(listOf<ImeKeyAction>(ImeKeyAction.Text("Q")), actions)
    }

    // --- 候选 ---

    @Test
    fun candidateTapSelectsOnlyTheClickedCandidate() {
        var thirdCandidate: View? = null
        onMain {
            buildPanel()
            panel.submitCandidates(
                listOf(
                    ImeTypingPanel.Candidate(label = "一", select = { selections += "一" }),
                    ImeTypingPanel.Candidate(label = "二", select = { selections += "二" }),
                    ImeTypingPanel.Candidate(label = "三", select = { selections += "三" }),
                ),
                preedit = "yi",
            )
            layoutPanel()
            thirdCandidate = findView(panel) { it.contentDescription == "三" }
            thirdCandidate?.performClick()
        }
        assertNotNull("候选三应渲染", thirdCandidate)
        assertEquals(listOf("三"), selections)
        assertTrue("候选点击不应触发键盘动作", actions.isEmpty())
    }

    @Test
    fun candidateLongPressForgetsWithoutSelecting() {
        var candidate: View? = null
        var handled = false
        onMain {
            buildPanel()
            panel.submitCandidates(
                listOf(
                    ImeTypingPanel.Candidate(
                        label = "自定义",
                        select = { selections += "自定义" },
                        forget = { forgotCount++ },
                    ),
                ),
                preedit = null,
            )
            layoutPanel()
            candidate = findView(panel) { it.contentDescription == "自定义" }
            handled = candidate?.performLongClick() == true
        }
        assertNotNull("带 forget 的候选应渲染", candidate)
        assertTrue("长按应被消费", handled)
        assertEquals(1, forgotCount)
        assertTrue("长按遗忘不应同时选中", selections.isEmpty())
    }

    @Test
    fun candidateWithoutForgetIgnoresLongPress() {
        var candidate: View? = null
        var handled = true
        onMain {
            buildPanel()
            panel.submitCandidates(
                listOf(ImeTypingPanel.Candidate(label = "普通", select = { selections += "普通" })),
                preedit = null,
            )
            layoutPanel()
            candidate = findView(panel) { it.contentDescription == "普通" }
            handled = candidate?.performLongClick() == true
        }
        assertNotNull(candidate)
        assertFalse("无 forget 回调时不应消费长按", handled)
        assertTrue(selections.isEmpty())
    }

    @Test
    fun equalCandidateLabelsUseTheLatestCallback() {
        onMain {
            buildPanel()
            panel.submitCandidates(listOf(ImeTypingPanel.Candidate("词", select = { selections += "old" })), null)
            val item = findView(panel) { it.contentDescription == "词" }
            panel.submitCandidates(listOf(ImeTypingPanel.Candidate("词", select = { selections += "new" })), null)
            item?.performClick()
        }
        assertEquals(listOf("new"), selections)
    }

    @Test
    fun removedCandidateViewCannotSelectAReplacement() {
        onMain {
            buildPanel()
            panel.submitCandidates(listOf(ImeTypingPanel.Candidate("旧", select = { selections += "old" })), null)
            val stale = findView(panel) { it.contentDescription == "旧" }
            panel.submitCandidates(listOf(ImeTypingPanel.Candidate("新", select = { selections += "new" })), null)
            stale?.performClick()
        }
        assertTrue(selections.isEmpty())
    }

    // --- 展开候选 ---

    @Test
    fun expandingCandidatesKeepsPanelHeight() {
        var heightBefore = 0
        var heightAfter = 0
        var expandedVisible = false
        var expandFound = false
        var gridStillVisible = false
        onMain {
            buildPanel()
            panel.submitCandidates(
                listOf(
                    ImeTypingPanel.Candidate(label = "甲", select = {}),
                    ImeTypingPanel.Candidate(label = "乙", select = {}),
                ),
                preedit = null,
            )
            layoutPanel()
            heightBefore = panel.measuredHeight

            val expand = findView(panel) {
                it.isClickable && it.contentDescription?.contains("候选") == true
            }
            expandFound = expand != null
            expand?.performClick()
            layoutPanel()
            heightAfter = panel.measuredHeight

            expandedVisible = findView(panel) { it is ScrollView }?.visibility == View.VISIBLE
            gridStillVisible = taggedKey("letters_r1_c1") != null
        }
        assertTrue("展开按钮应存在", expandFound)
        assertTrue("展开后键盘仍应可见", gridStillVisible)
        assertEquals(900, heightBefore)
        assertEquals("展开候选不应改变面板高度", heightBefore, heightAfter)
        assertTrue("展开层应可见", expandedVisible)
        assertTrue("展开不应提交任何按键动作", actions.isEmpty())
    }

    // --- 层切换与渲染完整性 ---

    @Test
    fun switchingToNumbersReplacesLetterKeys() {
        var numbersKeyPresent = false
        var lettersKeyGone = false
        onMain {
            buildPanel()
            renderKeyboard(layer = ImeKeyboardLayer.NUMBERS)
            numbersKeyPresent = taggedKey("numbers_r1_c1") != null
            lettersKeyGone = taggedKey("letters_r1_c1") == null
        }
        assertTrue(numbersKeyPresent)
        assertTrue(lettersKeyGone)
    }

    @Test
    fun everyModelKeyIsRenderedWithATag() {
        var rendered = 0
        val expected = ImeKeyboardLayouts.rows(
            ImeKeyboardLayer.LETTERS,
            ImeTypingLanguage.ENGLISH,
            ImeLetterCase.LOWER,
            "Enter",
        ).flatten().size
        onMain {
            buildPanel()
            rendered = countKeys()
        }
        assertEquals(expected, rendered)
    }

    @Test
    fun closeTransientUiCollapsesCandidatesWithoutCommitting() {
        var expandedVisible = false
        onMain {
            buildPanel()
            panel.submitCandidates(listOf(ImeTypingPanel.Candidate("候", select = {})), preedit = null)
            layoutPanel()
            findView(panel) { it.isClickable && it.contentDescription?.contains("候选") == true }?.performClick()
            panel.closeTransientUi()
            layoutPanel()
            expandedVisible = findView(panel) { it is ScrollView }?.visibility == View.VISIBLE
        }
        assertFalse("closeTransientUi 应收起候选展开层", expandedVisible)
        assertTrue(actions.isEmpty())
    }
}
