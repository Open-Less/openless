package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Test

class ImeVoiceFramePolicyTest {
    @Test fun bufferedFinalizationCannotSendQueuedStopOrModeAgain() {
        for (state in listOf("transcribing", "polishing", "done", "cancelled", "error")) {
            assertEquals(false, ImeVoiceFramePolicy.canApplyQueuedStartActions(state))
        }
        assertEquals(true, ImeVoiceFramePolicy.canApplyQueuedStartActions(null))
        assertEquals(true, ImeVoiceFramePolicy.canApplyQueuedStartActions("recording"))
    }
    @Test fun foreignFramesNeverAcquireAnImeRequest() {
        for (state in listOf("recording", "transcribing", "polishing", "done", "cancelled", "error", "idle")) {
            assertEquals(ImeVoiceFrameAction.EXTERNAL, ImeVoiceFramePolicy.resolve(false, false, state))
            assertEquals(ImeVoiceFrameAction.EXTERNAL, ImeVoiceFramePolicy.resolve(false, true, state))
        }
    }
    @Test fun lateRecordingCannotReopenCaptureAfterRelease() {
        assertEquals(ImeVoiceFrameAction.IGNORE, ImeVoiceFramePolicy.resolve(true, true, "recording"))
    }
    @Test fun ownedRecordingAndProcessingRemainObservable() {
        assertEquals(ImeVoiceFrameAction.RECORDING, ImeVoiceFramePolicy.resolve(true, false, "recording"))
        for (state in listOf("transcribing", "polishing")) {
            assertEquals(ImeVoiceFrameAction.PROCESSING, ImeVoiceFramePolicy.resolve(true, false, state))
        }
    }
    @Test fun globalDoneWaitsForScopedResultDelivery() {
        assertEquals(ImeVoiceFrameAction.WAIT_RESULT, ImeVoiceFramePolicy.resolve(true, true, "done"))
        assertEquals(ImeVoiceFrameAction.EXTERNALLY_COMPLETED, ImeVoiceFramePolicy.resolve(true, false, "done"))
        assertEquals(ImeVoiceFrameAction.EXTERNALLY_COMPLETED, ImeVoiceFramePolicy.resolve(true, true, "done", stopSubmitted = false))
    }
    @Test fun globalIdleCannotDropAnOwnedResult() {
        assertEquals(ImeVoiceFrameAction.IGNORE, ImeVoiceFramePolicy.resolve(true, true, "idle"))
    }
    @Test fun ownedTerminalFailuresRemainVisible() {
        assertEquals(ImeVoiceFrameAction.CANCELLED, ImeVoiceFramePolicy.resolve(true, false, "cancelled"))
        assertEquals(ImeVoiceFrameAction.ERROR, ImeVoiceFramePolicy.resolve(true, false, "error"))
    }
}
