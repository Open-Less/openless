package com.openless.app

import android.content.Context

/** Old backup-format compatibility only; never queried by the Rime engine. */
internal class LegacyPinyinBackup(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        "openless_pinyin_learned_phrases", Context.MODE_PRIVATE,
    )

    fun entryCount(): Int = exportAll().size

    fun exportAll(): Map<String, String> = preferences.all.mapNotNull { (key, value) ->
        (value as? String)?.let { key to it }
    }.toMap()

    fun importAll(entries: Map<String, String>) {
        check(OpenLessMigrationState.settings {
            check(preferences.edit().clear().apply {
                entries.forEach { (key, value) -> putString(key, value) }
            }.commit()) { "Legacy pinyin backup save failed" }
        }) { "migration_android_busy" }
    }
}
