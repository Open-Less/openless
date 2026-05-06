package com.openless.android;

import android.content.Context;
import android.content.SharedPreferences;

import org.json.JSONArray;
import org.json.JSONObject;

import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Date;
import java.util.List;
import java.util.Locale;
import java.util.TimeZone;
import java.util.UUID;

final class HistoryStore {
    private static final String PREFS = "openless_history";
    private static final String KEY = "items";

    private final SharedPreferences prefs;

    HistoryStore(Context context) {
        prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    void add(String raw, String text, PolishMode mode, long durationMs) {
        add(raw, text, mode, null, null, InsertStatus.COPIED_FALLBACK, null, durationMs, null);
    }

    void add(String raw, String text, PolishMode mode, InsertStatus insertStatus, String errorCode, long durationMs, Integer dictionaryEntryCount) {
        add(raw, text, mode, null, null, insertStatus, errorCode, durationMs, dictionaryEntryCount);
    }

    void add(String raw, String text, PolishMode mode, String appBundleId, String appName,
             InsertStatus insertStatus, String errorCode, long durationMs, Integer dictionaryEntryCount) {
        try {
            JSONArray items = new JSONArray(prefs.getString(KEY, "[]"));
            JSONObject item = new JSONObject();
            item.put("id", UUID.randomUUID().toString());
            item.put("createdAt", isoNow());
            item.put("rawTranscript", raw == null ? "" : raw);
            item.put("finalText", text == null ? "" : text);
            item.put("mode", mode.id);
            item.put("appBundleId", appBundleId == null ? JSONObject.NULL : appBundleId);
            item.put("appName", appName == null ? JSONObject.NULL : appName);
            item.put("insertStatus", insertStatus.id);
            item.put("errorCode", errorCode == null ? JSONObject.NULL : errorCode);
            item.put("durationMs", durationMs > 0 ? durationMs : JSONObject.NULL);
            item.put("dictionaryEntryCount", dictionaryEntryCount == null ? JSONObject.NULL : dictionaryEntryCount);
            JSONArray next = new JSONArray();
            next.put(item);
            for (int i = 0; i < Math.min(items.length(), 199); i++) {
                next.put(items.getJSONObject(i));
            }
            prefs.edit().putString(KEY, next.toString()).apply();
        } catch (Exception ignored) {
        }
    }

    void addFailure(String raw, PolishMode mode, String errorCode, long durationMs, Integer dictionaryEntryCount) {
        add(raw, "", mode, null, null, InsertStatus.FAILED, errorCode, durationMs, dictionaryEntryCount);
    }

    List<Item> list() {
        List<Item> out = new ArrayList<>();
        try {
            JSONArray items = new JSONArray(prefs.getString(KEY, "[]"));
            for (int i = 0; i < items.length(); i++) {
                JSONObject json = items.getJSONObject(i);
                out.add(new Item(
                        json.optString("id"),
                        json.optString("createdAt"),
                        firstNonEmpty(json.optString("rawTranscript"), json.optString("raw")),
                        firstNonEmpty(json.optString("finalText"), json.optString("text")),
                        PolishMode.fromId(json.optString("mode")),
                        nullIfEmpty(json.optString("appBundleId")),
                        nullIfEmpty(json.optString("appName")),
                        InsertStatus.fromId(json.optString("insertStatus")),
                        nullIfEmpty(json.optString("errorCode")),
                        json.optLong("durationMs"),
                        json.has("dictionaryEntryCount") && !json.isNull("dictionaryEntryCount")
                                ? json.optInt("dictionaryEntryCount")
                                : null));
            }
        } catch (Exception ignored) {
        }
        return out;
    }

    void delete(String id) {
        if (id == null || id.isEmpty()) {
            return;
        }
        try {
            JSONArray items = new JSONArray(prefs.getString(KEY, "[]"));
            JSONArray next = new JSONArray();
            for (int i = 0; i < items.length(); i++) {
                JSONObject json = items.getJSONObject(i);
                if (!id.equals(json.optString("id"))) {
                    next.put(json);
                }
            }
            prefs.edit().putString(KEY, next.toString()).apply();
        } catch (Exception ignored) {
        }
    }

    void clear() {
        prefs.edit().putString(KEY, "[]").apply();
    }

    private static String isoNow() {
        SimpleDateFormat format = new SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'", Locale.US);
        format.setTimeZone(TimeZone.getTimeZone("UTC"));
        return format.format(new Date());
    }

    private static String firstNonEmpty(String a, String b) {
        return a == null || a.isEmpty() ? b : a;
    }

    private static String nullIfEmpty(String value) {
        return value == null || value.isEmpty() || "null".equals(value) ? null : value;
    }

    static final class Item {
        final String id;
        final String createdAt;
        final String raw;
        final String text;
        final PolishMode mode;
        final String appBundleId;
        final String appName;
        final InsertStatus insertStatus;
        final String errorCode;
        final long durationMs;
        final Integer dictionaryEntryCount;

        Item(String id, String createdAt, String raw, String text, PolishMode mode, String appBundleId,
             String appName, InsertStatus insertStatus, String errorCode, long durationMs, Integer dictionaryEntryCount) {
            this.id = id;
            this.createdAt = createdAt;
            this.raw = raw;
            this.text = text;
            this.mode = mode;
            this.appBundleId = appBundleId;
            this.appName = appName;
            this.insertStatus = insertStatus;
            this.errorCode = errorCode;
            this.durationMs = durationMs;
            this.dictionaryEntryCount = dictionaryEntryCount;
        }
    }
}
