package com.openless.app

import android.content.Context
import kotlin.math.exp
import kotlin.math.ln

/**
 * Local-only Pinyin-mode selection-history store — phase 5 of the lite-pinyin
 * plan (docs/pinyin-lite/phase-0-audit.md). Same shape and scoring formula as
 * EnglishUserFrequency/StrokeUserFrequency (log-scaled hit count + exponential
 * recency decay), keyed by "编码 + 候选文本" per plan 8.4 — a selection only
 * boosts that exact encoding's ranking, not the candidate text in general
 * (picking 国 after typing "guo" shouldn't also boost it for an unrelated
 * encoding that happens to also resolve to 国).
 */
internal class LitePinyinUserFrequency(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(STORE, Context.MODE_PRIVATE)

    fun score(encoding: String, text: String, now: Long = System.currentTimeMillis()): Double {
        val value = preferences.getString(key(encoding, text), null) ?: return 0.0
        val parts = value.split(',')
        val count = parts.getOrNull(0)?.toDoubleOrNull() ?: return 0.0
        val last = parts.getOrNull(1)?.toLongOrNull() ?: return 0.0
        val ageDays = ((now - last).coerceAtLeast(0L)).toDouble() / MILLIS_PER_DAY
        return ln(1.0 + count) * COUNT_WEIGHT + exp(-ageDays / RECENCY_HALF_LIFE_DAYS) * RECENCY_WEIGHT
    }

    fun record(encoding: String, text: String) {
        if (encoding.isBlank() || text.isBlank()) return
        val entryKey = key(encoding, text)
        val old = preferences.getString(entryKey, null)?.split(',')
        val count = (old?.getOrNull(0)?.toIntOrNull() ?: 0) + 1
        val now = System.currentTimeMillis()
        preferences.edit().putString(entryKey, "$count,$now").apply()
    }

    /** Clearing this app's data (plan 16's own acceptance criterion: "清除应用数据可恢复默认排序") already wipes this preferences file with everything else — this is only for an explicit in-app reset, not currently wired to any UI. */
    fun clear() = preferences.edit().clear().apply()

    private fun key(encoding: String, text: String) = "${encoding.trim().lowercase()}|$text"

    private companion object {
        const val STORE = "openless_pinyin_frequency"
        const val MILLIS_PER_DAY = 86_400_000L
        const val RECENCY_HALF_LIFE_DAYS = 30.0
        const val COUNT_WEIGHT = 80.0
        const val RECENCY_WEIGHT = 24.0
    }
}
