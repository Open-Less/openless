package com.openless.app

import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.FrameLayout
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * 独立草稿栏的 View 级仪器测试：不启动 Activity、不触达 Service/JNI/InputConnection/网络。
 *
 * - 真实 [EditText] + fake 回调，`instrumentation.runOnMainSync` 内构建/布局/点击；
 * - 覆盖：重挂保留 editor 内容与选区、render 不写 editor、confirm/cancel 单次且各一次反馈、
 *   submitting 真正禁用、错误态色/可读 a11y、confirm accent、detach 后旧挂起聚焦不夺权、
 *   不同 AT_MOST 尺寸下控件不重叠可点击、cancelInteractions 清 pressed。
 *
 * 限制：测试不附着真实窗口（`isAttachedToWindow` 恒为 false），因此只断言"父节点归属/无夺权"
 * 这类结构性约束；真实光标聚焦需要真机/Activity 场景，本文件不声称已实测。
 */
@RunWith(AndroidJUnit4::class)
class ImeDraftBarInstrumentedTest {

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val palette = ImePalette.resolve(false)

    private fun onMain(block: () -> Unit) = instrumentation.runOnMainSync(block)

    private fun dp(value: Int): Int = (value * context.resources.displayMetrics.density).toInt()

    private var cancelCount = 0
    private var confirmCount = 0
    private var feedbackCount = 0

    private fun newEditor(text: String = "草稿内容 draft"): EditText = EditText(context).apply {
        setText(text)
        setSelection(minOf(2, text.length), minOf(5, text.length))
    }

    private fun buildBar(editor: EditText): ImeDraftBar = ImeDraftBar(
        context,
        palette,
        editor,
        onCancel = { cancelCount++ },
        onConfirm = { confirmCount++ },
        onFeedback = { feedbackCount++ },
    )

