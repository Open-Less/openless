package com.openless.app

import com.openless.app.ImeDraftReplacement.DeleteAroundCaret
import com.openless.app.ImeDraftReplacement.ReplaceSelection
import com.openless.app.ImeDraftReplacement.SelectRange
import org.junit.Assert.*
import org.junit.Test

class ImeDraftReplacementTest {
    private fun plan(
        original: String,
        before: String,
        after: String = "",
        selected: String = "",
        caretOffset: Int? = before.length,
        hintStart: Int? = null,
    ) = ImeDraftReplacement.plan(original, before, selected, after, caretOffset, hintStart)

    @Test fun dictationRightBeforeTheCaretIsSelected() {
        assertEquals(SelectRange(3, 8), plan("今天天气好", before = "你好，今天天气好"))
    }

    /** The reported bug: the host caret had moved away from the end of the dictated text. */
    @Test fun dictationIsStillFoundWhenTheCaretMovedAway() {
        // Caret at the very start of the field.
        assertEquals(SelectRange(3, 8), plan("今天天气好", before = "", after = "你好，今天天气好。"))
        // Caret in the middle of the dictated text.
        assertEquals(SelectRange(3, 8), plan("今天天气好", before = "你好，今天", after = "天气好。"))
        // Caret after text typed behind it.
        assertEquals(SelectRange(3, 8), plan("今天天气好", before = "你好，今天天气好。后面又打了字"))
    }

    @Test fun selectionEqualToTheOriginalIsReplacedDirectly() {
        assertEquals(ReplaceSelection, plan("天气", before = "今天", selected = "天气", after = "好"))
        // Also without offsets: committing over a selection needs none.
        assertEquals(ReplaceSelection, plan("天气", before = "今天", selected = "天气", after = "好", caretOffset = null))
    }

    @Test fun lostSelectionFallsBackToTheRememberedOffset() {
        // "好" occurs at 1, 4 and 7; it was selected at 4 and the host has since dropped the selection.
        val field = "你好今天好明天好"
        assertEquals(SelectRange(4, 5), plan("好", before = field, hintStart = 4))
        assertEquals(SelectRange(1, 2), plan("好", before = field, hintStart = 1))
        // The remembered offset also wins over a nearer occurrence when the caret sits elsewhere.
        assertEquals(SelectRange(1, 2), plan("好", before = field.take(7), after = field.drop(7), hintStart = 1))
    }

    @Test fun rememberedOffsetToleratesASmallShift() {
        // "好的" was selected at 7; two chars typed at the start of the field
        // while the draft was open moved it to 9. Still the nearer of the two.
        assertEquals(SelectRange(9, 11), plan("好的", before = "嗯，好的，明天见。好的，后天见。", hintStart = 7))
    }

    @Test fun withoutARememberedOffsetTheOccurrenceNearestTheCaretWins() {
        val field = "你好今天好明天好"
        assertEquals(SelectRange(1, 2), plan("好", before = field.take(1), after = field.drop(1)))
        assertEquals(SelectRange(4, 5), plan("好", before = field.take(4), after = field.drop(4)))
        assertEquals(SelectRange(7, 8), plan("好", before = field))
    }

    @Test fun onATieTheLaterOccurrenceWins() {
        // Caret between two adjacent occurrences touches both.
        assertEquals(1, ImeDraftReplacement.locate("好好", "好", caret = 1, wanted = null))
        // One char away on either side of the caret.
        assertEquals(3, ImeDraftReplacement.locate("好一二好", "好", caret = 2, wanted = null))
    }

    @Test fun offsetsAreAbsoluteEvenWhenOnlyAWindowOfTheFieldWasRead() {
        // The caret is at 1000 in the host; only the 6 chars before it were read.
        assertEquals(SelectRange(996, 999), plan("天气好", before = "今天天气好。", caretOffset = 1000))
        assertEquals(SelectRange(996, 999), plan("天气好", before = "今天天气好。", caretOffset = 1000, hintStart = 996))
    }

    @Test fun selectedTextCountsAsPartOfTheField() {
        // The original spans the host's current (different) selection.
        assertEquals(SelectRange(1, 5), plan("天天气好", before = "今天", selected = "天气", after = "好", caretOffset = 2))
    }

    @Test fun withoutOffsetsTextTouchingTheCaretIsDeletedAroundIt() {
        assertEquals(DeleteAroundCaret(5, 0), plan("今天天气好", before = "你好，今天天气好", caretOffset = null))
        assertEquals(DeleteAroundCaret(2, 3), plan("今天天气好", before = "你好，今天", after = "天气好。", caretOffset = null))
        assertEquals(DeleteAroundCaret(0, 5), plan("今天天气好", before = "你好，", after = "今天天气好。", caretOffset = null))
    }

    @Test fun withoutOffsetsTextAwayFromTheCaretIsLeftAlone() {
        assertNull(plan("今天天气好", before = "", after = "你好，今天天气好。", caretOffset = null))
        // A selection that isn't the original can't be worked around without offsets either.
        assertNull(plan("今天", before = "今天", selected = "天气", after = "好", caretOffset = null))
    }

    @Test fun missingOriginalNeverProducesAPlan() {
        assertNull(plan("今天天气好", before = "你好，今天天气不好"))
        assertNull(plan("今天天气好", before = "", after = ""))
        assertNull(plan("今天天气好", before = "你好，今天天气不好", caretOffset = null))
        assertNull(plan("", before = "你好"))
        assertEquals(-1, ImeDraftReplacement.locate("你好", "", caret = 0, wanted = null))
    }
}
