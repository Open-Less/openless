package com.openless.app

import android.content.Context

/**
 * Learns a multi-character "combo" phrase purely from the user's own
 * consecutive typing pattern — no manual whitelist entry required. If the
 * user types encoding "zg" and picks 中国, then immediately types "rm" and
 * picks 人民, this records the pair (combined encoding "zgrm" -> combined
 * text 中国人民) with a hit count; once that exact combo has been observed
 * [PROMOTION_THRESHOLD] times, it "graduates" — LitePinyinRepository.query()
 * starts surfacing it (pinned first) whenever the user types "zgrm" from
 * scratch, the same effect a manually-curated whitelist entry would have,
 * except it only ever contains combos this specific user actually built up
 * themselves.
 *
 * Deliberately its own small store, not folded into LitePinyinUserFrequency
 * — that one ranks/reorders candidates already present in the static
 * dictionary; this one's job is to conjure a candidate that was NEVER in
 * the dictionary at all, gated by a hit-count threshold rather than a
 * continuous score.
 */
internal class LitePinyinLearnedPhrases(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(STORE, Context.MODE_PRIVATE)

    /**
     * Call once per observed two-step sequence (see
     * LitePinyinController.observeCommitForLearning()). Only overwrites the
     * stored hit count when [text] matches what's already stored for
     * [combinedEncoding] — a different text combo landing on the same
     * encoding starts its own count from zero rather than inheriting an
     * unrelated combo's progress.
     */
    fun observeSequence(combinedEncoding: String, text: String) {
        if (combinedEncoding.length < 2 || text.length < 2) return
        val existing = preferences.getString(combinedEncoding, null)?.split(',')
        val count = if (existing?.getOrNull(0) == text) (existing.getOrNull(1)?.toIntOrNull() ?: 0) + 1 else 1
        preferences.edit().putString(combinedEncoding, "$text,$count").apply()
    }

    /** The learned text for this exact encoding, only once it has crossed the promotion threshold — null otherwise (including "seen once or twice but not yet promoted", which is deliberately invisible to callers). */
    fun promoted(encoding: String): String? {
        val stored = preferences.getString(encoding, null)?.split(',') ?: return null
        val text = stored.getOrNull(0) ?: return null
        val count = stored.getOrNull(1)?.toIntOrNull() ?: 0
        return text.takeIf { count >= PROMOTION_THRESHOLD }
    }

    /** How many combos have crossed the promotion threshold and are actively surfacing — for the settings page's "简拼优选" count (see OpenLessKeyboardSettingsActivity). */
    fun promotedCount(): Int = preferences.all.values.count { value ->
        val count = (value as? String)?.split(',')?.getOrNull(1)?.toIntOrNull() ?: 0
        count >= PROMOTION_THRESHOLD
    }

    /** Every stored combo (promoted or not) as a plain encoding->"text,count" map, for the settings page's export/import feature — see OpenLessSettingsExport. */
    fun exportAll(): Map<String, String> {
        @Suppress("UNCHECKED_CAST")
        return preferences.all as Map<String, String>
    }

    /** Replaces the entire store with [entries] — an import wholesale-restores a backup rather than merging with whatever's already on this device. */
    fun importAll(entries: Map<String, String>) {
        preferences.edit().clear().apply()
        preferences.edit().apply { entries.forEach { (key, value) -> putString(key, value) } }.apply()
    }

    private companion object {
        const val STORE = "openless_pinyin_learned_phrases"
        // Chosen so one coincidental repeat (2 total) doesn't graduate a
        // combo, but a real habit (3 total) does — easy to retune, not
        // derived from any measurement.
        const val PROMOTION_THRESHOLD = 3
    }
}
