package com.openless.app

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
}
