package com.openless.app

import android.content.Context
import kotlin.math.exp
import kotlin.math.ln

/**
 * Local-only candidate preference store, shared globally across target apps
 * — which characters/phrases you mean for a given context or stroke code is
 * a personal typing habit, not something that differs by app.
 */
internal class StrokeUserFrequency(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(STORE, Context.MODE_PRIVATE)
    private val cache = FrequencyStampCache.of(context, STORE) { key ->
        key.indexOf(SEPARATOR).takeIf { it >= 0 }?.let { key.substring(0, it) }
    }

    // Tracks entry count without paying preferences.all's full-map-copy cost
    // on every record() — see trimIfNeeded(). -1 means "not yet known",
    // resolved (once) the first time it's actually needed.
    @Volatile
    private var approxSize = -1

    fun score(context: String, candidate: String, now: Long = System.currentTimeMillis()): Double {
        val stamp = cache.get(key(context, candidate)) ?: return 0.0
        val ageDays = ((now - stamp.lastUsedMs).coerceAtLeast(0L)).toDouble() / MILLIS_PER_DAY
        return ln(1.0 + stamp.count) * COUNT_WEIGHT + exp(-ageDays / RECENCY_HALF_LIFE_DAYS) * RECENCY_WEIGHT
    }

    /** Every candidate recorded for exactly [context], with its score — read from memory, no store scan. */
    fun picksFor(context: String, now: Long = System.currentTimeMillis()): List<Pair<String, Double>> =
        cache.keysInGroup(context).map { key ->
            val candidate = key.substring(context.length + SEPARATOR.length)
            candidate to score(context, candidate, now)
        }

    fun record(context: String, candidate: String) {
        if (candidate.isBlank()) return
        OpenLessMigrationState.learn {
            val entryKey = key(context, candidate)
            val isNewEntry = !preferences.contains(entryKey)
            val count = (cache.get(entryKey)?.count ?: 0) + 1
            val now = System.currentTimeMillis()
            preferences.edit().putString(entryKey, "$count,$now").apply()
            cache.put(entryKey, FrequencyStampCache.Stamp(count, now))
            // Re-recording an existing entry (by far the common case — the same
            // handful of characters/phrases typed over and over) never touches
            // preferences.all at all now. Only a genuinely new key can push the
            // count past MAX_ENTRIES, so that's the only case worth checking.
            if (isNewEntry) {
                val size = resolvedSize() + 1
                approxSize = size
                if (size > MAX_ENTRIES) trimIfNeeded()
            }
        }
    }

    fun clear() {
        check(OpenLessMigrationState.settings {
            preferences.edit().clear().apply()
            cache.invalidate()
            approxSize = 0
        }) { "migration_android_busy" }
    }

    /** Current entry count, for showing usage against [MAX_ENTRIES] in settings. */
    fun size(): Int = resolvedSize()

    fun capacity(): Int = MAX_ENTRIES

    /** Every stored entry as a plain "context<sep>candidate"->"count,lastUsedMs" map, for the settings page's export/import feature — see OpenLessSettingsExport. */
    fun exportAll(): Map<String, String> {
        @Suppress("UNCHECKED_CAST")
        return preferences.all as Map<String, String>
    }

    /** Replaces the entire store with [entries] — an import wholesale-restores a backup rather than merging with whatever's already on this device. */
    fun importAll(entries: Map<String, String>) {
        check(OpenLessMigrationState.settings {
            check(preferences.edit().clear().apply { entries.forEach { (key, value) -> putString(key, value) } }.commit()) { "Stroke learning save failed" }
            cache.invalidate()
            approxSize = entries.size
        }) { "migration_android_busy" }
    }

    private fun key(context: String, candidate: String) = "$context$SEPARATOR$candidate"

    private fun resolvedSize(): Int {
        val cached = approxSize
        if (cached >= 0) return cached
        val size = preferences.all.size
        approxSize = size
        return size
    }

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
        const val STORE = "openless_stroke_frequency"
        const val SEPARATOR = ""
        // Shared by both phrase-association contexts and stroke-code
        // contexts; each entry is well under 100 bytes so even the full cap
        // stays a few hundred KB, cheap to parse into memory on first use.
        const val MAX_ENTRIES = 3000
        const val MILLIS_PER_DAY = 86_400_000L
        const val RECENCY_HALF_LIFE_DAYS = 30.0
        const val COUNT_WEIGHT = 80.0
        const val RECENCY_WEIGHT = 24.0
    }
}
