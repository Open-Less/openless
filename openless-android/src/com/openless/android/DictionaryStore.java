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

final class DictionaryStore {
    private static final String PREFS = "openless_dictionary";
    private static final String KEY = "items";

    private final SharedPreferences prefs;

    DictionaryStore(Context context) {
        prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    List<Entry> list() {
        List<Entry> out = new ArrayList<>();
        try {
            JSONArray items = new JSONArray(prefs.getString(KEY, "[]"));
            for (int i = 0; i < items.length(); i++) {
                JSONObject json = items.getJSONObject(i);
                String phrase = json.optString("phrase", "").trim();
                if (phrase.isEmpty()) {
                    continue;
                }
                out.add(new Entry(
                        firstNonEmpty(json.optString("id"), UUID.randomUUID().toString()),
                        phrase,
                        json.optString("note", json.optString("notes", "")).trim(),
                        json.optBoolean("enabled", true),
                        json.optLong("hits", json.optLong("hitCount", 0)),
                        firstNonEmpty(json.optString("createdAt"), json.optString("created_at"))));
            }
        } catch (Exception ignored) {
        }
        return out;
    }

    List<Entry> enabledEntries() {
        List<Entry> out = new ArrayList<>();
        for (Entry entry : list()) {
            if (entry.enabled && !entry.phrase.isEmpty()) {
                out.add(entry);
            }
        }
        return out;
    }

    List<String> enabledPhrases() {
        List<String> out = new ArrayList<>();
        for (Entry entry : enabledEntries()) {
            out.add(entry.phrase);
        }
        return out;
    }

    String exportPlainText() {
        StringBuilder builder = new StringBuilder();
        for (Entry entry : list()) {
            if (builder.length() > 0) {
                builder.append('\n');
            }
            builder.append(entry.phrase);
            if (!entry.note.isEmpty()) {
                builder.append(" | ").append(entry.note);
            }
        }
        return builder.toString();
    }

    void replacePlainText(String text) {
        JSONArray items = new JSONArray();
        String[] parts = text.split("[,，\\n]");
        for (String part : parts) {
            String value = part.trim();
            if (value.isEmpty()) {
                continue;
            }
            String phrase = value;
            String note = "";
            int divider = value.indexOf('|');
            if (divider >= 0) {
                phrase = value.substring(0, divider).trim();
                note = value.substring(divider + 1).trim();
            }
            if (!phrase.isEmpty()) {
                items.put(toJson(new Entry(UUID.randomUUID().toString(), phrase, note, true, 0, isoNow())));
            }
        }
        prefs.edit().putString(KEY, items.toString()).apply();
    }

    Entry add(String phrase, String note) {
        Entry entry = new Entry(UUID.randomUUID().toString(), phrase.trim(), note == null ? "" : note.trim(), true, 0, isoNow());
        if (entry.phrase.isEmpty()) {
            return entry;
        }
        List<Entry> entries = list();
        entries.add(0, entry);
        write(entries);
        return entry;
    }

    void remove(String id) {
        List<Entry> entries = list();
        List<Entry> next = new ArrayList<>();
        for (Entry entry : entries) {
            if (!entry.id.equals(id)) {
                next.add(entry);
            }
        }
        write(next);
    }

    void setEnabled(String id, boolean enabled) {
        List<Entry> entries = list();
        List<Entry> next = new ArrayList<>();
        for (Entry entry : entries) {
            if (entry.id.equals(id)) {
                next.add(new Entry(entry.id, entry.phrase, entry.note, enabled, entry.hits, entry.createdAt));
            } else {
                next.add(entry);
            }
        }
        write(next);
    }

    int recordHits(String text) {
        if (text == null || text.isEmpty()) {
            return 0;
        }
        String haystack = text.toLowerCase(Locale.ROOT);
        int total = 0;
        boolean changed = false;
        List<Entry> next = new ArrayList<>();
        for (Entry entry : list()) {
            if (!entry.enabled || entry.phrase.isEmpty()) {
                next.add(entry);
                continue;
            }
            int count = countOccurrences(haystack, entry.phrase.toLowerCase(Locale.ROOT));
            if (count > 0) {
                total += count;
                changed = true;
                next.add(new Entry(entry.id, entry.phrase, entry.note, entry.enabled, entry.hits + count, entry.createdAt));
            } else {
                next.add(entry);
            }
        }
        if (changed) {
            write(next);
        }
        return total;
    }

    private void write(List<Entry> entries) {
        JSONArray items = new JSONArray();
        for (Entry entry : entries) {
            items.put(toJson(entry));
        }
        prefs.edit().putString(KEY, items.toString()).apply();
    }

    private JSONObject toJson(Entry entry) {
        JSONObject item = new JSONObject();
        try {
            item.put("id", entry.id);
            item.put("phrase", entry.phrase);
            item.put("note", entry.note);
            item.put("enabled", entry.enabled);
            item.put("hits", entry.hits);
            item.put("createdAt", entry.createdAt == null || entry.createdAt.isEmpty() ? isoNow() : entry.createdAt);
        } catch (Exception ignored) {
        }
        return item;
    }

    private static int countOccurrences(String haystack, String needle) {
        if (needle.isEmpty() || haystack.length() < needle.length()) {
            return 0;
        }
        int count = 0;
        int start = 0;
        while (true) {
            int pos = haystack.indexOf(needle, start);
            if (pos < 0) {
                return count;
            }
            count++;
            start = pos + needle.length();
        }
    }

    private static String isoNow() {
        SimpleDateFormat format = new SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'", Locale.US);
        format.setTimeZone(TimeZone.getTimeZone("UTC"));
        return format.format(new Date());
    }

    private static String firstNonEmpty(String first, String second) {
        return first == null || first.isEmpty() ? (second == null ? "" : second) : first;
    }

    static final class Entry {
        final String id;
        final String phrase;
        final String note;
        final boolean enabled;
        final long hits;
        final String createdAt;

        Entry(String id, String phrase, String note, boolean enabled, long hits, String createdAt) {
            this.id = id;
            this.phrase = phrase;
            this.note = note;
            this.enabled = enabled;
            this.hits = hits;
            this.createdAt = createdAt;
        }
    }
}
