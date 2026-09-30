package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executors

/**
 * English word-completion candidates for the ABC keyboard layout. Same
 * architecture as StrokePhraseRepository (lazy asset load off a dedicated
 * background thread, an in-memory trie, an LRU query cache, base weight
 * re-ranked by a UserFrequency store at query time) — deliberately reused
 * rather than inventing a second indexing scheme, since it already satisfies
 * this feature's own "no full-dictionary scan per keystroke" requirement.
 *
 * The trie is a genuine PREFIX index (unlike StrokePhraseRepository's, which
 * indexes by suffix for its own "what comes after this confirmed text"
 * question): every node reachable by walking a prefix's characters from the
 * root holds that prefix's own top-N words by base frequency, so a query is
 * one node lookup, not a subtree walk.
 */
internal class EnglishCandidateProvider(context: Context) {
    private val appContext = context.applicationContext
    private val userFrequency = EnglishUserFrequency(context)
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-english-candidate").apply { isDaemon = true }
    }

    private class Node {
        val children = HashMap<Char, Node>()
        // Filled in frequency-descending insertion order (see ensureLoaded()),
        // so simply capping at NODE_TOP_N already keeps the top entries with
        // no per-insert sort or eviction needed.
        val topWords = ArrayList<Entry>(NODE_TOP_N)
    }

    private data class Entry(val word: String, val baseWeight: Int)

    private val root = Node()
    private val indexedWords = HashSet<String>()
    // Words long-pressed away via forgetCustomWord() — the trie itself has
    // no live-removal (only insertion; see insert()/walk()), so this is a
    // cheap post-filter instead of rebuilding the index. Only ever touched
    // from executor's thread, same as indexedWords/loaded.
    private val excludedWords = HashSet<String>()
    private var loaded = false

    /** Off the caller's thread; posts [callback] back to the main looper. */
    fun queryTopN(prefix: String, limit: Int = 3, callback: (List<String>) -> Unit) {
        val normalized = prefix.trim().lowercase()
        if (normalized.isEmpty()) {
            callback(emptyList())
            return
        }
        executor.execute {
            ensureLoaded()
            val candidates = walk(normalized)
            val ranked = candidates
                .asSequence()
                .filter { it.word !in excludedWords }
                .sortedByDescending { it.baseWeight + userFrequency.score(it.word) }
                .map { it.word }
                .distinct()
                .take(limit)
                .toList()
            Handler(Looper.getMainLooper()).post { callback(ranked) }
        }
    }

    /** True only for a word the user's own typing added (see recordCommit()) — the bundled base dictionary isn't user-removable. */
    fun isCustomWord(word: String): Boolean = userFrequency.hasCustomWord(word)

    /**
     * "Forget" a word this keyboard only knows because the user typed it
     * once (long-press on an English candidate — see
     * OpenLessImeService.forgetEnglishCandidate()). Un-marks it as custom
     * (so a future cold start's ensureLoaded() won't re-index it) and hides
     * it from this session's own suggestions immediately, without needing
     * to rebuild the trie.
     */
    fun forgetCustomWord(word: String) {
        val normalized = word.trim().lowercase()
        if (normalized.isEmpty()) return
        executor.execute {
            userFrequency.removeCustomWord(normalized)
            excludedWords.add(normalized)
        }
    }

    /**
     * Call once a word is actually committed (typed to completion and
     * followed by a boundary, or tapped from the candidate bar) — boosts its
     * ranking for next time, and — if it wasn't already indexed at all —
     * inserts it as an implicit custom word so future prefixes of it can
     * find it, which is what makes "user custom words" work without a
     * separate explicit add step.
     */
    fun recordCommit(word: String) {
        val normalized = word.trim().lowercase()
        if (normalized.length < 2) return
        executor.execute {
            ensureLoaded()
            userFrequency.record(normalized)
            if (normalized !in indexedWords) {
                userFrequency.addCustomWord(normalized)
                insert(normalized, CUSTOM_WORD_BASE_WEIGHT)
            }
        }
    }

    fun shutdown() = executor.shutdownNow()

    private fun ensureLoaded() {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            runCatching {
                appContext.assets.open("english-frequency.tsv").bufferedReader().useLines { lines ->
                    lines.forEach { line ->
                        val parts = line.split('\t', limit = 2)
                        val word = parts.getOrNull(0) ?: return@forEach
                        val weight = parts.getOrNull(1)?.toIntOrNull() ?: 0
                        // Frequencies in the source file span many orders of
                        // magnitude (millions down to hundreds); dividing
                        // into log-ish buckets keeps the ranking formula's
                        // baseWeight comparable in scale to userFrequency's
                        // own score() range instead of always drowning it
                        // out for the small handful of extremely common words.
                        if (word.isNotEmpty() && weight > 0) insert(word, bucketWeight(weight))
                    }
                }
            }
            // User-added custom words from a previous session need to be
            // re-indexed on every fresh load too, not just remembered as a
            // set — the trie itself is rebuilt from scratch each process.
            runCatching {
                userFrequency.customWords().forEach { word ->
                    if (word !in indexedWords) insert(word, CUSTOM_WORD_BASE_WEIGHT)
                }
            }
            loaded = true
        }
    }

    private fun insert(word: String, baseWeight: Int) {
        if (!indexedWords.add(word)) return
        val entry = Entry(word, baseWeight)
        var node = root
        for (character in word) {
            node = node.children.getOrPut(character) { Node() }
            if (node.topWords.size < NODE_TOP_N) node.topWords += entry
        }
    }

    private fun walk(prefix: String): List<Entry> {
        var node = root
        for (character in prefix) {
            node = node.children[character] ?: return emptyList()
        }
        return node.topWords
    }

    /** Roughly log-scaled: millions -> ~140, thousands -> ~90, hundreds -> ~60. */
    private fun bucketWeight(rawCount: Int): Int = (kotlin.math.ln(rawCount.toDouble()) * 10).toInt()

    private companion object {
        const val NODE_TOP_N = 20
        const val CUSTOM_WORD_BASE_WEIGHT = 70
    }
}
