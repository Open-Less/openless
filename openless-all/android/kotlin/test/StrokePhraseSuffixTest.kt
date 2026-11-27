package com.openless.app

import com.openless.app.StrokePhraseRepository.Candidate
import org.junit.Assert.assertEquals
import org.junit.Test

class StrokePhraseSuffixTest {
    /** A trie stand-in: every phrase is listed under each of its prefixes, itself included. */
    private fun lookupOf(vararg phrases: String): (String) -> List<Candidate> = { prefix ->
        phrases.filter { it.startsWith(prefix) }.map { Candidate(it, 1, prefix) }
    }

    private fun texts(context: String, lookup: (String) -> List<Candidate>) =
        longestSuffixExtensions(context, lookup).map { it.text }

    @Test fun phrasesContinuingTheLongestSuffixAreOffered() {
        assertEquals(listOf("天气好", "天气预报"), texts("今天天气", lookupOf("天气好", "天气预报", "气球")))
    }

    @Test fun thePhraseTheSuffixAlreadyCompletesIsDropped() {
        assertEquals(listOf("天气预报"), texts("今天天气", lookupOf("天气", "天气预报")))
    }

    /** The reported bug: after the last association was picked, it stayed in the row. */
    @Test fun nothingIsOfferedOnceTheLastPhraseIsOnScreen() {
        assertEquals(emptyList<String>(), texts("今天天气预报", lookupOf("天气预报", "预报员", "报告")))
    }

    @Test fun aShorterSuffixIsTriedOnlyWhenTheLongerOneMatchesNothing() {
        assertEquals(listOf("好的"), texts("你好", lookupOf("好的")))
    }

    @Test fun emptyContextOrNoMatchGivesNothing() {
        assertEquals(emptyList<String>(), texts("", lookupOf("天气")))
        assertEquals(emptyList<String>(), texts("你好", lookupOf("天气")))
    }

    private fun ranked(weights: Map<String, Int>, scores: Map<String, Double>) =
        rankAssociations(weights.map { (text, weight) -> Candidate(text, weight, "天气") }) { scores[it.text] ?: 0.0 }
            .map { it.text }

    /** The old additive order could never lift 天气图 (weight 7) over 天气预报 (2054), however often it was picked. */
    @Test fun aPickedPhraseOutranksAnUnpickedOneWhateverTheCorpusWeight() {
        val weights = mapOf("天气预报" to 2054, "天气形势" to 12, "天气图" to 7)
        assertEquals(listOf("天气图", "天气预报", "天气形势"), ranked(weights, mapOf("天气图" to 79.0)))
    }

    @Test fun pickedPhrasesFollowTheirScoreAndTheRestTheirWeight() {
        val weights = mapOf("天气预报" to 2054, "天气形势" to 12, "天气图" to 7, "天气变化" to 1)
        assertEquals(
            listOf("天气变化", "天气图", "天气预报", "天气形势"),
            ranked(weights, mapOf("天气图" to 79.0, "天气变化" to 216.0)),
        )
        assertEquals(listOf("天气预报", "天气形势", "天气图", "天气变化"), ranked(weights, emptyMap()))
    }

    @Test fun typingOutAnOfferedAssociationCountsAsPickingIt() {
        val lookup = lookupOf("天气", "天气预报", "天气好")
        val hit = typedContinuation("今天天气", "预报", lookup)
        assertEquals("天气预报", hit?.text)
        assertEquals("天气", hit?.matchedPrefix)
    }

    @Test fun splitCommitsLearnTheOriginalAssociationPrefix() {
        val lookup = lookupOf("天气", "天气预报")
        val history = TypedContinuationHistory()
        assertEquals(emptyList<Candidate>(), history.advance("天气", "预").mapNotNull {
            typedContinuation(it.contextBefore, it.word, lookup)
        })
        val completed = history.advance("天气预", "报").mapNotNull {
            typedContinuation(it.contextBefore, it.word, lookup)
        }
        assertEquals(listOf("天气", "天气预"), completed.map { it.matchedPrefix })
        assertEquals(listOf("天气预报", "天气预报"), completed.map { it.text })
    }

    @Test fun batchCommitsStillLearnAndUnrelatedTextDoesNot() {
        val lookup = lookupOf("天气", "天气预报")
        val history = TypedContinuationHistory()
        val batch = history.advance("天气", "预报").mapNotNull {
            typedContinuation(it.contextBefore, it.word, lookup)
        }
        assertEquals(listOf("天气"), batch.map { it.matchedPrefix })
        assertEquals(emptyList<Candidate>(), history.advance("天气预报", "雨").mapNotNull {
            typedContinuation(it.contextBefore, it.word, lookup)
        })
    }

    @Test fun resetAndContextChangesDiscardPendingContinuations() {
        val history = TypedContinuationHistory()
        history.advance("天气", "预")
        history.reset()
        assertEquals(listOf(TypedContinuation("天气预", "报")), history.advance("天气预", "报"))
        history.advance("天气", "预")
        assertEquals(listOf(TypedContinuation("别处", "报")), history.advance("别处", "报"))
    }

    @Test fun historyIsBoundedAndDoesNotInventCommitBoundaries() {
        val history = TypedContinuationHistory()
        assertEquals(emptyList<TypedContinuation>(), history.advance("", "天气"))
        assertEquals(listOf(TypedContinuation("天气", "预报")), history.advance("天气", "预报"))
        var context = "天气预报"
        repeat(20) {
            val continuations = history.advance(context, "字")
            assertEquals(true, continuations.size <= 7)
            assertEquals(true, continuations.all { it.word.length < 8 })
            context = (context + "字").takeLast(8)
        }
    }

    @Test fun typingSomethingElseCountsAsNothing() {
        val lookup = lookupOf("天气", "天气预报")
        // Only part of the association, more than it, or unrelated text.
        assertEquals(null, typedContinuation("今天天气", "预", lookup))
        assertEquals(null, typedContinuation("今天天气", "预报员", lookup))
        assertEquals(null, typedContinuation("今天天气", "很好", lookup))
        // The phrase that is already complete is never offered, so it can't be "typed out".
        assertEquals(null, typedContinuation("今天", "天气", lookupOf("今天")))
        assertEquals(null, typedContinuation("", "天气", lookup))
    }
}
