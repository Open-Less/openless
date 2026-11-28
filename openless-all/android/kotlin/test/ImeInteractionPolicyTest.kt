package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** SDK-free JUnit4：只验证策略的默认值、边界收敛与值语义。 */
class ImeInteractionPolicyTest {

    @Test
    fun defaultIntervalIsSixtyMilliseconds() {
        assertEquals(60L, ImeInteractionPolicy.DEFAULT.repeatIntervalMs)
    }

    @Test
    fun storedValueClampsToSupportedRange() {
        assertEquals(30L, ImeInteractionPolicy.fromStoredRepeat(Long.MIN_VALUE).repeatIntervalMs)
        assertEquals(30L, ImeInteractionPolicy.fromStoredRepeat(-1L).repeatIntervalMs)
        assertEquals(30L, ImeInteractionPolicy.fromStoredRepeat(0L).repeatIntervalMs)
        assertEquals(30L, ImeInteractionPolicy.fromStoredRepeat(29L).repeatIntervalMs)
        assertEquals(30L, ImeInteractionPolicy.fromStoredRepeat(30L).repeatIntervalMs)
        assertEquals(60L, ImeInteractionPolicy.fromStoredRepeat(60L).repeatIntervalMs)
        assertEquals(120L, ImeInteractionPolicy.fromStoredRepeat(120L).repeatIntervalMs)
        assertEquals(200L, ImeInteractionPolicy.fromStoredRepeat(200L).repeatIntervalMs)
        assertEquals(200L, ImeInteractionPolicy.fromStoredRepeat(201L).repeatIntervalMs)
        assertEquals(200L, ImeInteractionPolicy.fromStoredRepeat(Long.MAX_VALUE).repeatIntervalMs)
    }

    @Test
    fun everyStoredValueLandsInsideTheSupportedRange() {
        var sample = -400L
        while (sample <= 400L) {
            val interval = ImeInteractionPolicy.fromStoredRepeat(sample).repeatIntervalMs
            assertTrue("$sample -> $interval", interval in 30L..200L)
            sample += 7L
        }
    }

    @Test
    fun valueEqualityFollowsInterval() {
        assertEquals(ImeInteractionPolicy.fromStoredRepeat(60L), ImeInteractionPolicy.DEFAULT)
        assertEquals(
            ImeInteractionPolicy.fromStoredRepeat(30L),
            ImeInteractionPolicy.fromStoredRepeat(30L),
        )
        assertNotEquals(
            ImeInteractionPolicy.fromStoredRepeat(60L),
            ImeInteractionPolicy.fromStoredRepeat(61L),
        )
        assertNotEquals(ImeInteractionPolicy.DEFAULT, ImeInteractionPolicy.fromStoredRepeat(200L))
    }

    @Test
    fun derivedPoliciesAreStableForSameInput() {
        repeat(4) {
            val first = ImeInteractionPolicy.fromStoredRepeat(45L)
            val second = ImeInteractionPolicy.fromStoredRepeat(45L)
            assertEquals(first, second)
            assertEquals(45L, first.repeatIntervalMs)
        }
    }
}
