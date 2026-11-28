package com.openless.app

import android.content.Context
import android.content.SharedPreferences
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * The keyboard settings page's own export/import, in one JSON file: that
 * page's preferences (keyboard size, haptics and the other switches), the
 * Rime pinyin user dictionary, the stroke and association picks, and the
 * cloud notes webhook (its token in plaintext). App
 * settings are not part of it; they travel through cloud sync and the
 * migration bundle. Importing merges the learning data into what this
 * device already learned and never deletes it.
 */
internal object KeyboardDataFile {
    const val VERSION = 1
    // The pinyin snapshot alone may reach RimeUserDictionaryFile.MAX_BYTES.
    const val MAX_BYTES = 48 * 1024 * 1024
    private const val MAX_PICKS = 50_000
    private const val PREFS_STORE = "openless_ime_ui"

    enum class Section(val key: String, val labelZh: String, val labelEn: String) {
        SETTINGS("keyboardSettings", "键盘设置（高度、震动、开关）", "Keyboard settings (size, haptics, switches)"),
        PINYIN("pinyinUserDictionary", "拼音用户词典", "Pinyin user dictionary"),
        STROKE("strokeCodes", "笔画调频", "Stroke ranking"),
        ASSOCIATION("associations", "联想学习", "Association learning"),
        CLOUD_NOTES("cloudNotes", "云笔记地址 / Token（明文）", "Cloud notes URL / token (plaintext)"),
    }

    /** An export starts with everything checked, the plaintext token included (the checklist marks it). */
    val defaultExport: Set<Section> = Section.entries.toSet()

    /** An import starts without the token, so another file never replaces this device's by accident. */
    val defaultImport: Set<Section> = Section.entries.toSet() - Section.CLOUD_NOTES

    /** One preference of the keyboard settings page: its file key, preference key and accepted values. */
    private class Setting(val key: String, val pref: String, val accepts: (Any) -> Boolean, val read: (SharedPreferences) -> Any)

    private fun bool(key: String, pref: String, default: Boolean) =
        Setting(key, pref, { it is Boolean }) { it.getBoolean(pref, default) }

    private fun int(key: String, pref: String, range: IntRange, default: Int) =
        Setting(key, pref, { it is Int && it in range }) { it.getInt(pref, default).coerceIn(range) }

    private val settings = listOf(
        int("keyboardHeightDp", OpenLessImeService.PREF_KEYBOARD_HEIGHT_DP,
            OpenLessImeService.MIN_KEYBOARD_HEIGHT_DP..OpenLessImeService.MAX_KEYBOARD_HEIGHT_DP, OpenLessImeService.DEFAULT_KEYBOARD_HEIGHT_DP),
        int("keyboardRaiseDp", OpenLessImeService.PREF_KEYBOARD_RAISE_DP,
            OpenLessImeService.MIN_KEYBOARD_RAISE_DP..OpenLessImeService.MAX_KEYBOARD_RAISE_DP, OpenLessImeService.DEFAULT_KEYBOARD_RAISE_DP),
        bool("hapticEnabled", "key_haptic_enabled", true),
        int("hapticAmplitude", "key_haptic_amplitude", 1..255, 55),
        // Stored as a Long; the page's slider keeps it at 1..125.
        Setting("hapticDurationMs", "key_haptic_duration_ms", { it is Int && it in 1..1000 }) {
            it.getLong("key_haptic_duration_ms", 12L).coerceIn(1L, 1000L).toInt()
        },
        bool("debugFeaturesEnabled", "key_debug_features_enabled", false),
        bool("cloudPinyinEnabled", "cloud_pinyin_enabled", false),
        bool("publicDictionaryUpdates", "public_dictionary_updates", true),
        bool("englishSuggestionsEnabled", "english_suggestions_enabled", true),
        Setting("fuzzyPinyin", "pinyin_fuzzy_groups", { value ->
            value is List<*> && value.all { id -> RimeFuzzyPinyin.groups.any { it.id == id } }
        }) { RimeFuzzyPinyin.selected(it).toList() },
    )

