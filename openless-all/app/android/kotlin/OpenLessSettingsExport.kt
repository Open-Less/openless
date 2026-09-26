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
    const val VERSION = 1
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
        if (Category.PROVIDER_SELECTION in selected) {
            root.put(
                Category.PROVIDER_SELECTION.key,
                runCatching { JSONObject(OpenLessNative.nativeExportPreferencesSubset()) }.getOrDefault(JSONObject()),
            )
        }
        if (Category.CREDENTIALS in selected) {
            root.put(
                Category.CREDENTIALS.key,
                runCatching { JSONObject(OpenLessNative.nativeExportCredentialsSnapshot()) }.getOrDefault(JSONObject()),
            )
        }
        return root.toString(2)
    }

    /** Which categories are actually present in a previously-exported [json] — for the import dialog's checkbox list, which only ever offers what the file really has. Empty (not a throw) for unparseable input. */
    fun categoriesPresent(json: String): Set<Category> {
        val root = runCatching { JSONObject(json) }.getOrNull() ?: return emptySet()
        return Category.entries.filter { root.has(it.key) }.toSet()
    }

    /** Applies whichever of [selected] are actually present in [json]; anything else in [json] (or missing from [selected]) is left untouched on this device. */
    fun import(context: Context, json: String, selected: Set<Category>) {
        val app = context.applicationContext
        val root = JSONObject(json)
        val prefs = app.getSharedPreferences(PREFS_STORE, Context.MODE_PRIVATE)

        if (Category.CLOUD_NOTES in selected && root.has(Category.CLOUD_NOTES.key)) {
            val cloudNotes = root.getJSONObject(Category.CLOUD_NOTES.key)
            prefs.edit()
                .putString("key_cloud_note_webhook_url", cloudNotes.optString("webhookUrl", ""))
                .putString("key_cloud_note_webhook_token", cloudNotes.optString("webhookToken", ""))
                .apply()
        }
        if (Category.HAPTIC in selected && root.has(Category.HAPTIC.key)) {
            val haptic = root.getJSONObject(Category.HAPTIC.key)
            prefs.edit()
                .putBoolean("key_haptic_enabled", haptic.optBoolean("enabled", true))
                .putInt("key_haptic_amplitude", haptic.optInt("amplitude", 55))
                .putLong("key_haptic_duration_ms", haptic.optLong("durationMs", 12L))
                .apply()
        }
        if (Category.STROKE_FREQUENCY in selected && root.has(Category.STROKE_FREQUENCY.key)) {
            StrokeUserFrequency(app).importAll(root.getJSONObject(Category.STROKE_FREQUENCY.key).toStringMap())
        }
        if (Category.PINYIN_LEARNED_PHRASES in selected && root.has(Category.PINYIN_LEARNED_PHRASES.key)) {
            LitePinyinLearnedPhrases(app).importAll(root.getJSONObject(Category.PINYIN_LEARNED_PHRASES.key).toStringMap())
        }
        if (Category.PROVIDER_SELECTION in selected && root.has(Category.PROVIDER_SELECTION.key)) {
            runCatching {
                OpenLessNative.nativeImportPreferencesSubset(root.getJSONObject(Category.PROVIDER_SELECTION.key).toString())
            }
        }
        if (Category.CREDENTIALS in selected && root.has(Category.CREDENTIALS.key)) {
            runCatching {
                OpenLessNative.nativeImportCredentialsSnapshot(root.getJSONObject(Category.CREDENTIALS.key).toString())
            }
        }
    }

    /** True once import() has (or would have) touched anything that only takes effect after the Rust backend next reads it from disk — i.e. PROVIDER_SELECTION or CREDENTIALS were among [selected] and present in [json]. */
    fun importNeedsAppRestart(json: String, selected: Set<Category>): Boolean {
        val root = runCatching { JSONObject(json) }.getOrNull() ?: return false
        return (Category.PROVIDER_SELECTION in selected && root.has(Category.PROVIDER_SELECTION.key)) ||
            (Category.CREDENTIALS in selected && root.has(Category.CREDENTIALS.key))
    }

    private fun JSONObject.toStringMap(): Map<String, String> {
        val map = mutableMapOf<String, String>()
        val iterator = keys()
        while (iterator.hasNext()) {
            val key = iterator.next()
            map[key] = optString(key, "")
        }
        return map
    }
}
