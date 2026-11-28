package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLessOverlayStateTest {
    @Test
    fun queuedMeterFramesCannotReleaseProcessingButExplicitRecoveryCan() {
        for (message in listOf(null, "", "  ")) {
            assertTrue(shouldIgnoreOverlayRecordingFrame(true, message))
        }
        assertFalse(shouldIgnoreOverlayRecordingFrame(true, "voice edit capture failed"))
        assertFalse(shouldIgnoreOverlayRecordingFrame(false, null))
    }

    @Test
    fun foreignRecordingFramesMirrorWithoutPromoteOrAutoCancel() {
        assertEquals(
            OverlayRecordingFrameAction.MIRROR_ONLY,
            overlayRecordingFrameAction(ownsSession = false, processing = false, message = null),
        )
        assertEquals(
            OverlayRecordingFrameAction.MIRROR_ONLY,
            overlayRecordingFrameAction(ownsSession = false, processing = false, message = "voice edit capture failed"),
        )
        assertFalse(shouldAutoCancelOwnedSession(ownsSession = false))
    }

    @Test
    fun queuedMeterFramesStillWinOverOwnership() {
        for (message in listOf(null, "", "  ")) {
            assertEquals(
                OverlayRecordingFrameAction.IGNORE,
                overlayRecordingFrameAction(ownsSession = true, processing = true, message = message),
            )
            assertEquals(
                OverlayRecordingFrameAction.IGNORE,
                overlayRecordingFrameAction(ownsSession = false, processing = true, message = message),
            )
        }
    }

    @Test
    fun ownedRecordingKeepsPromoteAndOwnedAutoCancel() {
        assertEquals(
            OverlayRecordingFrameAction.PROMOTE_AND_SHOW,
            overlayRecordingFrameAction(ownsSession = true, processing = false, message = null),
        )
        assertTrue(shouldAutoCancelOwnedSession(ownsSession = true))
    }

    @Test
    fun ownershipIsReleasedOnTerminalFailureAndManualEnds() {
        val ownership = OverlaySessionOwnership()
        assertFalse(ownership.owned)
        ownership.markStarted()
        assertTrue(ownership.owned)
        // 终态/失败/手动手势都走同一个 release，后续外部帧只会镜像。
        ownership.release()
        assertFalse(ownership.owned)
    }

    @Test
    fun neverTriggerBlocksOverlayDisplayButOtherModesRemainAvailable() {
        assertFalse(OpenLessAndroidPreferences.shouldDisplayOverlay("never"))
        assertTrue(OpenLessAndroidPreferences.shouldDisplayOverlay("background"))
        assertTrue(OpenLessAndroidPreferences.shouldDisplayOverlay("always"))
        assertTrue(OpenLessAndroidPreferences.shouldDisplayOverlay(null))
    }
}
