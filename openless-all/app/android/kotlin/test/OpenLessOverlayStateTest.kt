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

    @Test
    fun neverTriggerBlocksOverlayDisplayButOtherModesRemainAvailable() {
        assertFalse(OpenLessAndroidPreferences.shouldDisplayOverlay("never"))
        assertTrue(OpenLessAndroidPreferences.shouldDisplayOverlay("background"))
        assertTrue(OpenLessAndroidPreferences.shouldDisplayOverlay("always"))
        assertTrue(OpenLessAndroidPreferences.shouldDisplayOverlay(null))
    }

    @Test
    fun newlyAddedOverlayRootWithParentCountsAsLiveBeforeFirstTraversal() {
        assertTrue(isOverlayRootLive(isAttachedToWindow = false, hasParent = true))
        assertTrue(isOverlayRootLive(isAttachedToWindow = true, hasParent = true))
        assertTrue(isOverlayRootLive(isAttachedToWindow = true, hasParent = false))
        assertFalse(isOverlayRootLive(isAttachedToWindow = false, hasParent = false))
    }

    @Test
    fun destroyedOrSupersededOverlayServiceIgnoresInteractions() {
        assertTrue(shouldHandleOverlayServiceInteraction(destroyed = false, isCurrentInstance = true))
        assertFalse(shouldHandleOverlayServiceInteraction(destroyed = true, isCurrentInstance = true))
        assertFalse(shouldHandleOverlayServiceInteraction(destroyed = false, isCurrentInstance = false))
        assertFalse(shouldHandleOverlayServiceInteraction(destroyed = true, isCurrentInstance = false))
    }
}
