package com.openless.app

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ImeVoiceEligibilityTest {
    private fun state(recording: Boolean = false, processing: Boolean = false) = ImeVoiceUiProjection.fromLegacy(
        "idle", recording, processing, false, false, false, false, "ready",
    )
    @Test fun foreignRecordingNeverAdvertisesOwnedCancel() {
        val ui = state(recording = true).withEligibility(false, true, false, true)
        assertFalse(ui.canCancel)
        assertFalse(ui.canStart)
        assertTrue(ui.editorLocked)
    }
    @Test fun permissionAndSensitiveFieldsCannotStartCapture() {
        assertFalse(state().withEligibility(false, false, false, false).canStart)
        assertFalse(state().withEligibility(false, true, true, false).canStart)
        assertTrue(state().withEligibility(false, true, false, false).canStart)
    }
    @Test fun ownedCaptureCanBeCancelledEvenIfPermissionWasRevoked() {
        assertTrue(state(recording = true).withEligibility(true, false, false, false).canCancel)
    }
    @Test fun eligibilityCannotReopenProcessingOrCreateCancelForIdle() {
        assertFalse(state(processing = true).withEligibility(true, true, false, false).canStart)
        assertFalse(state().withEligibility(true, true, false, false).canCancel)
    }
}