    /** Whether [value] (a JSON value with arrays as lists) is acceptable for the keyboard setting [key]. */
    fun acceptsSetting(key: String, value: Any): Boolean = settings.firstOrNull { it.key == key }?.accepts?.invoke(value) == true

    /** Whether a cloud notes webhook address may be imported: blank, or http(s). */
    fun acceptsWebhookUrl(url: String): Boolean =
        url.isBlank() || runCatching { java.net.URI(url).scheme in listOf("http", "https") }.getOrDefault(false)

    class Contents(
        val settings: Map<String, Any>,
        val pinyinSnapshot: String?,
        val strokeCodes: Map<String, String>,
        val associations: Map<String, String>,
        val cloudNotes: Pair<String, String>?,
    ) {
        private val pinyinCount = pinyinSnapshot?.let { RimeUserDictionaryFile.entryCount(it) } ?: 0

        fun count(section: Section): Int = when (section) {
            Section.SETTINGS -> settings.size
            Section.PINYIN -> pinyinCount
            Section.STROKE -> strokeCodes.size
            Section.ASSOCIATION -> associations.size
            Section.CLOUD_NOTES -> if (cloudNotes == null) 0 else 1
        }

        /** Sections that hold something to import. */
        fun present(): List<Section> = Section.entries.filter { count(it) > 0 }
    }

    /** The chosen [sections] as a file. Off the main thread: reading the pinyin dictionary closes the open Rime sessions. */
    fun export(context: Context, sections: Set<Section>): String {
        val app = context.applicationContext
        val prefs = app.getSharedPreferences(PREFS_STORE, Context.MODE_PRIVATE)
        val pinyin = if (Section.PINYIN !in sections) null else {
            val scratch = File(app.cacheDir, "export-pinyin_simp.userdb.txt")
            try {
                if (OpenLessRime.exportUserDictionary(app, scratch)) scratch.readText() else null
            } finally { scratch.delete() }
        }
        return JSONObject().apply {
            put("openlessKeyboardDataVersion", VERSION)
            put(
                "exportedAt",
                java.text.SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ssXXX", java.util.Locale.US).format(java.util.Date()),
            )
            if (Section.SETTINGS in sections) {
                put(Section.SETTINGS.key, JSONObject().apply {
                    for (setting in settings) {
                        val value = setting.read(prefs)
                        put(setting.key, if (value is List<*>) JSONArray(value) else value)
                    }
                })
            }
            pinyin?.let { put(Section.PINYIN.key, it) }
            if (Section.STROKE in sections) put(Section.STROKE.key, JSONObject(StrokeUserFrequency.strokeCodes(app).exportAll()))
            if (Section.ASSOCIATION in sections) put(Section.ASSOCIATION.key, JSONObject(StrokeUserFrequency.associations(app).exportAll()))
            if (Section.CLOUD_NOTES in sections) {
                put(Section.CLOUD_NOTES.key, JSONObject().apply {
                    put("webhookUrl", prefs.getString("key_cloud_note_webhook_url", "") ?: "")
                    put("webhookToken", prefs.getString("key_cloud_note_webhook_token", "") ?: "")
                })
            }
        }.toString()
    }

    /** Parses and checks every section; throws IllegalArgumentException for anything that is not a valid keyboard data file. */
    fun read(json: String): Contents {
        val root = runCatching { JSONObject(json) }.getOrElse { throw IllegalArgumentException("Not JSON") }
        require(root.optInt("openlessKeyboardDataVersion", 0) == VERSION) { "Unsupported keyboard data version" }
        val pinyin = root.opt(Section.PINYIN.key)?.let { value ->
            require(value is String) { "Invalid pinyin dictionary" }
            value.also { RimeUserDictionaryFile.entryCount(it) }
        }
        val cloudNotes = root.opt(Section.CLOUD_NOTES.key)?.let { value ->
            require(value is JSONObject) { "Invalid cloud notes" }
            val url = value.optString("webhookUrl", "")
            require(acceptsWebhookUrl(url)) { "Invalid webhook URL" }
            url to value.optString("webhookToken", "")
        }
        return Contents(
            readSettings(root),
            pinyin,
            picks(root, Section.STROKE, strokeKeys = true),
            picks(root, Section.ASSOCIATION, strokeKeys = false),
            cloudNotes,
        )
    }

