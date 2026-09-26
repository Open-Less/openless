package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.util.LinkedHashMap
import java.util.concurrent.Executors

/**
 * Full-pinyin single-character AND high-frequency-abbreviation lookup for
 * Pinyin mode (phases 3-5 of the lite-pinyin plan —
 * docs/pinyin-lite/phase-0-audit.md). Same background-load-off-a-dedicated-
 * thread shape as EnglishCandidateProvider/StrokePhraseRepository, but plain
 * exact-match Maps instead of a trie — ~5000 characters and ~2000 phrases is
 * small enough that a prefix index buys nothing (plan 8.1: "当前规模下无需
 * SQLite，也无需大型 Trie").
 *
 * Reads two android/assets files, each one row per entry:
 *   pinyin_chars.tsv:   character<TAB>pinyin<TAB>weight
 *   pinyin_phrases.tsv: phrase<TAB>full_pinyin<TAB>abbreviation<TAB>weight
 * (see scripts/generate-pinyin-characters.mjs / generate-pinyin-phrases.mjs
 * and their .LICENSE.txt files for where these come from).
 */
internal class LitePinyinRepository(context: Context) {
    private val appContext = context.applicationContext
    private val userFrequency = LitePinyinUserFrequency(context)
    private val learnedPhrases = LitePinyinLearnedPhrases(context)
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-pinyin-candidate").apply { isDaemon = true }
    }

    private data class Entry(val text: String, val weight: Int)

    // pinyin -> its characters; abbreviation -> its phrases. Each list is
    // sorted by weight descending once at load time (mirrors
    // EnglishCandidateProvider's Node.topWords: sort once on load, not per
    // query).
    private val charIndex = HashMap<String, List<Entry>>()
    private val abbreviationIndex = HashMap<String, List<Entry>>()

    // Caches only the STATIC merge (char entries then phrase entries, each
    // already weight-sorted) — same split as StrokePhraseRepository's own
    // cache: the user-frequency re-rank below is cheap (at most ~40
    // entries) and depends on data that changes independently of this, so
    // it's deliberately redone on every query rather than cached, which
    // sidesteps plan 8.2's "用户词频变化后，使相关缓存失效" requirement
    // entirely instead of having to implement invalidation.
    private val mergeCache = object : LinkedHashMap<String, List<Entry>>(CACHE_SIZE, .75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, List<Entry>>?) = size > CACHE_SIZE
    }

    @Volatile
    private var loaded = false

    /**
     * Off the caller's thread; posts [callback] back to the main looper.
     * Exact match only — no prefix/fuzzy matching in this phase. Ranks by
     * plan 3.5's five tiers, in order:
     *   0. a personally-learned two-commit combo for this exact encoding
     *      (LitePinyinLearnedPhrases — see observeSequence()) — not from
     *      the static dictionary at all, so it's pinned first outright
     *      rather than woven into the weight-based tiers below.
     *   1. a candidate the user picked before for this exact encoding
     *      (LitePinyinUserFrequency — see recordSelection())
     *   2. exact single-character full-pinyin matches
     *   3. exact abbreviation-phrase matches (only once the encoding is
     *      >= 2 letters — plan 3.4)
     *   4/5. each tier's own static weight / original order
     * Tiers 2 and 3 are never weight-interleaved with each other even
     * though pinyin_chars.tsv and pinyin_phrases.tsv share a comparable
     * weight scale (both ultimately from rime-pinyin-simp) — a phrase
     * never outranks a character on raw weight alone, only via tier 0/1.
     */
    fun query(encoding: String, limit: Int = 20, callback: (List<String>) -> Unit) {
        val normalized = encoding.trim().lowercase()
        if (normalized.isEmpty()) {
            callback(emptyList())
            return
        }
        executor.execute {
            ensureLoaded()
            val raw = mergedEntries(normalized)
            val previouslySelected = raw
                .filter { userFrequency.score(normalized, it.text) > 0.0 }
                .sortedByDescending { userFrequency.score(normalized, it.text) }
            val rest = raw.filterNot { entry -> previouslySelected.any { it.text == entry.text } }
            var ranked = (previouslySelected + rest).map { it.text }.distinct()
            learnedPhrases.promoted(normalized)?.let { learned -> ranked = listOf(learned) + ranked.filterNot { it == learned } }
            Handler(Looper.getMainLooper()).post { callback(ranked.take(limit)) }
        }
    }

    /** Call once a candidate is actually committed (a candidate tap — see OpenLessImeService.selectPinyinCandidate()) — off the caller's thread. */
    fun recordSelection(encoding: String, text: String) {
        val normalized = encoding.trim().lowercase()
        if (normalized.isEmpty() || text.isEmpty()) return
        executor.execute { userFrequency.record(normalized, text) }
    }

    /** Call after two direct pinyin commits land back-to-back — see LitePinyinController.observeCommitForLearning() — off the caller's thread. */
    fun observeSequence(combinedEncoding: String, text: String) {
        val normalized = combinedEncoding.trim().lowercase()
        if (normalized.isEmpty() || text.isEmpty()) return
        executor.execute { learnedPhrases.observeSequence(normalized, text) }
    }

    /** Warms both indexes off the caller's thread without waiting for a query — mirrors StrokePhraseRepository.preloadAsync(); call once, e.g. when the Pinyin panel first becomes reachable. */
    fun preloadAsync() {
        executor.execute { ensureLoaded() }
    }

    fun shutdown() = executor.shutdownNow()

    private fun mergedEntries(normalized: String): List<Entry> {
        synchronized(mergeCache) { mergeCache[normalized] }?.let { return it }
        val charMatches = charIndex[normalized].orEmpty()
        val phraseMatches = if (normalized.length >= 2) abbreviationIndex[normalized].orEmpty() else emptyList()
        val merged = charMatches + phraseMatches
        synchronized(mergeCache) { mergeCache[normalized] = merged }
        return merged
    }

    private fun ensureLoaded() {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            loadChars()
            loadPhrases()
            loaded = true
        }
    }

    private fun loadChars() {
        val grouped = HashMap<String, MutableList<Entry>>()
        runCatching {
            appContext.assets.open("pinyin_chars.tsv").bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    val parts = line.split('\t')
                    if (parts.size < 3) return@forEach
                    val char = parts[0]
                    val pinyin = parts[1]
                    val weight = parts[2].toIntOrNull() ?: 0
                    if (char.isEmpty() || pinyin.isEmpty()) return@forEach
                    grouped.getOrPut(pinyin) { mutableListOf() }.add(Entry(char, weight))
                }
            }
        }
        grouped.forEach { (pinyin, entries) -> charIndex[pinyin] = entries.sortedByDescending { it.weight } }
    }

    private fun loadPhrases() {
        val grouped = HashMap<String, MutableList<Entry>>()
        runCatching {
            appContext.assets.open("pinyin_phrases.tsv").bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    val parts = line.split('\t')
                    if (parts.size < 4) return@forEach
                    val phrase = parts[0]
                    val abbreviation = parts[2]
                    val weight = parts[3].toIntOrNull() ?: 0
                    if (phrase.isEmpty() || abbreviation.isEmpty()) return@forEach
                    grouped.getOrPut(abbreviation) { mutableListOf() }.add(Entry(phrase, weight))
                }
            }
        }
        // Within one abbreviation's own phrase list only (never crosses into
        // the char tier above it) — a manually curated domain term (see
        // PRIORITY_PHRASES) sorts before any ordinary corpus-ranked phrase
        // sharing that same code, regardless of either one's raw weight;
        // ties within each of those two groups still fall back to weight.
        grouped.forEach { (abbreviation, entries) ->
            abbreviationIndex[abbreviation] = entries.sortedWith(
                compareByDescending<Entry> { it.text in PRIORITY_PHRASES }.thenByDescending { it.weight },
            )
        }
    }

    private companion object {
        // Same size as StrokePhraseRepository's own cache — this one is
        // keyed by the exact (short) encoding string rather than a rolling
        // suffix, so cardinality is naturally bounded by realistic typing
        // patterns without needing a larger budget.
        const val CACHE_SIZE = 256

        // Mirrors generate-pinyin-phrases.mjs's own WHITELIST word list
        // (kept as a separate small constant here, not a 5th TSV column,
        // since it only ever affects in-app ranking, never the asset's
        // own membership or generation) — a whitelisted domain term the
        // user confirmed on 2026-09-25 outranks a same-abbreviation
        // ordinary corpus phrase on sight, since it was deliberately added
        // because the generic frequency corpus has no reason to rank it
        // highly on its own.
        val PRIORITY_PHRASES = setOf("输入法", "候选词", "剪贴板", "供应商", "物料", "主管")
    }
}
