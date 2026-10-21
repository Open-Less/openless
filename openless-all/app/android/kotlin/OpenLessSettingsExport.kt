package com.openless.app

import android.content.Context
import org.json.JSONObject

/**
 * Coordinates the native local migration page's legacy JSON compatibility flow — collects
 * whichever of the seven categories the user selected into one JSON object
 * for export, and applies whichever categories are both present in an
 * imported file and still checked. Deliberately its own file rather than
 * folded into an Activity, keeping this self-contained
 * read-everything/write-everything concern free of View dependencies.
 *
 * Plain, unencrypted JSON, by explicit product decision — a Cloud notes
 * token or (if CREDENTIALS is selected) an ASR/LLM API key ends up in the
 * exported file exactly as stored on this device. The settings page's own
 * export button carries a one-line warning about this; this class itself
 * makes no attempt to protect the file's contents.
 */
internal object OpenLessSettingsExport {
    const val VERSION = 2
    private const val PREFS_STORE = "openless_ime_ui"

    /** One category = one row in the export/import checkbox dialog. Order here is the order shown. */
    enum class Category(val key: String, val labelZh: String, val labelEn: String) {
        CLOUD_NOTES("cloudNotes", "云笔记地址 / Token", "Cloud notes URL / token"),
        HAPTIC("haptic", "震动反馈", "Haptic feedback"),
        STROKE_FREQUENCY("strokeFrequency", "笔画调频", "Stroke ranking"),
        PINYIN_LEARNED_PHRASES("pinyinLearnedPhrases", "旧版拼音备份", "Legacy pinyin backup"),
        PROVIDER_SELECTION("providerSelection", "ASR / LLM / 风格包 选择", "ASR / LLM / style pack selection"),
        CREDENTIALS("credentials", "ASR / LLM API Key（明文，敏感）", "ASR / LLM API keys (plaintext, sensitive)"),
    }

    fun export(context: Context, selected: Set<Category>): String {
        val app = context.applicationContext
        val root = JSONObject()
        root.put("openlessSettingsExportVersion", VERSION)
        root.put("learningPrivacyVersion", ImeLearningPolicy.PRIVACY_VERSION)
        check(ImeLearningPolicy.migrate(app)) { "Learning data cleanup failed" }
        root.put(
            "exportedAt",
            java.text.SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ssXXX", java.util.Locale.US).format(java.util.Date()),
        )
        val prefs = app.getSharedPreferences(PREFS_STORE, Context.MODE_PRIVATE)

        if (Category.CLOUD_NOTES in selected) {
            root.put(
                Category.CLOUD_NOTES.key,
                JSONObject().apply {
                    put("webhookUrl", prefs.getString("key_cloud_note_webhook_url", "") ?: "")
                    put("webhookToken", prefs.getString("key_cloud_note_webhook_token", "") ?: "")
                },
            )
        }
        if (Category.HAPTIC in selected) {
            root.put(
                Category.HAPTIC.key,
                JSONObject().apply {
                    put("enabled", prefs.getBoolean("key_haptic_enabled", true))
                    put("amplitude", prefs.getInt("key_haptic_amplitude", 55))
                    put("durationMs", prefs.getLong("key_haptic_duration_ms", 12L))
                },
            )
        }
        if (Category.STROKE_FREQUENCY in selected) {
            root.put(Category.STROKE_FREQUENCY.key, JSONObject(StrokeUserFrequency(app).exportAll()))
        }
        if (Category.PINYIN_LEARNED_PHRASES in selected) {
            root.put(Category.PINYIN_LEARNED_PHRASES.key, JSONObject(LegacyPinyinBackup(app).exportAll()))
        }
        if (Category.PROVIDER_SELECTION in selected || Category.CREDENTIALS in selected) {
            val response = JSONObject(OpenLessNative.nativeExportProviderSettings(Category.CREDENTIALS in selected))
            check(response.optBoolean("ok")) { response.optString("error", "Provider export failed") }
            val payload = response.getJSONObject("payload")
            if (Category.PROVIDER_SELECTION in selected) root.put(Category.PROVIDER_SELECTION.key, payload.getJSONObject("providerSelection"))
            if (Category.CREDENTIALS in selected) root.put(Category.CREDENTIALS.key, payload.getJSONObject("credentials"))
        }
        return root.toString(2)
    }