    private fun readSettings(root: JSONObject): Map<String, Any> {
        val value = root.opt(Section.SETTINGS.key) ?: return emptyMap()
        require(value is JSONObject) { "Invalid keyboard settings" }
        val result = mutableMapOf<String, Any>()
        for (key in value.keys()) {
            val raw = value.get(key)
            val item: Any = if (raw is JSONArray) (0 until raw.length()).map { raw.get(it) } else raw
            require(acceptsSetting(key, item)) { "Invalid keyboard setting $key" }
            result[key] = item
        }
        return result
    }

    private fun picks(root: JSONObject, section: Section, strokeKeys: Boolean): Map<String, String> {
        val value = root.opt(section.key) ?: return emptyMap()
        require(value is JSONObject && value.length() <= MAX_PICKS) { "Invalid ${section.key}" }
        val entries = mutableMapOf<String, String>()
        for (key in value.keys()) {
            val stamp = value.get(key)
            require(
                stamp is String && StrokeUserFrequency.isValidEntry(key, stamp) &&
                    StrokeUserFrequency.isStrokeCodeKey(key) == strokeKeys,
            ) { "Invalid ${section.key} entry" }
            entries[key] = stamp
        }
        return entries
    }

    /** Applies the chosen [sections] of [contents]: settings and cloud notes are set, learning data is merged. Off the main thread. */
    fun import(context: Context, contents: Contents, sections: Set<Section>) {
        val app = context.applicationContext
        val prefs = app.getSharedPreferences(PREFS_STORE, Context.MODE_PRIVATE)
        if (Section.SETTINGS in sections && contents.settings.isNotEmpty()) {
            val fuzzyBefore = RimeFuzzyPinyin.selected(prefs)
            // Haptics, English suggestions and debug features are also in the
            // migration bundle, so the write goes through its fence.
            check(OpenLessMigrationState.settings {
                val editor = prefs.edit()
                for ((key, value) in contents.settings) {
                    val setting = settings.first { it.key == key }
                    when {
                        key == "fuzzyPinyin" -> RimeFuzzyPinyin.save(prefs, (value as List<*>).map { it as String }.toSet())
                        key == "hapticDurationMs" -> editor.putLong(setting.pref, (value as Int).toLong())
                        value is Boolean -> editor.putBoolean(setting.pref, value)
                        value is Int -> editor.putInt(setting.pref, value)
                    }
                }
                check(editor.commit()) { "Keyboard settings save failed" }
            }) { "migration_android_busy" }
            val fuzzyAfter = RimeFuzzyPinyin.selected(prefs)
            if (fuzzyAfter != fuzzyBefore) OpenLessRime.applyFuzzyPinyin(app, fuzzyAfter)
        }
        if (Section.PINYIN in sections && contents.pinyinSnapshot != null) {
            // Rime parses a snapshot as text only when its name ends in .userdb.txt.
            val scratch = File(app.cacheDir, "import-pinyin_simp.userdb.txt")
            try {
                scratch.writeText(contents.pinyinSnapshot)
                OpenLessRime.importUserDictionary(app, scratch)
            } finally { scratch.delete() }
        }
        if (Section.STROKE in sections) StrokeUserFrequency.strokeCodes(app).mergeAll(contents.strokeCodes)
        if (Section.ASSOCIATION in sections) StrokeUserFrequency.associations(app).mergeAll(contents.associations)
        if (Section.CLOUD_NOTES in sections) {
            contents.cloudNotes?.let { (url, token) ->
                check(prefs.edit().putString("key_cloud_note_webhook_url", url).putString("key_cloud_note_webhook_token", token).commit()) {
                    "Cloud notes save failed"
                }
            }
        }
    }
}
