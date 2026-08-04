package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Test

class OpenLessAccessibilityResultTest {
    @Test
    fun accessibilityPasteResultRoundTripsCodes() {
        AccessibilityPasteResult.entries.forEach { expected ->
            assertEquals(expected, AccessibilityPasteResult.fromCode(expected.code))
        }
        assertEquals(
            AccessibilityPasteResult.IPC_PROTOCOL_ERROR,
            AccessibilityPasteResult.fromCode(999),
        )
    }

    @Test
    fun ipcProtocolErrorIsNotRetriablePasteFailure() {
        assertEquals("IPC_PROTOCOL_ERROR", AccessibilityPasteResult.IPC_PROTOCOL_ERROR.reason)
        assertEquals("SERVICE_NOT_CONNECTED", AccessibilityPasteResult.SERVICE_NOT_CONNECTED.reason)
    }
}