    /** Portable Android-only state that is not represented by Core repositories. */
    fun exportPortable(context: Context): String {
        val app = context.applicationContext
        val prefs = app.getSharedPreferences(PREFS_STORE, Context.MODE_PRIVATE)
        return JSONObject().apply {
            put("schemaVersion", 1)
            put("learningPrivacyVersion", ImeLearningPolicy.PRIVACY_VERSION)
            put("preferences", JSONObject().apply {
                prefs.getString("locale", null)?.let { put("locale", OpenLessMigrationBundlePolicy.canonicalLocale(it)) }
                prefs.getString("theme_mode", null)?.let { put("themeMode", it) }
                put("hapticEnabled", prefs.getBoolean("key_haptic_enabled", true))
                put("hapticAmplitude", prefs.getInt("key_haptic_amplitude", 55))
                put("hapticDurationMs", prefs.getLong("key_haptic_duration_ms", 12L))
                put("englishSuggestionsEnabled", prefs.getBoolean("english_suggestions_enabled", true))
                put("debugFeaturesEnabled", prefs.getBoolean("key_debug_features_enabled", false))
            })
            put("strokeFrequency", JSONObject(StrokeUserFrequency(app).exportAll()))
            put("pinyinLearnedPhrases", JSONObject(LegacyPinyinBackup(app).exportAll()))
        }.toString()
    }

    fun validatePortable(json: String): Boolean = runCatching { parsePortable(json) }.isSuccess

    internal fun replacePortable(context: Context, json: String, beforeWrite: (String) -> Unit = {}) {
        val app = context.applicationContext
        val root = parsePortable(json)
        val prefs = app.getSharedPreferences(PREFS_STORE, Context.MODE_PRIVATE)
        val values = root.getJSONObject("preferences")
        val editor = prefs.edit()
        for (key in listOf("locale", "theme_mode", "key_haptic_enabled", "key_haptic_amplitude", "key_haptic_duration_ms", "english_suggestions_enabled", "key_debug_features_enabled")) editor.remove(key)
        if (values.has("locale")) editor.putString("locale", values.getString("locale"))
        if (values.has("themeMode")) editor.putString("theme_mode", values.getString("themeMode"))
        if (values.has("hapticEnabled")) editor.putBoolean("key_haptic_enabled", values.getBoolean("hapticEnabled"))
        if (values.has("hapticAmplitude")) editor.putInt("key_haptic_amplitude", values.getInt("hapticAmplitude"))
        if (values.has("hapticDurationMs")) editor.putLong("key_haptic_duration_ms", values.getLong("hapticDurationMs"))
        if (values.has("englishSuggestionsEnabled")) editor.putBoolean("english_suggestions_enabled", values.getBoolean("englishSuggestionsEnabled"))
        if (values.has("debugFeaturesEnabled")) editor.putBoolean("key_debug_features_enabled", values.getBoolean("debugFeaturesEnabled"))
        beforeWrite("preferences")
        check(editor.commit()) { "Portable settings save failed" }
        beforeWrite("strokeFrequency")
        StrokeUserFrequency(app).importAll(root.getJSONObject("strokeFrequency").toStringMap())
        beforeWrite("pinyinLearnedPhrases")
        LegacyPinyinBackup(app).importAll(root.getJSONObject("pinyinLearnedPhrases").toStringMap())
    }

    private fun parsePortable(json: String): JSONObject {
        require(json.toByteArray(Charsets.UTF_8).size <= 4 * 1024 * 1024)
        val root = JSONObject(json)
        require(root.opt("schemaVersion") == 1 && root.opt("learningPrivacyVersion") == ImeLearningPolicy.PRIVACY_VERSION) { "Unsupported portable settings version" }
        val allowed = setOf("schemaVersion", "learningPrivacyVersion", "preferences", "strokeFrequency", "pinyinLearnedPhrases")
        val keys = root.keys()
        while (keys.hasNext()) require(keys.next() in allowed) { "Unknown portable settings field" }
        val values = root.getJSONObject("preferences")
        val allowedValues = setOf("locale", "themeMode", "hapticEnabled", "hapticAmplitude", "hapticDurationMs", "englishSuggestionsEnabled", "debugFeaturesEnabled")
        val valueKeys = values.keys()
        while (valueKeys.hasNext()) require(valueKeys.next() in allowedValues) { "Unknown portable preference" }
        for (key in listOf("locale", "themeMode")) if (values.has(key)) require(values.get(key) is String)
        for (key in listOf("hapticEnabled", "englishSuggestionsEnabled", "debugFeaturesEnabled")) if (values.has(key)) require(values.get(key) is Boolean)
        for (key in listOf("hapticAmplitude", "hapticDurationMs")) if (values.has(key)) require(values.get(key) is Int || values.get(key) is Long)
        if (values.has("locale")) require(values.getString("locale") in setOf("system", "zh-CN", "zh-TW", "en", "ja", "ko", "es", "fr", "de"))
        if (values.has("themeMode")) require(values.getString("themeMode") in setOf("system", "light", "dark"))
        if (values.has("hapticAmplitude")) require(values.getInt("hapticAmplitude") in 1..255)
        if (values.has("hapticDurationMs")) require(values.getLong("hapticDurationMs") in 1L..1000L)
        require(root.getJSONObject("strokeFrequency").length() <= 20000)
        require(root.getJSONObject("pinyinLearnedPhrases").length() <= 20000)
        for (key in listOf("strokeFrequency", "pinyinLearnedPhrases")) {
            val entries = root.getJSONObject(key).toStringMap()
            require(entries.keys.all { it.isNotEmpty() && it.toByteArray(Charsets.UTF_8).size <= 256 })
        }
        return root
    }

