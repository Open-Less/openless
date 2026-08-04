package com.openless.app

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLessPasteVerificationTest {
    @Test
    fun acceptsWhenClipboardTextIsContained() {
        assertTrue(
            OpenLessPasteVerification.pasteAppearsApplied(
                beforeText = "hello ",
                afterText = "hello world",
                clipboardText = "world",
            ),
        )
    }

    @Test
    fun acceptsWhenTextAppendedAtEnd() {
        assertTrue(
            OpenLessPasteVerification.pasteAppearsApplied(
                beforeText = "",
                afterText = "dictation",
                clipboardText = "dictation",
            ),
        )
    }

    @Test
    fun rejectsWhenActionSucceededButTextUnchanged() {
        assertFalse(
            OpenLessPasteVerification.pasteAppearsApplied(
                beforeText = "still empty",
                afterText = "still empty",
                clipboardText = "new words",
            ),
        )
    }
}
