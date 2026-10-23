package com.openless.app

import android.text.InputType
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class EnglishAutoTextTest {
    @Test fun sentenceStartsAtTheFieldLineOrAfterAFinishedSentence() {
        assertTrue(EnglishAutoText.isSentenceStart(""))
        assertTrue(EnglishAutoText.isSentenceStart("   "))
        assertTrue(EnglishAutoText.isSentenceStart("Hello there\n"))
        assertTrue(EnglishAutoText.isSentenceStart("Done. "))
        assertTrue(EnglishAutoText.isSentenceStart("Really?  "))
        assertTrue(EnglishAutoText.isSentenceStart("Wow! "))
    }

    @Test fun midSentenceIsNotASentenceStart() {
        assertFalse(EnglishAutoText.isSentenceStart("Hello"))
        assertFalse(EnglishAutoText.isSentenceStart("Hello "))
        assertFalse(EnglishAutoText.isSentenceStart("one, "))
        // The mark must be followed by whitespace: "3." or "e.g." mid-word isn't an end yet.
        assertFalse(EnglishAutoText.isSentenceStart("Version 3."))
    }

    @Test fun capitalsOnlyInOrdinaryTextFields() {
        val text = InputType.TYPE_CLASS_TEXT
        assertTrue(EnglishAutoText.allowsAutoCapitals(text))
        assertTrue(EnglishAutoText.allowsAutoCapitals(text or InputType.TYPE_TEXT_FLAG_MULTI_LINE))
        assertTrue(EnglishAutoText.allowsAutoCapitals(text or InputType.TYPE_TEXT_VARIATION_SHORT_MESSAGE))
        assertFalse(EnglishAutoText.allowsAutoCapitals(text or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS))
        assertFalse(EnglishAutoText.allowsAutoCapitals(text or InputType.TYPE_TEXT_VARIATION_URI))
        assertFalse(EnglishAutoText.allowsAutoCapitals(text or InputType.TYPE_TEXT_VARIATION_PASSWORD))
        assertFalse(EnglishAutoText.allowsAutoCapitals(text or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD))
        assertFalse(EnglishAutoText.allowsAutoCapitals(InputType.TYPE_CLASS_NUMBER))
    }

    @Test fun doubleSpaceFullStopOnlyRightAfterAWord() {
        assertTrue(EnglishAutoText.doubleSpaceMakesFullStop("hello "))
        assertTrue(EnglishAutoText.doubleSpaceMakesFullStop("at 5 "))
        assertFalse(EnglishAutoText.doubleSpaceMakesFullStop(" "))
        assertFalse(EnglishAutoText.doubleSpaceMakesFullStop("hello"))
        assertFalse(EnglishAutoText.doubleSpaceMakesFullStop("hello. "))
        assertFalse(EnglishAutoText.doubleSpaceMakesFullStop("hello  "))
        assertFalse(EnglishAutoText.doubleSpaceMakesFullStop("hello\n "))
    }

    @Test fun onlySentencePunctuationSwallowsTheAutoSpace() {
        listOf(".", ",", "!", "?", ";", ":").forEach { assertTrue(it, EnglishAutoText.swallowsAutoSpace(it)) }
        listOf("(", "'", "-", "@", "a", "1").forEach { assertFalse(it, EnglishAutoText.swallowsAutoSpace(it)) }
    }
}
