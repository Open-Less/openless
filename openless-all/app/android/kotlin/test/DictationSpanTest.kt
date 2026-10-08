package com.openless.app

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DictationSpanTest {
    @Test
    fun matchesOnlyTheExactTextBeforeTheCursor() {
        assertTrue(DictationSpan.matchesBeforeCursor("hello world", "hello world"))
        assertFalse(DictationSpan.matchesBeforeCursor("hello", "hello world"))
        assertFalse(DictationSpan.matchesBeforeCursor("Please review: hello", "hello world"))
        assertFalse(DictationSpan.matchesBeforeCursor("hello world!", "hello world"))
        assertFalse(DictationSpan.matchesBeforeCursor(null, "hello"))
        assertFalse(DictationSpan.matchesBeforeCursor("", "hello"))
        assertFalse(DictationSpan.matchesBeforeCursor("hello", ""))
    }
}
