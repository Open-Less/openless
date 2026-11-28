package com.openless.app

import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * 反馈 / 互动策略的本地 View 级测试（不附着窗口、不 sleep、不跑定时器 tick）。
 *
 * 约定与假设：
 * - 只在主线程构造/measure/layout（尺寸由测试显式给定），事件用真实
 *   `dispatchTouchEvent(DOWN/MOVE/UP/CANCEL)`；
 * - `holdDelayMs` 故意设为 30s，测试期间不推进 looper，因此不会触发 hold/repeat 计时器；
 * - 不触达 Service/Native/真实 vibrator，也不读写用户偏好；
 * - 断言全部在主线程外执行（主线程内只做调用与计数采集）。
 */
@RunWith(AndroidJUnit4::class)
class ImeInteractionFeedbackInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val palette = ImePalette.resolve(false)

    private fun onMain(block: () -> Unit) = instrumentation.runOnMainSync(block)

    private fun newButton(
        onAction: () -> Unit,
        onFeedback: () -> Unit,
        onHold: (() -> Unit)? = null,
        repeatOnHold: Boolean = false,
        interactionPolicy: ImeInteractionPolicy = ImeInteractionPolicy.DEFAULT,
        enabled: Boolean = true,
    ): ImeActionButton = ImeActionButton(
        context,
        palette,
        "键",
        description = "test key",
        onAction = onAction,
        onHold = onHold,
        repeatOnHold = repeatOnHold,
        holdDelayMs = 30_000L,
        onFeedback = onFeedback,
        interactionPolicy = interactionPolicy,
    ).apply { isEnabled = enabled }

    private fun layout(view: View, width: Int = 240, height: Int = 96) {
        view.measure(
            View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY),
        )
        view.layout(0, 0, width, height)
    }

    private fun dispatch(view: View, action: Int, x: Float, y: Float) {
        val now = SystemClock.uptimeMillis()
        val event = MotionEvent.obtain(now, now, action, x, y, 0)
        view.dispatchTouchEvent(event)
        event.recycle()
    }

    private fun centerX(view: View): Float = view.width / 2f
    private fun centerY(view: View): Float = view.height / 2f

    private fun tap(view: View) {
        val x = centerX(view)
        val y = centerY(view)
        dispatch(view, MotionEvent.ACTION_DOWN, x, y)
        dispatch(view, MotionEvent.ACTION_MOVE, x + 2f, y + 1f)
        dispatch(view, MotionEvent.ACTION_UP, x, y)
    }

    private fun findButton(root: View, predicate: (ImeActionButton) -> Boolean = { true }): ImeActionButton? {
        if (root is ImeActionButton && predicate(root)) return root
        if (root is ViewGroup) {
            for (index in 0 until root.childCount) {
                findButton(root.getChildAt(index), predicate)?.let { return it }
            }
        }
        return null
    }

    // --- ImeActionButton：反馈不重复 ---

    @Test
    fun tapFeedsDownOnceAndUpDoesNotDoubleFeedback() {
        var actions = 0
        var feedback = 0
        onMain {
            val button = newButton(onAction = { actions++ }, onFeedback = { feedback++ })
            layout(button)
            tap(button)
        }
        assertEquals("onAction 只应触发一次", 1, actions)
        assertEquals("DOWN 反馈一次，UP 不重复", 1, feedback)
    }

    @Test
    fun accessibilityPerformClickFeedsOnce() {
        var actions = 0
        var feedback = 0
        var handled = false
        onMain {
            val button = newButton(onAction = { actions++ }, onFeedback = { feedback++ })
            layout(button)
            handled = button.performClick()
        }
        assertTrue(handled)
        assertEquals(1, actions)
        assertEquals("无障碍 click 没有 DOWN，应补一次反馈", 1, feedback)
    }

    @Test
    fun accessibilityPerformLongClickCallsHoldNotPlainAction() {
        var actions = 0
        var holds = 0
        var feedback = 0
        var handled = false
        onMain {
            val button = newButton(
                onAction = { actions++ },
                onFeedback = { feedback++ },
                onHold = { holds++ },
            )
            layout(button)
            handled = button.performLongClick()
        }
        assertTrue(handled)
        assertEquals("长按必须走 onHold", 1, holds)
        assertEquals("长按不得触发普通 onAction", 0, actions)
        assertEquals(1, feedback)
    }

    // --- 互动策略 apply 语义 ---

    @Test
    fun samePolicyApplyDoesNotCancelInProgressTap() {
        var actions = 0
        var feedback = 0
        onMain {
            val button = newButton(onAction = { actions++ }, onFeedback = { feedback++ })
            layout(button)
            val x = centerX(button)
            val y = centerY(button)
            dispatch(button, MotionEvent.ACTION_DOWN, x, y)
            // 与 DEFAULT 等值（都 60ms）：不得取消进行中的触摸。
            button.applyInteractionPolicy(ImeInteractionPolicy.fromStoredRepeat(60L))
            dispatch(button, MotionEvent.ACTION_MOVE, x + 2f, y + 1f)
            dispatch(button, MotionEvent.ACTION_UP, x, y)
        }
        assertEquals("相同策略不应打断点击", 1, actions)
        assertEquals(1, feedback)
    }

    @Test
    fun differentPolicyApplyCancelsInProgressTap() {
        var actions = 0
        var feedback = 0
        onMain {
            val button = newButton(onAction = { actions++ }, onFeedback = { feedback++ })
            layout(button)
            val x = centerX(button)
            val y = centerY(button)
            dispatch(button, MotionEvent.ACTION_DOWN, x, y)
            button.applyInteractionPolicy(ImeInteractionPolicy.fromStoredRepeat(130L))
            dispatch(button, MotionEvent.ACTION_MOVE, x + 2f, y + 1f)
            dispatch(button, MotionEvent.ACTION_UP, x, y)
        }
        assertEquals("不同策略应中止进行中的点击", 0, actions)
        assertEquals("只有 DOWN 那一次反馈", 1, feedback)
    }

    @Test
    fun cancelAfterDownKeepsSingleFeedbackWithoutAction() {
        var actions = 0
        var feedback = 0
        onMain {
            val button = newButton(onAction = { actions++ }, onFeedback = { feedback++ })
            layout(button)
            val x = centerX(button)
            val y = centerY(button)
            dispatch(button, MotionEvent.ACTION_DOWN, x, y)
            dispatch(button, MotionEvent.ACTION_CANCEL, x, y)
        }
        assertEquals("ACTION_CANCEL 不得执行动作", 0, actions)
        assertEquals("CANCEL 只保留按下时的反馈", 1, feedback)
    }

    @Test
    fun disabledKeyDoesNotFeedOrAct() {
        var actions = 0
        var feedback = 0
        var clickHandled = true
        onMain {
            val button = newButton(
                onAction = { actions++ },
                onFeedback = { feedback++ },
                enabled = false,
            )
            layout(button)
            tap(button)
            clickHandled = button.performClick()
        }
        assertEquals(0, actions)
        // 实现约定：disabled 的 DOWN 在反馈前即被拒绝，因此不应有任何震动。
        assertEquals(0, feedback)
        assertFalse("disabled 的 performClick 应返回 false", clickHandled)
    }

    // --- Stroke / Clipboard 入口确实接上了 feedback/policy（非模型空参数） ---

    @Test
    fun strokePanelRoutesFeedbackAndCommandsToHostHooks() {
        var feedback = 0
        var business = 0
        var found = false
        onMain {
            val panel = ImeStrokePanel(
                context,
                palette,
                View(context),
                englishUi = false,
                onCommand = { business++ },
                onText = { business++ },
                onPunctuationPage = { business++ },
                onFeedback = { feedback++ },
            )
            layout(panel, width = 1080, height = 900)
            // 不同策略下也应更新子按键，再验证点击仍把 feedback 送到宿主 hook。
            panel.applyInteractionPolicy(ImeInteractionPolicy.fromStoredRepeat(120L))
            val key = findButton(panel) { it.isEnabled && it.visibility == View.VISIBLE }
            found = key != null
            key?.performClick()
        }
        assertTrue(found)
        assertEquals("Stroke 子按键的反馈应到达 onFeedback", 1, feedback)
        assertEquals("Stroke 子按键应触发真实业务回调", 1, business)
    }

    @Test
    fun clipboardPanelRoutesFeedbackAndCommandsToHostHooks() {
        var feedback = 0
        var clipboardKeyFound = false
        val commands = mutableListOf<String>()
        onMain {
            val panel = ImeClipboardPanel(
                context,
                palette,
                View(context),
                englishUi = false,
                onCommand = { commands += it },
                onCategory = { commands += "category:$it" },
                onFeedback = { feedback++ },
            )
            layout(panel, width = 1080, height = 900)
            panel.applyInteractionPolicy(ImeInteractionPolicy.fromStoredRepeat(140L))
            val copyKey = findButton(panel) { it.tag == "ime-edit-copy" }
            clipboardKeyFound = copyKey != null
            copyKey?.performClick()
        }
        assertTrue("剪贴板编辑网格应有 copy 键", clipboardKeyFound)
        assertEquals("Clipboard 子按键的反馈应到达 onFeedback", 1, feedback)
        assertEquals("Clipboard 子按键应触发真实命令回调", listOf("copy"), commands)
    }
}
