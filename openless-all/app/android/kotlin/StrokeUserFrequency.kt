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

    fun score(context: String, candidate: String, now: Long = System.currentTimeMillis()): Double {
        val value = preferences.getString(key(context, candidate), null) ?: return 0.0
        val parts = value.split(',')
        val count = parts.getOrNull(0)?.toDoubleOrNull() ?: return 0.0
        val last = parts.getOrNull(1)?.toLongOrNull() ?: return 0.0
        val ageDays = ((now - last).coerceAtLeast(0L)).toDouble() / MILLIS_PER_DAY
        return ln(1.0 + count) * COUNT_WEIGHT + exp(-ageDays / RECENCY_HALF_LIFE_DAYS) * RECENCY_WEIGHT
    }

    fun record(context: String, candidate: String) {
        if (candidate.isBlank()) return
        val entryKey = key(context, candidate)
        val old = preferences.getString(entryKey, null)?.split(',')
        val count = (old?.getOrNull(0)?.toIntOrNull() ?: 0) + 1
        val now = System.currentTimeMillis()
        preferences.edit().putString(entryKey, "$count,$now").apply()
        trimIfNeeded()
    }

    fun clear() = preferences.edit().clear().apply()

    /** Current entry count, for showing usage against [MAX_ENTRIES] in settings. */
    fun size(): Int = preferences.all.size

    fun capacity(): Int = MAX_ENTRIES

    private fun key(context: String, candidate: String) = "$context$SEPARATOR$candidate"

    private fun trimIfNeeded() {
        val all = preferences.all
        if (all.size <= MAX_ENTRIES) return
        val removeCount = all.size - MAX_ENTRIES
        val oldest = all.entries
            .sortedBy { it.value.toString().substringAfterLast(',').toLongOrNull() ?: 0L }
            .take(removeCount)
        preferences.edit().apply {
            oldest.forEach { remove(it.key) }
        }.apply()
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
