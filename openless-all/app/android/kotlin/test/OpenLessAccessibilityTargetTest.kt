package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLessAccessibilityTargetTest {
    @Test
    fun passesEditableFocusChecksRequiresEditableFocusedAndMatchingPackage() {
        assertTrue(
            OpenLessAccessibilityTarget.passesEditableFocusChecks(
                isEditable = true,
                isFocused = true,
                nodePackage = "com.example.app",
                activePackage = "com.example.app",
            ),
        )
        assertFalse(
            OpenLessAccessibilityTarget.passesEditableFocusChecks(
                isEditable = false,
                isFocused = true,
                nodePackage = "com.example.app",
                activePackage = "com.example.app",
            ),
        )
        assertFalse(
            OpenLessAccessibilityTarget.passesEditableFocusChecks(
                isEditable = true,
                isFocused = false,
                nodePackage = "com.example.app",
                activePackage = "com.example.app",
            ),
        )
        assertFalse(
            OpenLessAccessibilityTarget.passesEditableFocusChecks(
                isEditable = true,
                isFocused = true,
                nodePackage = "com.other.app",
                activePackage = "com.example.app",
            ),
        )
    }

    @Test
    fun passesWindowChecksRequiresMatchingActiveWindow() {
        assertTrue(OpenLessAccessibilityTarget.passesWindowChecks(3, 3))
        assertFalse(OpenLessAccessibilityTarget.passesWindowChecks(3, 4))
        assertFalse(OpenLessAccessibilityTarget.passesWindowChecks(-1, 3))
        assertFalse(OpenLessAccessibilityTarget.passesWindowChecks(3, -1))
    }

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

    @Test
    fun isPasteTargetAcceptsEditTextClassAndPasteAction() {
        assertTrue(
            OpenLessAccessibilityTarget.isPasteTarget(
                isEditable = false,
                isPassword = false,
                className = "android.widget.EditText",
                actions = emptyList(),
            ),
        )
        assertTrue(
            OpenLessAccessibilityTarget.isPasteTarget(
                isEditable = false,
                isPassword = false,
                className = "android.view.View",
                actionIds = listOf(0x00008000),
            ),
        )
        assertFalse(
            OpenLessAccessibilityTarget.isPasteTarget(
                isEditable = false,
                isPassword = true,
                className = "android.widget.EditText",
                actions = emptyList(),
            ),
        )
    }
}
