package com.openless.app

import android.content.SharedPreferences

/**
 * Optional fuzzy-pinyin pairs for the Rime pinyin schemas. Each group is a
 * pair of rime-ice's own (commented-out) spelling rules, appended to
 * `speller/algebra` through a `.custom.yaml` patch in the user data dir —
 * both pinyin schemas get the same patch, since they share one compiled
 * spelling index.
 */
internal object RimeFuzzyPinyin {
    data class Group(val id: String, val labelZh: String, val labelEn: String, val rules: List<String>)

    val groups = listOf(
        Group("zcs", "z c s = zh ch sh", "z c s = zh ch sh", listOf("derive/^([zcs])h/\$1/", "derive/^([zcs])([^h])/\$1h\$2/")),
        Group("nl", "n = l", "n = l", listOf("derive/^n/l/", "derive/^l/n/")),
        Group("fh", "f = h", "f = h", listOf("derive/^f/h/", "derive/^h/f/")),
        Group("rl", "r = l", "r = l", listOf("derive/^r/l/", "derive/^l/r/")),
        // an$ also covers ian/uan, so this pair includes ian = iang and uan = uang.
        Group("ang", "an = ang（含 ian、uan）", "an = ang (incl. ian, uan)", listOf("derive/ang\$/an/", "derive/an\$/ang/")),
        Group("eng", "en = eng", "en = eng", listOf("derive/eng\$/en/", "derive/en\$/eng/")),
        Group("ing", "in = ing", "in = ing", listOf("derive/ing\$/in/", "derive/in\$/ing/")),
    )

    private const val KEY = "pinyin_fuzzy_groups"

    fun selected(prefs: SharedPreferences): Set<String> {
        val known = groups.map { it.id }.toSet()
        return prefs.getString(KEY, "").orEmpty().split(',').filter { it in known }.toSet()
    }

    fun save(prefs: SharedPreferences, ids: Set<String>) {
        prefs.edit().putString(KEY, groups.map { it.id }.filter { it in ids }.joinToString(",")).apply()
    }

    /** The schema patch for [ids], or null when no group is on (the patch file is then removed). */
    fun customYaml(ids: Set<String>): String? {
        val rules = groups.filter { it.id in ids }.flatMap { it.rules }
        if (rules.isEmpty()) return null
        return buildString {
            append("# Written by OpenLess keyboard settings (fuzzy pinyin); changes here are overwritten.\n")
            append("patch:\n")
            append("  \"speller/algebra/+\":\n")
            rules.forEach { append("    - ").append(it).append('\n') }
        }
    }
}
