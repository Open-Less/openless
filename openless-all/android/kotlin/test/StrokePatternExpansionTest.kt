package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** `*` must stand for any stroke, including past what used to be the 25th expansion. */
class StrokePatternExpansionTest {
    private val keys = listOf('h', 's', 'p', 'n', 'z')

    @Test fun plainCodePassesThrough() {
        assertEquals(listOf("hspnz"), expandStrokePattern("hspnz"))
    }

    @Test fun wildcardCoverageIsExhaustiveUpToFour() {
        for (wildcards in 1..4) {
            val codes = expandStrokePattern("*".repeat(wildcards))
            var expected = 1
            repeat(wildcards) { expected *= keys.size }
            assertEquals("$wildcards wildcards", expected, codes.size)
            assertEquals("$wildcards wildcards must not repeat", codes.size, codes.toSet().size)
            assertTrue(
                "$wildcards wildcards must stay wildcards",
                codes.all { code -> code.length == wildcards && code.all { it in keys } },
            )
        }
        // A mid-pattern wildcard keeps its neighbours untouched.
        assertEquals(
            setOf("hhs", "hss", "hps", "hns", "hzs"),
            expandStrokePattern("h*s").toSet(),
        )
    }

    /**
     * Reported in review: `***phhspnpphs` used to stop after 25 codes, all with
     * `h` substituted for the first wildcard, so `新` (`nhnphhspnpphs`) was never
     * queried even though it is one of the few matching entries.
     */
    @Test fun threeWildcardsStillReachTheOnlyMatch() {
        val codes = expandStrokePattern("***phhspnpphs")
        assertEquals(125, codes.size)
        assertTrue("expansion dropped the only match for 新", "nhnphhspnpphs" in codes)
    }

    /**
     * Beyond four wildcards the product is too large to enumerate, so the query is
     * sampled — but every position must still vary instead of turning trailing
     * wildcards into one fixed stroke.
     */
    @Test fun manyWildcardsNeverPinPositionToAStroke() {
        for (wildcards in 5..8) {
            val codes = expandStrokePattern("*".repeat(wildcards))
            assertEquals("$wildcards wildcards", 625, codes.size)
            assertEquals("$wildcards wildcards must not repeat", codes.size, codes.toSet().size)
            for (position in 0 until wildcards) {
                assertEquals(
                    "position $position of $wildcards wildcards",
                    keys.toSet(),
                    codes.mapTo(mutableSetOf()) { it[position] },
                )
            }
        }
    }
}
