package com.openless.app

import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Event-level coverage of the voice surface without microphone/network/JNI. */
@RunWith(AndroidJUnit4::class)
class ImeVoicePanelInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val actions = mutableListOf<ImeVoiceGesture>()
    private lateinit var button: ImeVoiceButton
    private var startedAt = 0L

    private fun prepare(recording: Boolean = false) {
        actions.clear()
        val context = instrumentation.targetContext
        button = ImeVoiceButton(context, ImePalette.resolve(false)) { actions += it }
        button.isRecording = recording
        val density = context.resources.displayMetrics.density
        button.measure(
            View.MeasureSpec.makeMeasureSpec((180 * density).toInt(), View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec((72 * density).toInt(), View.MeasureSpec.EXACTLY),
        )
        button.layout(0, 0, button.measuredWidth, button.measuredHeight)
        startedAt = SystemClock.uptimeMillis()
    }

    private fun event(action: Int, dxDp: Float = 0f, dyDp: Float = 0f) {
        val density = button.resources.displayMetrics.density
        val event = MotionEvent.obtain(
            startedAt, SystemClock.uptimeMillis(), action,
            button.width / 2f + dxDp * density,
            button.height / 2f + dyDp * density, 0,
        )
        button.dispatchTouchEvent(event)
        event.recycle()
    }

    private fun swipe(dxDp: Float, dyDp: Float, recording: Boolean = false) {
        instrumentation.runOnMainSync {
            prepare(recording)
            event(MotionEvent.ACTION_DOWN)
            event(MotionEvent.ACTION_MOVE, dxDp, dyDp)
            event(MotionEvent.ACTION_UP, dxDp, dyDp)
        }
    }

    @Test fun tapProducesExactlyOneAction() {
        instrumentation.runOnMainSync {
            prepare()
            event(MotionEvent.ACTION_DOWN)
            event(MotionEvent.ACTION_UP)
        }
        assertEquals(listOf(ImeVoiceGesture.TAP), actions)
    }

    @Test fun swipeUpRequestsRawModeOnce() {
        swipe(0f, -70f)
        assertEquals(listOf(ImeVoiceGesture.RAW), actions)
    }

    @Test fun swipeLeftRequestsQuickNoteOnce() {
        swipe(-70f, 0f)
        assertEquals(listOf(ImeVoiceGesture.QUICK_NOTE), actions)
    }

    @Test fun swipeRightRequestsCloudNoteOnce() {
        swipe(70f, 0f)
        assertEquals(listOf(ImeVoiceGesture.CLOUD_NOTE), actions)
    }

    @Test fun swipeDownCancelsOnlyAnActiveRecording() {
        swipe(0f, 70f, recording = true)
        assertEquals(listOf(ImeVoiceGesture.CANCEL), actions)
    }

    @Test fun downwardDragWhileIdleDoesNotStartRecording() {
        swipe(0f, 70f)
        assertTrue(actions.isEmpty())
    }

    @Test fun cancelledTouchNeverCommitsThePreviewedAction() {
        instrumentation.runOnMainSync {
            prepare(recording = true)
            event(MotionEvent.ACTION_DOWN)
            event(MotionEvent.ACTION_MOVE, 0f, 70f)
            event(MotionEvent.ACTION_CANCEL, 0f, 70f)
        }
        assertTrue(actions.isEmpty())
    }

    @Test fun terminalCoordinatesWinOverTheLastMovePreview() {
        instrumentation.runOnMainSync {
            prepare()
            event(MotionEvent.ACTION_DOWN)
            event(MotionEvent.ACTION_MOVE, 70f, 0f)
            event(MotionEvent.ACTION_UP, -70f, 0f)
        }
        assertEquals(listOf(ImeVoiceGesture.QUICK_NOTE), actions)
    }

    @Test fun returningToTheNeutralZoneClearsTheSwipePreview() {
        instrumentation.runOnMainSync {
            prepare()
            event(MotionEvent.ACTION_DOWN)
            event(MotionEvent.ACTION_MOVE, 70f, 0f)
            event(MotionEvent.ACTION_MOVE)
            event(MotionEvent.ACTION_UP)
        }
        assertEquals(listOf(ImeVoiceGesture.TAP), actions)
    }

    @Test fun freshTapAfterCancellationDoesNotReuseOldGestureState() {
        instrumentation.runOnMainSync {
            prepare()
            event(MotionEvent.ACTION_DOWN)
            event(MotionEvent.ACTION_MOVE, 70f, 0f)
            event(MotionEvent.ACTION_CANCEL, 70f, 0f)
            event(MotionEvent.ACTION_DOWN)
            event(MotionEvent.ACTION_UP)
        }
        assertEquals(listOf(ImeVoiceGesture.TAP), actions)
    }
}