    private fun host(view: View, width: Int = dp(360), height: Int = dp(240)): FrameLayout {
        val host = FrameLayout(context)
        host.addView(
            view,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
            ),
        )
        layout(host, width, height)
        return host
    }

    private fun layout(view: View, width: Int, height: Int) {
        view.measure(
            View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.AT_MOST),
            View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.AT_MOST),
        )
        view.layout(0, 0, view.measuredWidth, view.measuredHeight)
    }

    private fun resetCounters() {
        cancelCount = 0
        confirmCount = 0
        feedbackCount = 0
    }

    @Test
    fun reattachKeepsEditorContentAndSelectionWithoutWritingIt() {
        lateinit var editor: EditText
        lateinit var oldParent: FrameLayout
        lateinit var bar: ImeDraftBar
        onMain {
            editor = newEditor()
            oldParent = FrameLayout(context)
            oldParent.addView(editor)
            bar = buildBar(editor)
        }
        assertSame("editor 必须被搬到新栏", bar, editor.parent)
        assertEquals(0, oldParent.childCount)
        assertEquals("草稿内容 draft", editor.text.toString())
        assertEquals(2, editor.selectionStart)
        assertEquals(5, editor.selectionEnd)

        onMain { bar.render("编辑中", isError = false, submitting = false) }

        assertEquals("render 不得改写 editor 文本", "草稿内容 draft", editor.text.toString())
        assertEquals(2, editor.selectionStart)
        assertEquals(5, editor.selectionEnd)
    }

    @Test
    fun confirmAndCancelFireOnceEachWithOneFeedbackPerClick() {
        lateinit var bar: ImeDraftBar
        onMain {
            resetCounters()
            bar = buildBar(newEditor())
            host(bar)
        }
        onMain { bar.confirm.performClick() }
        assertEquals(1, confirmCount)
        assertEquals(0, cancelCount)
        assertEquals(1, feedbackCount)

        onMain { bar.cancel.performClick() }
        assertEquals(1, confirmCount)
        assertEquals(1, cancelCount)
        assertEquals(2, feedbackCount)
    }

    @Test
    fun submittingDisablesBothButtonsAndBlocksCallbacks() {
        lateinit var bar: ImeDraftBar
        onMain {
            resetCounters()
            bar = buildBar(newEditor())
            host(bar)
            bar.render("提交中", isError = false, submitting = true)
        }
        assertFalse(bar.confirm.isEnabled)
        assertFalse(bar.cancel.isEnabled)

        onMain {
            bar.confirm.performClick()
            bar.cancel.performClick()
        }
        assertEquals(0, confirmCount)
        assertEquals(0, cancelCount)
        assertEquals(0, feedbackCount)

        onMain {
            bar.render("编辑中", isError = false, submitting = false)
            bar.confirm.performClick()
        }
        assertTrue(bar.confirm.isEnabled)
        assertTrue(bar.cancel.isEnabled)
        assertEquals(1, confirmCount)
        assertEquals(1, feedbackCount)
    }

    @Test
    fun errorStateUsesPaletteErrorAndReadableDescription() {
        lateinit var bar: ImeDraftBar
        onMain {
            bar = buildBar(newEditor())
            host(bar)
            bar.render("保存失败", isError = true, submitting = false)
        }
        assertEquals("保存失败", bar.status.text.toString())
        assertEquals(palette.error, bar.status.currentTextColor)
        assertEquals("保存失败", bar.status.contentDescription?.toString())

        onMain { bar.render("已就绪", isError = false, submitting = false) }
        assertEquals(palette.secondaryText, bar.status.currentTextColor)
        assertNull(bar.status.contentDescription)
    }

    @Test
    fun confirmAccentIsAppliedToConfirmBackground() {
        // GradientDrawable.getColor() 仅 API 24+ 暴露；低版本设备跳过该色值读取。
        org.junit.Assume.assumeTrue(Build.VERSION.SDK_INT >= Build.VERSION_CODES.N)
        lateinit var bar: ImeDraftBar
        val accent = 0xFF00C853.toInt()
        onMain {
            bar = buildBar(newEditor())
            host(bar)
            bar.render("编辑中", isError = false, submitting = false, confirmAccent = accent)
        }
        val background = bar.confirm.background as GradientDrawable
        assertEquals(accent, background.color?.defaultColor)
    }

    @Test
    fun detachedBarCannotRepostFocusAfterEditorMovesToAnotherBar() {
        lateinit var editor: EditText
        lateinit var firstBar: ImeDraftBar
        lateinit var secondBar: ImeDraftBar
        val firstHost = FrameLayout(context)
        val secondHost = FrameLayout(context)
        onMain {
            editor = newEditor()
            firstBar = buildBar(editor)
            firstHost.addView(firstBar)
        }
        onMain {
            firstHost.removeView(firstBar)
            secondBar = buildBar(editor)
            secondHost.addView(secondBar)
        }
        instrumentation.waitForIdleSync()

        assertSame("重挂后 editor 必须归第二栏", secondBar, editor.parent)
        assertNotSame(firstBar, editor.parent)
        onMain { firstBar.cancelInteractions() }
        assertSame("旧栏取消交互不得夺回 editor", secondBar, editor.parent)
    }

    @Test
    fun atMostConstraintsKeepControlsNonOverlappingAndClickable() {
        lateinit var bar: ImeDraftBar
        onMain {
            bar = buildBar(newEditor("短草稿"))
            layout(bar, dp(320), dp(240))
        }
        assertTrue("应尊重 AT_MOST 高度", bar.measuredHeight <= dp(240))
        assertTrue(bar.cancel.height > 0)
        assertTrue(bar.confirm.height > 0)
        assertTrue("cancel 不得与 status 重叠", bar.cancel.right <= bar.status.left)
        assertTrue("status 不得与 confirm 重叠", bar.status.right <= bar.confirm.left)
        assertTrue("confirm 必须在栏内", bar.confirm.right <= bar.width)

        onMain {
            layout(bar, dp(1080), dp(900))
        }
        assertTrue(bar.confirm.width > 0)
        assertTrue(bar.confirm.right <= bar.width)
    }

    @Test
    fun cancelInteractionsClearsPressedStateAndDoesNotCallBack() {
        lateinit var bar: ImeDraftBar
        onMain {
            resetCounters()
            bar = buildBar(newEditor())
            host(bar)
        }
        onMain {
            bar.cancel.isPressed = true
            bar.confirm.isPressed = true
            bar.cancelInteractions()
        }
        assertFalse(bar.cancel.isPressed)
        assertFalse(bar.confirm.isPressed)
        assertEquals(0, cancelCount)
        assertEquals(0, confirmCount)
        assertEquals(0, feedbackCount)
    }
}
