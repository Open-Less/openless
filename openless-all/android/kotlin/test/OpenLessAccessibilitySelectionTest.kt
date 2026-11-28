package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLessAccessibilitySelectionTest {
    private val captured = OpenLessAccessibilitySelection(
        packageName = "com.example.editor",
        windowId = 7,
        selectionStart = 6,
        selectionEnd = 11,
        sourceText = "hello world",
    )

    @Test
    fun replacesOnlyTheCapturedRange() {
        assertEquals("hello voice", captured.replaceIn("hello world", "voice"))
        assertFalse(captured.replaceIn("hello other", "voice") != null)
    }

    @Test
    fun matchingRequiresSameTargetTextWindowAndRange() {
        assertTrue(captured.matches(true, "com.example.editor", 7, "hello world", 11, 6))
        assertFalse(captured.matches(true, "com.other.editor", 7, "hello world", 6, 11))
        assertFalse(captured.matches(true, "com.example.editor", 8, "hello world", 6, 11))
        assertFalse(captured.matches(true, "com.example.editor", 7, "hello other", 6, 11))
        assertFalse(captured.matches(true, "com.example.editor", 7, "hello world", 5, 11))
    }

    @Test
    fun identicalFieldsInTheSameWindowRequireTheCapturedNode() {
        val emptyField = captured.copy(sourceText = "", selectionStart = 0, selectionEnd = 0)
        assertTrue(emptyField.matches(true, "com.example.editor", 7, "", 0, 0))
        assertFalse(emptyField.matches(false, "com.example.editor", 7, "", 0, 0))
    }

    @Test
    fun emptyAndOutOfBoundsSelectionsAreNotUsable() {
        assertFalse(captured.copy(selectionStart = 4, selectionEnd = 4).isUsable())
        assertTrue(captured.copy(selectionStart = 4, selectionEnd = 4).isFieldUsable())
        assertFalse(captured.copy(selectionStart = 0, selectionEnd = 99).isUsable())
        assertNull(captured.copy(sourceText = "short").replaceIn("short", "voice"))
    }

    @Test
    fun caretTargetReplacesTheWholeFieldAfterRevalidation() {
        val field = captured.copy(selectionStart = 4, selectionEnd = 4)
        assertEquals("replacement", field.replaceTarget("hello world", "replacement"))
        assertNull(field.replaceTarget("changed", "replacement"))
    }
}
