package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class InputLearningRulesTest {
    /** The old query: every match, scored and fully sorted. */
    private fun naiveStrokeRank(
        fileOrder: List<Pair<String, String>>,
        pattern: String,
        scores: Map<String, Double>,
        freq: Map<String, Long>,
        limit: Int,
    ): List<String> = fileOrder
        .filter { (_, code) -> strokeCodeMatches(pattern, code) }
        .map { it.first }
        .distinct()
        .sortedWith(compareByDescending<String> { scores[it] ?: 0.0 }.thenByDescending { freq[it] ?: 0L })
        .take(limit)

    @Test fun strokeRankingMatchesTheOldFullSortOnRandomData() {
        val random = java.util.Random(42)
        val chars = (0 until 60).map { "字$it" }
        val freq = chars.associateWith { (random.nextInt(5) * 100).toLong() }  // plenty of ties
        val fileOrder = (0 until 400).map {
            chars[random.nextInt(chars.size)] to (1..(1 + random.nextInt(5))).map { "hspnz"[random.nextInt(5)] }.joinToString("")
        }
        val sorted = fileOrder.sortedByDescending { freq[it.first] ?: 0L }
        listOf("h", "s", "hs", "p*", "*z", "hsp", "nn", "zzzzzz").forEach { pattern ->
            val matching = fileOrder.filter { strokeCodeMatches(pattern, it.second) }.map { it.first }.distinct()
            // Distinct scores: real ones carry a millisecond timestamp, so two picks never tie.
            val scores = matching.shuffled(random).take(3).withIndex().associate { (i, char) -> char to i * 50.0 + 1.0 }
            listOf(1, 5, 36).forEach { limit ->
                assertEquals(
                    "pattern=$pattern limit=$limit",
                    naiveStrokeRank(fileOrder, pattern, scores, freq, limit),
                    rankStrokeCandidates(sorted, pattern, scores.toList(), limit) { freq[it] ?: 0L },
                )
            }
        }
    }

    @Test fun aPickedCharacterLeadsEvenWithTheLowestFrequency() {
        val entries = listOf("的" to "p", "了" to "z", "人" to "p", "仁" to "pp")
        val freq = mapOf("的" to 1000L, "人" to 500L, "仁" to 1L)
        assertEquals(listOf("仁", "的", "人"), rankStrokeCandidates(entries, "p", listOf("仁" to 79.0), 36) { freq[it] ?: 0L })
        assertEquals(listOf("仁"), rankStrokeCandidates(entries, "p", listOf("仁" to 79.0), 1) { freq[it] ?: 0L })
        assertEquals(listOf("的", "人", "仁"), rankStrokeCandidates(entries, "p", emptyList(), 36) { freq[it] ?: 0L })
    }

    @Test fun storedUsageParsesOnlyWellFormedValues() {
        assertEquals(FrequencyStampCache.Stamp(3, 1700000000000L), FrequencyStampCache.parseStamp("3,1700000000000"))
        assertNull(FrequencyStampCache.parseStamp(null))
        assertNull(FrequencyStampCache.parseStamp(""))
        assertNull(FrequencyStampCache.parseStamp("3"))
        assertNull(FrequencyStampCache.parseStamp(",17"))
        assertNull(FrequencyStampCache.parseStamp("x,17"))
        assertNull(FrequencyStampCache.parseStamp("3,abc"))
    }

    @Test fun strokePickIsRecordedWhenCorrectedOrAlreadyLearned() {
        assertTrue(shouldRecordStrokePick(wasTop = false, alreadyLearned = false))
        assertTrue(shouldRecordStrokePick(wasTop = false, alreadyLearned = true))
        assertTrue(shouldRecordStrokePick(wasTop = true, alreadyLearned = true))
        assertFalse(shouldRecordStrokePick(wasTop = true, alreadyLearned = false))
    }
}
