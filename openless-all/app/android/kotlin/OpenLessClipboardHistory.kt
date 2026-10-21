package com.openless.app

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.nio.file.Files
import java.nio.file.StandardCopyOption

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
    // A failed disk write must leave the text available in the clipboard panel until retry or process exit.
    private var unsavedEntries: List<ClipboardEntry>? = null

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
        unsavedEntries?.let { return it }
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
    private fun save(context: Context, entries: List<ClipboardEntry>): Boolean {
        val target = file(context)
        val temporary = File(target.parentFile, "$FILE_NAME.tmp")
        return try {
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
            FileOutputStream(temporary).use { output ->
                output.write(array.toString().toByteArray(Charsets.UTF_8))
                output.fd.sync()
            }
            Files.move(temporary.toPath(), target.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            unsavedEntries = null
            true
        } catch (error: Exception) {
            unsavedEntries = entries
            android.util.Log.w("OpenLessClipboardHistory", "failed to save history", error)
            false
        } finally {
            temporary.delete()
        }
    }

    /**
     * Records a freshly copied string (deduplicated — an existing entry with
     * the same text is moved to the front and its timestamp refreshed
     * instead of creating a second row). Also used to bump an existing
     * history entry to the front when the user pastes it from the history
     * browser, per the "after a selected clip is committed, move it to the front of the
     * clipboard history" requirement. Returns true only after the entry is persisted;
     * on failure, load() still exposes the in-memory copy.
     */
    @Synchronized
    fun recordCopy(context: Context, text: String): Boolean {
        if (text.isBlank()) return false
        val current = load(context).toMutableList()
        // Preserve an existing favorite flag rather than silently clearing
        // it just because the same text got copied again.
        val wasFavorite = current.any { it.text == text && it.favorite }
        current.removeAll { it.text == text }
        current.add(0, ClipboardEntry(text, System.currentTimeMillis(), wasFavorite))
        while (current.size > MAX_ENTRIES) current.removeAt(current.lastIndex)
        return save(context, current)
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

    /** Removes every non-favorited entry — the history browser's "清空" (Clear) tab. Favorited entries are kept. */
    @Synchronized
    fun clearNonFavorites(context: Context) {
        val current = load(context).filter { it.favorite }
        save(context, current)
    }
}
