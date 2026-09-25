package com.openless.app

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** One remembered clipboard entry. */
data class ClipboardEntry(val text: String, val timestamp: Long, val favorite: Boolean = false)

/**
 * Persisted clipboard history for the IME's clipboard panel. Stored as a
 * small JSON file in the app's private storage (survives IME/device
 * restarts, matching how a normal clipboard manager behaves) — there is no
 * system API for clipboard *history*, only the current clip, so this app
 * has to keep its own record as new clips come in.
 */
object OpenLessClipboardHistory {
    private const val FILE_NAME = "openless_clipboard_history.json"
    private const val MAX_ENTRIES = 200

    enum class Category { ALL, RECENT, TEXT, NUMBER, LINK }

    private val urlRegex = Regex("^(https?://|www\\.)\\S+$", RegexOption.IGNORE_CASE)
    private val numberRegex = Regex("^[+-]?\\d+(\\.\\d+)?$")

    fun categoryOf(text: String): Category {
        val trimmed = text.trim()
        return when {
            numberRegex.matches(trimmed) -> Category.NUMBER
            urlRegex.containsMatchIn(trimmed) -> Category.LINK
            else -> Category.TEXT
        }
    }

    fun filter(entries: List<ClipboardEntry>, category: Category): List<ClipboardEntry> = when (category) {
        Category.ALL -> entries
        Category.RECENT -> entries.take(20)
        else -> entries.filter { categoryOf(it.text) == category }
    }

    private fun file(context: Context): File = File(context.filesDir, FILE_NAME)

    @Synchronized
    fun load(context: Context): List<ClipboardEntry> {
        return try {
            val target = file(context)
            if (!target.exists()) return emptyList()
            val array = JSONArray(target.readText())
            (0 until array.length()).map { index ->
                val obj = array.getJSONObject(index)
                ClipboardEntry(obj.getString("text"), obj.optLong("ts"), obj.optBoolean("favorite", false))
            }
        } catch (error: Exception) {
            android.util.Log.w("OpenLessClipboardHistory", "failed to load history", error)
            emptyList()
        }
    }

    @Synchronized
    private fun save(context: Context, entries: List<ClipboardEntry>) {
        try {
            val array = JSONArray()
            entries.forEach { entry ->
                array.put(
                    JSONObject().apply {
                        put("text", entry.text)
                        put("ts", entry.timestamp)
                        put("favorite", entry.favorite)
                    },
                )
            }
            file(context).writeText(array.toString())
        } catch (error: Exception) {
            android.util.Log.w("OpenLessClipboardHistory", "failed to save history", error)
        }
    }

    /**
     * Records a freshly copied string (deduplicated — an existing entry with
     * the same text is moved to the front and its timestamp refreshed
     * instead of creating a second row). Also used to bump an existing
     * history entry to the front when the user pastes it from the history
     * browser, per the "选中上屏后调整剪贴板中的顺序为第一条" requirement.
     */
    @Synchronized
    fun recordCopy(context: Context, text: String) {
        if (text.isBlank()) return
        val current = load(context).toMutableList()
        // Preserve an existing favorite flag rather than silently clearing
        // it just because the same text got copied again.
        val wasFavorite = current.any { it.text == text && it.favorite }
        current.removeAll { it.text == text }
        current.add(0, ClipboardEntry(text, System.currentTimeMillis(), wasFavorite))
        while (current.size > MAX_ENTRIES) current.removeAt(current.lastIndex)
        save(context, current)
    }

    /** Flips one entry's favorite flag by text — swipe-right on a clipboard history row. Preserves list order. */
    @Synchronized
    fun toggleFavorite(context: Context, text: String) {
        val current = load(context).map { entry ->
            if (entry.text == text) entry.copy(favorite = !entry.favorite) else entry
        }
        save(context, current)
    }

    /** Removes one entry by text — the swipe-right "delete" zone, past the favorite zone. */
    @Synchronized
    fun delete(context: Context, text: String) {
        val current = load(context).filterNot { it.text == text }
        save(context, current)
    }
}
