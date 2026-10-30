package com.openless.app

import android.content.Context
import kotlin.math.exp
import kotlin.math.ln

/**
 * Local-only English word usage store — same shape and scoring formula as
 * StrokeUserFrequency (log-scaled hit count + exponential recency decay),
 * just without that class's per-context keying, since a typed word's
 * ranking here doesn't depend on what preceded it.
 *
 * Also holds the small, separate set of explicitly user-known words that
 * aren't in the bundled base dictionary at all (recordCommit() adds a word
 * here the first time it's committed if EnglishCandidateProvider doesn't
 * already index it — see that class) — this is the data layer for
 * "user custom words" (add/delete/exists/frequency/last-used). Delete is
 * wired up as a long-press on an English candidate — see
 * EnglishCandidateProvider.forgetCustomWord()/isCustomWord() and
 * OpenLessImeService.forgetEnglishCandidate().
 */
internal class EnglishUserFrequency(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(STORE, Context.MODE_PRIVATE)
    private val customWordsPrefs = context.applicationContext.getSharedPreferences(CUSTOM_STORE, Context.MODE_PRIVATE)
    private val cache = FrequencyStampCache.of(context, STORE)

    // Tracks entry count without paying preferences.all's full-map-copy cost
    // on every record() — see trimIfNeeded(). -1 means "not yet known",
    // resolved (once) the first time it's actually needed.
    @Volatile
    private var approxSize = -1

    fun score(word: String, now: Long = System.currentTimeMillis()): Double {
        val stamp = cache.get(key(word)) ?: return 0.0
        val ageDays = ((now - stamp.lastUsedMs).coerceAtLeast(0L)).toDouble() / MILLIS_PER_DAY
        return ln(1.0 + stamp.count) * COUNT_WEIGHT + exp(-ageDays / RECENCY_HALF_LIFE_DAYS) * RECENCY_WEIGHT
    }

    fun record(word: String) {
        if (word.isBlank()) return
        val entryKey = key(word)
        val isNewEntry = !preferences.contains(entryKey)
        val count = (cache.get(entryKey)?.count ?: 0) + 1
        val now = System.currentTimeMillis()
        preferences.edit().putString(entryKey, "$count,$now").apply()
        cache.put(entryKey, FrequencyStampCache.Stamp(count, now))
        // Re-recording an existing word (by far the common case) never
        // touches preferences.all at all now — only a genuinely new word
        // can push the count past MAX_ENTRIES, so that's the only case
        // worth checking.
        if (isNewEntry) {
            val size = resolvedSize() + 1
            approxSize = size
            if (size > MAX_ENTRIES) trimIfNeeded()
        }
    }

    fun clear() {
        preferences.edit().clear().apply()
        cache.invalidate()
        approxSize = 0
    }

    fun size(): Int = resolvedSize()

    fun capacity(): Int = MAX_ENTRIES

    private fun resolvedSize(): Int {
        val cached = approxSize
        if (cached >= 0) return cached
        val size = preferences.all.size
        approxSize = size
        return size
    }

    // --- User custom words (section 十二: Add/Delete/Exists/Frequency/LastUsed) ---
    // Frequency/LastUsed for a custom word are the same record()/score() above,
    // keyed by the same normalized word — a custom word is simply a word this
    // set says should be indexed even though the base dictionary doesn't have it.

    fun addCustomWord(rawWord: String): Boolean {
        val word = normalize(rawWord)
        if (word.isEmpty() || word.length > MAX_CUSTOM_WORD_LENGTH) return false
        if (customWordsPrefs.contains(word)) return false
        customWordsPrefs.edit().putBoolean(word, true).apply()
        return true
    }

    fun removeCustomWord(rawWord: String) {
        customWordsPrefs.edit().remove(normalize(rawWord)).apply()
    }

    fun hasCustomWord(rawWord: String): Boolean = customWordsPrefs.contains(normalize(rawWord))

    fun customWords(): Set<String> = customWordsPrefs.all.keys

    private fun normalize(word: String): String = word.trim().lowercase()

    private fun key(word: String) = normalize(word)

    private fun trimIfNeeded() {
        val all = preferences.all
        approxSize = all.size
        if (all.size <= MAX_ENTRIES) return
        val removeCount = all.size - MAX_ENTRIES
        val oldest = all.entries
            .sortedBy { it.value.toString().substringAfterLast(',').toLongOrNull() ?: 0L }
            .take(removeCount)
        preferences.edit().apply {
            oldest.forEach { remove(it.key) }
        }.apply()
        approxSize = all.size - oldest.size
    }

    private companion object {
        const val STORE = "openless_english_frequency"
        const val CUSTOM_STORE = "openless_english_custom_words"
        const val MAX_ENTRIES = 3000
        const val MAX_CUSTOM_WORD_LENGTH = 40
        const val MILLIS_PER_DAY = 86_400_000L
        const val RECENCY_HALF_LIFE_DAYS = 30.0
        const val COUNT_WEIGHT = 80.0
        const val RECENCY_WEIGHT = 24.0
    }
}
