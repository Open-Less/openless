package com.openless.app

import android.content.Context
import org.json.JSONObject

/**
 * Coordinates the keyboard settings page's "导出/导入配置" feature — collects
 * whichever of the seven categories the user selected into one JSON object
 * for export, and applies whichever categories are both present in an
 * imported file and still checked. Deliberately its own file rather than
 * folded into OpenLessKeyboardSettingsActivity, which is already large and
 * mostly UI-building code — this is a self-contained
 * read-everything/write-everything concern with no View dependencies.
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
        PINYIN_LEARNED_PHRASES("pinyinLearnedPhrases", "简拼优选", "Pinyin combo learning"),
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
            root.put(Category.PINYIN_LEARNED_PHRASES.key, JSONObject(LitePinyinLearnedPhrases(app).exportAll()))
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
                        check(prefs.edit().putBoolean("key_haptic_enabled", enabled).putInt("key_haptic_amplitude", amplitude).putLong("key_haptic_duration_ms", duration).commit()) { "Settings save failed" }
                    }
                    Category.STROKE_FREQUENCY -> StrokeUserFrequency(app).importAll(value.toStringMap())
                    Category.PINYIN_LEARNED_PHRASES -> {
                        require(root.optInt("learningPrivacyVersion", 0) >= ImeLearningPolicy.PRIVACY_VERSION) { "Legacy learned phrases cannot be safely restored" }
                        check(ImeLearningPolicy.migrate(app)) { "Learning data cleanup failed" }
                        LitePinyinLearnedPhrases(app).importAll(value.toStringMap())
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
