package com.openless.app

import android.content.Context
import kotlin.math.exp
import kotlin.math.ln

/**
 * Local-only candidate preference store, shared globally across target apps
 * — which characters/phrases you mean for a given context or stroke code is
 * a personal typing habit, not something that differs by app. Two separate
 * stores, each with its own cap: [strokeCodes] (stroke code -> picked
 * character) and [associations] (text before -> picked association), so
 * heavy use of one never evicts the other.
 */
internal class StrokeUserFrequency private constructor(context: Context, store: String, private val maxEntries: Int) {
    private val preferences = context.applicationContext.getSharedPreferences(store, Context.MODE_PRIVATE)
    private val cache = FrequencyStampCache.of(context, store) { key ->
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
            // count past the cap, so that's the only case worth checking.
            if (isNewEntry) {
                val size = resolvedSize() + 1
                approxSize = size
                if (size > maxEntries) trimIfNeeded()
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

    /** Current entry count, for showing usage against [capacity] in settings. */
    fun size(): Int = resolvedSize()

    fun capacity(): Int = maxEntries

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

    /** Merges [entries] (already checked with [isValidEntry]) into this store: per entry the higher count and the later use win; past the cap the least recently used go. */
    fun mergeAll(entries: Map<String, String>) {
        check(OpenLessMigrationState.settings {
            val current = exportAll()
            check(preferences.edit().apply {
                entries.forEach { (key, value) -> putString(key, current[key]?.let { mergeStamps(it, value) } ?: value) }
            }.commit()) { "Stroke learning save failed" }
            cache.invalidate()
            trimIfNeeded()
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

    // Past the cap, drops the least recently used down to TRIM_TO of it in one
    // pass, so the full copy and sort run once per batch rather than for every
    // new entry once the store is full.
    private fun trimIfNeeded() {
        val all = preferences.all
        approxSize = all.size
        if (all.size <= maxEntries) return
        val removeCount = all.size - trimTarget(maxEntries)
        val oldest = all.entries
            .sortedBy { it.value.toString().substringAfterLast(',').toLongOrNull() ?: 0L }
            .take(removeCount)
        preferences.edit().apply {
            oldest.forEach { remove(it.key) }
        }.apply()
        approxSize = all.size - oldest.size
    }

    companion object {
        private const val STROKE_STORE = "openless_stroke_frequency"
        private const val ASSOCIATION_STORE = "openless_association_frequency"
        private const val SEPARATOR = ""
        // Each entry is well under 100 bytes, so even the larger cap is under a
        // megabyte to parse on first use. Together they stay within the
        // migration bundle's 20,000-entry limit for "strokeFrequency".
        private const val MAX_STROKE_ENTRIES = 5000
        private const val MAX_ASSOCIATION_ENTRIES = 10_000
        private const val TRIM_TO = 0.9
        private const val MILLIS_PER_DAY = 86_400_000L
        private const val RECENCY_HALF_LIFE_DAYS = 30.0
        private const val COUNT_WEIGHT = 80.0
        private const val RECENCY_WEIGHT = 24.0
        @Volatile private var split = false

        fun strokeCodes(context: Context) = StrokeUserFrequency(context, STROKE_STORE, MAX_STROKE_ENTRIES).also { splitSharedStore(context) }

        fun associations(context: Context) = StrokeUserFrequency(context, ASSOCIATION_STORE, MAX_ASSOCIATION_ENTRIES).also { splitSharedStore(context) }

        /** How many entries a store over [cap] keeps after trimming. */
        fun trimTarget(cap: Int): Int = (cap * TRIM_TO).toInt()

        /** A "context<sep>candidate" -> "count,lastUsedMs" entry as record() writes it. */
        fun isValidEntry(key: String, value: String): Boolean {
            val separator = key.indexOf(SEPARATOR)
            return separator > 0 && separator < key.length - 1 && key.toByteArray(Charsets.UTF_8).size <= 256 &&
                FrequencyStampCache.parseStamp(value)?.let { it.count > 0 && it.lastUsedMs >= 0 } == true
        }

        /** Two records of one entry as one: the higher count and the later use. Both must be valid stamps. */
        fun mergeStamps(a: String, b: String): String {
            val first = checkNotNull(FrequencyStampCache.parseStamp(a))
            val second = checkNotNull(FrequencyStampCache.parseStamp(b))
            return "${maxOf(first.count, second.count)},${maxOf(first.lastUsedMs, second.lastUsedMs)}"
        }

        /** Stroke-code contexts are spelled with stroke keys only; association contexts are committed text. */
        fun isStrokeCodeKey(key: String): Boolean {
            val separator = key.indexOf(SEPARATOR)
            return separator > 0 && key.substring(0, separator).all { it in "hspnz*" }
        }

        /** Both stores as one map — the shape backups and the migration bundle's "strokeFrequency" have always had. */
        fun exportCombined(context: Context): Map<String, String> =
            strokeCodes(context).exportAll() + associations(context).exportAll()

        /** Replaces both stores from a combined map, each entry going to the store its key belongs to. */
        fun importCombined(context: Context, entries: Map<String, String>) {
            val (codes, phrases) = entries.entries.partition { isStrokeCodeKey(it.key) }
            strokeCodes(context).importAll(codes.associate { it.toPair() })
            associations(context).importAll(phrases.associate { it.toPair() })
        }

        // Until both kinds got their own store they shared the stroke one;
        // move the association entries out once.
        private fun splitSharedStore(context: Context) {
            if (split) return
            synchronized(this) {
                if (split) return
                split = true
                val app = context.applicationContext
                val marker = app.getSharedPreferences("openless_learning_layout", Context.MODE_PRIVATE)
                if (marker.getBoolean("associations_split", false)) return
                OpenLessMigrationState.learn {
                    val stroke = app.getSharedPreferences(STROKE_STORE, Context.MODE_PRIVATE)
                    val moved = stroke.all.filter { (key, value) -> value is String && !isStrokeCodeKey(key) }
                    if (moved.isNotEmpty()) {
                        val target = app.getSharedPreferences(ASSOCIATION_STORE, Context.MODE_PRIVATE)
                        check(target.edit().apply { moved.forEach { (key, value) -> putString(key, value as String) } }.commit())
                        check(stroke.edit().apply { moved.keys.forEach { remove(it) } }.commit())
                    }
                    marker.edit().putBoolean("associations_split", true).apply()
                }
            }
        }
    }
}
