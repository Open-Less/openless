package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ImeGestureSessionTest {
    private val gesture = ImeGestureSession(touchSlop = 8f)

    @Test fun tapDoesNotBecomeAModeSwipe() {
        gesture.begin(0f, 0f, horizontal = true, down = true)
        assertEquals(ImeGestureSession.Axis.NONE, gesture.move(4f, 2f))
        assertNull(release(4f, 2f))
    }

    @Test fun deliberateLeftSwipeCommitsOnce() {
        gesture.begin(200f, 0f, horizontal = true)
        assertEquals(ImeGestureSession.Axis.HORIZONTAL, gesture.move(180f, 0f))
        assertEquals(ImeGestureSession.Direction.LEFT, release(0f, 0f))
        assertNull(release(0f, 0f))
    }

    @Test fun deliberateRightSwipeCommits() {
        gesture.begin(0f, 0f, horizontal = true)
        gesture.move(20f, 0f)
        assertEquals(ImeGestureSession.Direction.RIGHT, release(150f, 0f))
    }

    @Test fun touchSlopClaimsButDoesNotCommitAnAccidentalDrag() {
        gesture.begin(0f, 0f, horizontal = true)
        assertEquals(ImeGestureSession.Axis.HORIZONTAL, gesture.move(15f, 0f))
        assertNull(release(25f, 0f))
    }

    @Test fun cancelledSwipeNeverCommits() {
        gesture.begin(0f, 0f, horizontal = true, down = true)
        gesture.move(200f, 0f)
        gesture.cancel()
        assertNull(release(200f, 0f))
    }

    @Test fun cancelledDismissNeverHidesTheKeyboard() {
        gesture.begin(0f, 0f, down = true)
        gesture.move(0f, 200f)
        gesture.cancel()
        assertNull(release(0f, 200f))
    }

    @Test fun cancelledRailNeverChangesPage() {
        gesture.begin(0f, 100f, up = true, down = true)
        gesture.move(0f, 0f)
        gesture.cancel()
        assertNull(release(0f, 0f))
    }

    @Test fun nextDownStartsCleanAfterCancellation() {
        gesture.begin(0f, 0f, horizontal = true)
        gesture.move(200f, 0f)
        gesture.cancel()
        gesture.begin(50f, 50f, down = true)
        gesture.move(50f, 80f)
        assertEquals(ImeGestureSession.Direction.DOWN, release(50f, 200f))
    }

    @Test fun horizontalOwnerCannotTurnIntoADismiss() {
        gesture.begin(0f, 0f, horizontal = true, down = true)
        gesture.move(20f, 0f)
        assertEquals(ImeGestureSession.Axis.HORIZONTAL, gesture.move(20f, 200f))
        assertNull(release(20f, 200f))
    }

    @Test fun verticalOwnerCannotTurnIntoAModeSwitch() {
        gesture.begin(0f, 0f, horizontal = true, down = true)
        gesture.move(0f, 20f)
        assertEquals(ImeGestureSession.Axis.VERTICAL, gesture.move(200f, 20f))
        assertNull(release(200f, 20f))
    }

    @Test fun diagonalMotionDoesNotStealAChildGesture() {
        gesture.begin(0f, 0f, horizontal = true, down = true)
        assertEquals(ImeGestureSession.Axis.NONE, gesture.move(50f, 50f))
        assertNull(release(150f, 150f))
    }

    @Test fun disabledAxesStayWithTheChild() {
        gesture.begin(0f, 0f)
        assertEquals(ImeGestureSession.Axis.NONE, gesture.move(200f, 0f))
        assertEquals(ImeGestureSession.Axis.NONE, gesture.move(0f, 200f))
        assertNull(release(0f, 200f))
    }

    @Test fun nestedScrollCanRejectDismiss() {
        gesture.begin(0f, 0f, down = true)
        assertEquals(ImeGestureSession.Axis.NONE, gesture.move(0f, 30f, allowDown = false))
        assertNull(release(0f, 200f))
    }

    @Test fun upwardMotionCannotDismiss() {
        gesture.begin(0f, 200f, down = true)
        assertEquals(ImeGestureSession.Axis.NONE, gesture.move(0f, 0f))
        assertNull(release(0f, 0f))
    }

    @Test fun railCanPageInBothDirections() {
        gesture.begin(0f, 100f, up = true, down = true)
        gesture.move(0f, 80f)
        assertEquals(ImeGestureSession.Direction.UP, release(0f, 0f))
        gesture.begin(0f, 0f, up = true, down = true)
        gesture.move(0f, 20f)
        assertEquals(ImeGestureSession.Direction.DOWN, release(0f, 100f))
    }

    @Test fun nonFiniteCoordinatesInvalidateTheSequence() {
        gesture.begin(0f, 0f, horizontal = true)
        gesture.move(20f, 0f)
        gesture.move(Float.NaN, 0f)
        assertNull(release(200f, 0f))
        gesture.begin(Float.POSITIVE_INFINITY, 0f, horizontal = true)
        assertNull(release(200f, 0f))
    }

    @Test fun pressWithoutClaimCannotCommitFromTheReleaseLocation() {
        gesture.begin(0f, 0f, horizontal = true)
        assertNull(release(200f, 0f))
    }

    private fun release(x: Float, y: Float) = gesture.release(
        x, y, horizontalThreshold = 100f, verticalThreshold = 60f,
    )
}
