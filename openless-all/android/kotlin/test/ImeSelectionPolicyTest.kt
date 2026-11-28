package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * [ImeSelectionPolicy] 的纯 JVM 单测（NoNative/NoIO）。
 *
 * 只验证 UTF-16/codepoint 语义：不宣称完整 grapheme cluster（ZWJ 序列按 codepoint 单步）。
 */
class ImeSelectionPolicyTest {

    private fun extend(
        text: CharSequence?,
        startOffset: Int = 0,
        selectionStart: Int,
        selectionEnd: Int,
        previous: ImeSelectionRange? = null,
        direction: Int,
    ): ImeSelectionRange? = ImeSelectionPolicy.extend(
        text, startOffset, selectionStart, selectionEnd, previous, direction,
    )

    private fun assertRange(anchor: Int, active: Int, actual: ImeSelectionRange?) {
        assertTrue("expected ($anchor,$active) but was $actual", actual != null)
        assertEquals(anchor, actual!!.anchor)
        assertEquals(active, actual.active)
    }

    private fun isInsideSurrogatePair(text: CharSequence, index: Int): Boolean =
        index > 0 && index < text.length &&
            Character.isHighSurrogate(text[index - 1]) &&
            Character.isLowSurrogate(text[index])

    @Test
    fun asciiSelectionExtendsAndShrinksByOne() {
        assertEquals(ImeSelectionRange(1, 4), extend("hello", selectionStart = 1, selectionEnd = 3, direction = 1))
        assertEquals(ImeSelectionRange(1, 2), extend("hello", selectionStart = 1, selectionEnd = 3, direction = -1))
    }

    @Test
    fun partialStartOffsetProducesAbsoluteOffsets() {
        val text = "abcdef"
        assertRange(12, 15, extend(text, startOffset = 10, selectionStart = 2, selectionEnd = 4, direction = 1))
        assertRange(12, 13, extend(text, startOffset = 10, selectionStart = 2, selectionEnd = 4, direction = -1))
    }

    @Test
    fun unknownOrInvalidInputsReturnNullWithoutGuessingZero() {
        assertNull("null text", extend(null, selectionStart = 0, selectionEnd = 1, direction = 1))
        assertNull("negative offset", extend("abc", startOffset = -1, selectionStart = 0, selectionEnd = 1, direction = 1))
        assertNull("negative start", extend("abc", selectionStart = -1, selectionEnd = 1, direction = 1))
        assertNull("negative end", extend("abc", selectionStart = 0, selectionEnd = -1, direction = 1))
        assertNull("end past text", extend("abc", selectionStart = 0, selectionEnd = 4, direction = 1))
        assertNull("start past text", extend("abc", selectionStart = 5, selectionEnd = 5, direction = 1))
        assertNull(
            "Int overflow",
            extend("abc", startOffset = Int.MAX_VALUE, selectionStart = 0, selectionEnd = 1, direction = 1),
        )
        assertNull(
            "Int overflow 2",
            extend("abc", startOffset = Int.MAX_VALUE - 1, selectionStart = 2, selectionEnd = 2, direction = -1),
        )
    }

    @Test
    fun invalidDirectionAlwaysReturnsNull() {
        listOf(0, 2, -2, Int.MIN_VALUE, Int.MAX_VALUE).forEach { direction ->
            assertNull(
                "direction=$direction",
                extend("abc", selectionStart = 1, selectionEnd = 1, direction = direction),
            )
        }
    }

    @Test
    fun existingRangeIsReusedWhenLiveSelectionMatches() {
        val text = "abcdef"
        // previous 的 min/max 与当前绝对 selection (12,15) 完全一致 → 保留 anchor，移动 active。
        val grew = extend(
            text, startOffset = 10, selectionStart = 2, selectionEnd = 5,
            previous = ImeSelectionRange(12, 15), direction = 1,
        )
        assertRange(12, 16, grew)
        val shrank = extend(
            text, startOffset = 10, selectionStart = 2, selectionEnd = 5,
            previous = ImeSelectionRange(12, 15), direction = -1,
        )
        assertRange(12, 14, shrank)
    }

