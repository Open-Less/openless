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

    @Test
    fun detectsOnlyPrefixesThatStillEndAtTheCursor() {
        assertTrue(DictationSpan.hasRemainingPrefix("prefix: hello", "hello world"))
        assertTrue(DictationSpan.hasRemainingPrefix("hello world", "hello world"))
        assertTrue(DictationSpan.hasRemainingPrefix("prefix: ababa", "ababac"))
        assertTrue(DictationSpan.hasRemainingPrefix("你好😀", "你好😀世界"))
        assertFalse(DictationSpan.hasRemainingPrefix("hello!", "hello world"))
        assertFalse(DictationSpan.hasRemainingPrefix("unrelated", "hello world"))
        assertFalse(DictationSpan.hasRemainingPrefix("", "hello"))
        assertFalse(DictationSpan.hasRemainingPrefix("hello", ""))
    }

    @Test
    fun matchesThePrefixContractForOverlappingInputs() {
        val random = java.util.Random(1131)
        repeat(1000) {
            val expected = (0 until random.nextInt(30)).map { "ab😀"[random.nextInt(4)] }.joinToString("")
            val before = (0 until random.nextInt(30)).map { "ab😀"[random.nextInt(4)] }.joinToString("")
            val survives = (1..expected.length).any { before.endsWith(expected.substring(0, it)) }
            org.junit.Assert.assertEquals("before=$before expected=$expected", survives, DictationSpan.hasRemainingPrefix(before, expected))
        }
    }

    @Test
    fun scansLongEditorTextOnceWithoutTakingSubsequences() {
        val text = "x".repeat(9999) + "z"
        var reads = 0
        val before = object : CharSequence {
            override val length = text.length
            override fun get(index: Int): Char { reads++; return text[index] }
            override fun subSequence(startIndex: Int, endIndex: Int): CharSequence = error("must not copy substrings")
        }
        assertFalse(DictationSpan.hasRemainingPrefix(before, "x".repeat(9999) + "y"))
        org.junit.Assert.assertEquals(text.length, reads)
    }
}
