package com.openless.android;

import android.content.Context;
import android.content.SharedPreferences;

import org.json.JSONArray;

import java.util.ArrayList;
import java.util.List;

final class SettingsStore {
    private static final String PREFS = "openless_settings";

    private final SharedPreferences prefs;
    private final SecureValueStore secure;

    SettingsStore(Context context) {
        prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        secure = new SecureValueStore(context);
    }

    Settings get() {
        Settings settings = new Settings();
        settings.activeAsrProvider = prefs.getString("activeAsrProvider", "volcengine");
        settings.activeLlmProvider = prefs.getString("activeLlmProvider", "ark");
        settings.asrBaseUrl = prefs.getString("asrBaseUrl", "https://api.openai.com/v1");
        settings.asrApiKey = secure.get("asrApiKey", prefs.getString("asrApiKey", ""));
        settings.asrModel = prefs.getString("asrModel", "whisper-1");
        settings.volcengineAppKey = secure.get("volcengineAppKey", prefs.getString("volcengineAppKey", ""));
        settings.volcengineAccessKey = secure.get("volcengineAccessKey", prefs.getString("volcengineAccessKey", ""));
        settings.volcengineResourceId = prefs.getString("volcengineResourceId", VolcengineAsrProvider.DEFAULT_RESOURCE_ID);
        settings.llmBaseUrl = prefs.getString("llmBaseUrl", "https://ark.cn-beijing.volces.com/api/v3");
        settings.llmApiKey = secure.get("llmApiKey", prefs.getString("llmApiKey", ""));
        settings.llmModel = prefs.getString("llmModel", "");
        settings.mode = PolishMode.fromId(prefs.getString("mode", PolishMode.LIGHT.id));
        settings.enabledModes = parseModes(prefs.getString("enabledModes", ""));
        settings.showCapsule = prefs.getBoolean("showCapsule", true);
        settings.allowClipboardFallback = prefs.getBoolean("allowClipboardFallback", true);
        settings.workingLanguages = parseStrings(prefs.getString("workingLanguages", "[\"简体中文\"]"));
        settings.translationTargetLanguage = prefs.getString("translationTargetLanguage", "");
        settings.qaSaveHistory = prefs.getBoolean("qaSaveHistory", false);
        return settings;
    }

    void save(Settings settings) {
        prefs.edit()
                .putString("activeAsrProvider", settings.activeAsrProvider)
                .putString("activeLlmProvider", settings.activeLlmProvider)
                .putString("asrBaseUrl", settings.asrBaseUrl)
                .putString("asrModel", settings.asrModel)
                .putString("volcengineResourceId", settings.volcengineResourceId)
                .putString("llmBaseUrl", settings.llmBaseUrl)
                .putString("llmModel", settings.llmModel)
                .putString("mode", settings.mode.id)
                .putString("enabledModes", modesToJson(settings.enabledModes))
                .putBoolean("showCapsule", settings.showCapsule)
                .putBoolean("allowClipboardFallback", settings.allowClipboardFallback)
                .putString("workingLanguages", stringsToJson(settings.workingLanguages))
                .putString("translationTargetLanguage", settings.translationTargetLanguage)
                .putBoolean("qaSaveHistory", settings.qaSaveHistory)
                .apply();
        secure.put("asrApiKey", settings.asrApiKey);
        secure.put("volcengineAppKey", settings.volcengineAppKey);
        secure.put("volcengineAccessKey", settings.volcengineAccessKey);
        secure.put("llmApiKey", settings.llmApiKey);
        prefs.edit()
                .remove("asrApiKey")
                .remove("volcengineAppKey")
                .remove("volcengineAccessKey")
                .remove("llmApiKey")
                .apply();
    }

    private List<PolishMode> parseModes(String raw) {
        List<PolishMode> out = new ArrayList<>();
        try {
            JSONArray json = new JSONArray(raw == null || raw.isEmpty() ? "[]" : raw);
            for (int i = 0; i < json.length(); i++) {
                PolishMode mode = PolishMode.fromId(json.optString(i));
                if (!out.contains(mode)) {
                    out.add(mode);
                }
            }
        } catch (Exception ignored) {
        }
        if (out.isEmpty()) {
            for (PolishMode mode : PolishMode.values()) {
                out.add(mode);
            }
        }
        return out;
    }

    private List<String> parseStrings(String raw) {
        List<String> out = new ArrayList<>();
        try {
            JSONArray json = new JSONArray(raw == null || raw.isEmpty() ? "[]" : raw);
            for (int i = 0; i < json.length(); i++) {
                String value = json.optString(i).trim();
                if (!value.isEmpty()) {
                    out.add(value);
                }
            }
        } catch (Exception ignored) {
        }
        if (out.isEmpty()) {
            out.add("简体中文");
        }
        return out;
    }

    private String modesToJson(List<PolishMode> modes) {
        JSONArray json = new JSONArray();
        if (modes != null) {
            for (PolishMode mode : modes) {
                json.put(mode.id);
            }
        }
        return json.toString();
    }

    private String stringsToJson(List<String> values) {
        JSONArray json = new JSONArray();
        if (values != null) {
            for (String value : values) {
                String normalized = value == null ? "" : value.trim();
                if (!normalized.isEmpty()) {
                    json.put(normalized);
                }
            }
        }
        return json.toString();
    }

    static final class Settings {
        String activeAsrProvider;
        String activeLlmProvider;
        String asrBaseUrl;
        String asrApiKey;
        String asrModel;
        String volcengineAppKey;
        String volcengineAccessKey;
        String volcengineResourceId;
        String llmBaseUrl;
        String llmApiKey;
        String llmModel;
        PolishMode mode;
        List<PolishMode> enabledModes;
        boolean showCapsule;
        boolean allowClipboardFallback;
        List<String> workingLanguages;
        String translationTargetLanguage;
        boolean qaSaveHistory;
    }
}