    @Test
    fun reversedPreviousKeepsItsAnchorDirection() {
        val text = "abcdef"
        // 反向选择：anchor > active，但 min/max 仍与 live selection 一致 → 复用。
        val result = extend(
            text, startOffset = 10, selectionStart = 2, selectionEnd = 5,
            previous = ImeSelectionRange(15, 12), direction = -1,
        )
        assertRange(15, 11, result)
    }

    @Test
    fun externalCaretMovementRebasesOnLiveSelection() {
        val text = "abcdefghij"
        val result = extend(
            text, startOffset = 10, selectionStart = 3, selectionEnd = 3,
            previous = ImeSelectionRange(12, 15), direction = 1,
        )
        // 外部把 caret 挪到 13：不得沿用旧 anchor 12。
        assertRange(13, 14, result)
    }

    @Test
    fun initialRangeFollowsSelectionOrder() {
        // 初始 range 的 anchor/active 直接来自 selectionStart/End（保持调用方顺序）。
        assertRange(3, 2, extend("abcd", selectionStart = 3, selectionEnd = 1, direction = 1))
        assertRange(3, 0, extend("abcd", selectionStart = 3, selectionEnd = 1, direction = -1))
    }

    @Test
    fun emojiIsNeverSplitAcrossSurrogatePairs() {
        val text = "a\uD83D\uDE00b" // a😀b, len=4
        // 从 'a' 之后向右：必须跨过一个完整 codepoint（1 → 3），不能停在 2。
        assertRange(1, 3, extend(text, selectionStart = 1, selectionEnd = 1, direction = 1))
        // 从 'b' 之前向左：必须回到 1，不能停在 2。
        assertRange(3, 1, extend(text, selectionStart = 3, selectionEnd = 3, direction = -1))
        // 落在 surrogate pair 中间一律 null。
        assertNull(extend(text, selectionStart = 2, selectionEnd = 2, direction = 1))
        assertNull(extend(text, selectionStart = 1, selectionEnd = 2, direction = 1))
    }

    @Test
    fun boundaryExtremesKeepOriginalRange() {
        assertEquals(ImeSelectionRange(0, 0), extend("ab", selectionStart = 0, selectionEnd = 0, direction = -1))
        assertEquals(ImeSelectionRange(2, 2), extend("ab", selectionStart = 2, selectionEnd = 2, direction = 1))
        assertEquals(
            ImeSelectionRange(1, 1),
            extend("a", selectionStart = 1, selectionEnd = 1, direction = 1),
        )
    }

    @Test
    fun chainedEmojiStepsNeverLandInsidePairAndStayMonotonic() {
        val text = "a\uD83D\uDE00b\uD83D\uDE01c" // a😀b😃c, len=7
        var selection = extend(text, selectionStart = 0, selectionEnd = 0, direction = 1)!!
        assertEquals(ImeSelectionRange(0, 1), selection)
        var previous: ImeSelectionRange? = selection
        var lastActive = selection.active
        repeat(6) {
            val active = selection.active
            val next = extend(
                text, selectionStart = active, selectionEnd = active,
                previous = previous, direction = 1,
            )
            if (next == null) return@repeat
            assertFalse(
                "active=${next.active} 落在 surrogate pair 中间",
                isInsideSurrogatePair(text, next.active),
            )
            assertFalse(
                "anchor=${next.anchor} 落在 surrogate pair 中间",
                isInsideSurrogatePair(text, next.anchor),
            )
            assertTrue("active 应单调前进", next.active >= lastActive)
            lastActive = next.active
            previous = next
            selection = next
        }
        assertEquals("应停在文本末尾", text.length, selection.active)
    }

    @Test
    fun zwjSequencesAdvanceAsCodepointsNotWholeGraphemes() {
        // 👨‍👩 = man(0,1) + ZWJ(2) + woman(3,4)，len=5。
        val text = "\uD83D\uDC68\u200D\uD83D\uDC69"
        val first = extend(text, selectionStart = 0, selectionEnd = 0, direction = 1)
        assertRange(0, 2, first) // 一次只跨 man 这一个 codepoint
        val second = extend(
            text, selectionStart = 0, selectionEnd = 2,
            previous = first, direction = 1,
        )
        // 只承诺 codepoint 粒度：下一步走到 ZWJ 之后（3），不假装一次跳过整个 ZWJ 序列。
        assertRange(0, 3, second)
    }
}
