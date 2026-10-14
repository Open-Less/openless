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
            )
        )
        assertFalse(
            OpenLessAccessibilityTarget.passesEditableFocusChecks(
                isEditable = false,
                isFocused = true,
                nodePackage = "com.example.app",
                activePackage = "com.example.app",
            )
        )
        assertFalse(
            OpenLessAccessibilityTarget.passesEditableFocusChecks(
                isEditable = true,
                isFocused = false,
                nodePackage = "com.example.app",
                activePackage = "com.example.app",
            )
        )
        assertFalse(
            OpenLessAccessibilityTarget.passesEditableFocusChecks(
                isEditable = true,
                isFocused = true,
                nodePackage = "com.other.app",
                activePackage = "com.example.app",
            )
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
    fun selectionRangeValidationRejectsEmptyAndOutOfBoundsRanges() {
        assertTrue(OpenLessAccessibilityTarget.isSelectionRangeValid("hello", 1, 4))
        assertTrue(OpenLessAccessibilityTarget.isSelectionRangeValid("hello", 4, 1))
        assertFalse(OpenLessAccessibilityTarget.isSelectionRangeValid("hello", 2, 2))
        assertFalse(OpenLessAccessibilityTarget.isSelectionRangeValid("hello", -1, 2))
        assertFalse(OpenLessAccessibilityTarget.isSelectionRangeValid("hello", 1, 8))
        assertFalse(OpenLessAccessibilityTarget.isSelectionRangeValid(null, 1, 2))
    }

    @Test
    fun fieldTargetValidationAllowsCaretButStillChecksBounds() {
        assertTrue(OpenLessAccessibilityTarget.isFieldTargetValid("hello", 2, 2))
        assertTrue(OpenLessAccessibilityTarget.isFieldTargetValid("hello", 4, 1))
        assertTrue(OpenLessAccessibilityTarget.isFieldTargetValid("", 0, 0))
        assertFalse(OpenLessAccessibilityTarget.isFieldTargetValid("hello", -1, 1))
        assertFalse(OpenLessAccessibilityTarget.isFieldTargetValid("hello", 1, 6))
        assertFalse(OpenLessAccessibilityTarget.isFieldTargetValid(null, 0, 0))
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
            )
        )
        assertTrue(
            OpenLessAccessibilityTarget.isPasteTarget(
                isEditable = false,
                isPassword = false,
                className = "android.view.View",
                actionIds = listOf(0x00008000),
            )
        )
        assertFalse(
            OpenLessAccessibilityTarget.isPasteTarget(
                isEditable = false,
                isPassword = true,
                className = "android.widget.EditText",
                actions = emptyList(),
            )
        )
    }
}