    /** Which categories are actually present in a previously-exported [json] — for the import dialog's checkbox list, which only ever offers what the file really has. Empty (not a throw) for unparseable input. */
    fun categoriesPresent(json: String): Set<Category> {
        val root = runCatching { JSONObject(json) }.getOrNull() ?: return emptySet()
        if (root.optInt("openlessSettingsExportVersion", 0) !in 1..VERSION) return emptySet()
        return Category.entries.filter { root.opt(it.key) is JSONObject }.toSet()
    }

    data class ImportResult(val applied: Set<Category>, val errors: Map<Category, String>)

    fun import(context: Context, json: String, selected: Set<Category>): ImportResult {
        val app = context.applicationContext
        val root = JSONObject(json)
        require(root.getInt("openlessSettingsExportVersion") in 1..VERSION) { "Unsupported settings version" }
        val prefs = app.getSharedPreferences(PREFS_STORE, Context.MODE_PRIVATE)
        val applied = mutableSetOf<Category>()
        val errors = mutableMapOf<Category, String>()
        val requested = selected.filter { root.has(it.key) }.toSet()
        // Validate the shape of every requested category before any writes.
        requested.forEach { root.getJSONObject(it.key) }
        for (category in requested - setOf(Category.PROVIDER_SELECTION, Category.CREDENTIALS)) {
            runCatching {
                val value = root.getJSONObject(category.key)
                when (category) {
                    Category.CLOUD_NOTES -> {
                        val url = value.getString("webhookUrl")
                        val token = value.getString("webhookToken")
                        require(url.isBlank() || java.net.URI(url).scheme in listOf("http", "https")) { "Invalid webhook URL" }
                        check(prefs.edit().putString("key_cloud_note_webhook_url", url).putString("key_cloud_note_webhook_token", token).commit()) { "Settings save failed" }
                    }
                    Category.HAPTIC -> {
                        val enabled = value.getBoolean("enabled")
                        val amplitude = value.getInt("amplitude")
                        val duration = value.getLong("durationMs")
                        require(amplitude in 1..255 && duration in 1..1000) { "Invalid haptic settings" }
                        check(OpenLessMigrationState.settings { check(prefs.edit().putBoolean("key_haptic_enabled", enabled).putInt("key_haptic_amplitude", amplitude).putLong("key_haptic_duration_ms", duration).commit()) }) { "migration_android_busy" }
                    }
                    Category.STROKE_FREQUENCY -> StrokeUserFrequency(app).importAll(value.toStringMap())
                    Category.PINYIN_LEARNED_PHRASES -> {
                        require(root.optInt("learningPrivacyVersion", 0) >= ImeLearningPolicy.PRIVACY_VERSION) { "Legacy learned phrases cannot be safely restored" }
                        check(ImeLearningPolicy.migrate(app)) { "Learning data cleanup failed" }
                        LegacyPinyinBackup(app).importAll(value.toStringMap())
                    }
                    else -> Unit
                }
                applied.add(category)
            }.onFailure { errors[category] = it.message ?: "Import failed" }
        }
        val providerCategories = requested intersect setOf(Category.PROVIDER_SELECTION, Category.CREDENTIALS)
        if (providerCategories.isNotEmpty()) {
            runCatching {
                val response = JSONObject(OpenLessNative.nativeImportProviderSettings(json,
                    Category.PROVIDER_SELECTION in requested, Category.CREDENTIALS in requested))
                val successful = response.getJSONArray("applied")
                for (i in 0 until successful.length()) Category.entries.find { it.key == successful.getString(i) }?.let { applied.add(it) }
                val failed = response.getJSONObject("errors")
                for (category in providerCategories) if (failed.has(category.key)) errors[category] = failed.getString(category.key)
            }.onFailure { for (category in providerCategories) errors[category] = "Provider import failed" }
        }
        return ImportResult(applied, errors)
    }

    private fun JSONObject.toStringMap(): Map<String, String> {
        val map = mutableMapOf<String, String>()
        val iterator = keys()
        while (iterator.hasNext()) {
            val key = iterator.next()
            val value = get(key)
            require(value is String) { "Invalid learning entry" }
            map[key] = value
        }
        return map
    }
}
