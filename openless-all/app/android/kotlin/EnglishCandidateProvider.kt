package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executors

/**
 * English word suggestions backed by Rime's `melt_eng` schema.
 *
 * The word list lives in Rime (rime-ice's `en_dicts/en.dict.yaml`, already a
 * dependency), so this class no longer loads or indexes a bundled corpus. What
 * stays here is what Rime cannot know:
 *
 * * the user's own picks and their ranking weight ([EnglishUserFrequency]);
 * * words the user typed that are not in any dictionary — they are kept as a
 *   small in-memory set and matched by prefix ahead of Rime's candidates;
 * * long-pressed "forget" words, hidden from the row.
 */
internal class EnglishCandidateProvider(context: Context) {
    private val appContext = context.applicationContext
    private val userFrequency = EnglishUserFrequency(context)
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-english-candidate").apply { isDaemon = true }
    }
    private val main = Handler(Looper.getMainLooper())

    private var session = ""
    private var sessionGeneration = -1L
    private var loaded = false
    /** Words the user typed that are not in Rime's dictionary (prefix-matched locally). */
    private val customWords = HashSet<String>()
    /** Long-pressed away via forgetCustomWord(); never suggested again. */
    private val excludedWords = HashSet<String>()

    /** Off the caller's thread; posts [callback] back to the main looper. */
    fun queryTopN(prefix: String, limit: Int = 3, callback: (List<String>) -> Unit) {
        val normalized = prefix.trim().lowercase()
        if (normalized.isEmpty()) {
            callback(emptyList())
            return
        }
        executor.execute {
            val result = runCatching {
                ensureLoaded()
                val fromRime = queryRime(normalized)
                val fromCustom = customWords.filter { it.startsWith(normalized) }
                (fromCustom + fromRime)
                    .filter { it !in excludedWords }
                    .distinct()
                    .sortedByDescending { userFrequency.score(it) }
                    .take(limit)
            }.getOrElse { failure ->
                android.util.Log.w(
                    "OpenLessEnglish",
                    "rime english query failed: ${failure.javaClass.simpleName}",
                )
                emptyList()
            }
            main.post { callback(result) }
        }
    }

    /** True only for a word the user's own typing added (see recordCommit()). */
    fun isCustomWord(word: String): Boolean = userFrequency.hasCustomWord(word)

    /**
     * "Forget" a word this keyboard only knows because the user typed it once
     * (long-press on an English candidate). Un-marks it as custom so a future
     * cold start will not re-add it, and hides it immediately.
     */
    fun forgetCustomWord(word: String) {
        val normalized = word.trim().lowercase()
        if (normalized.isEmpty()) return
        executor.execute {
            userFrequency.removeCustomWord(normalized)
            customWords.remove(normalized)
            excludedWords.add(normalized)
        }
    }

    /**
     * Call once a word is actually committed: boosts its ranking for next time
     * and — when no dictionary contains it — remembers it so future prefixes of
     * it are offered at all.
     */
    fun recordCommit(word: String) {
        val normalized = word.trim().lowercase()
        if (normalized.length < 2) return
        executor.execute {
            ensureLoaded()
            userFrequency.record(normalized)
            if (normalized !in customWords && !rimeKnows(normalized)) {
                userFrequency.addCustomWord(normalized)
                customWords.add(normalized)
            }
        }
    }

    fun shutdown() {
        executor.execute {
            if (session.isNotEmpty()) runCatching { OpenLessRime.request("destroy", session) }
            session = ""
        }
        executor.shutdown()
    }

    private fun ensureLoaded() {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            // Custom words from a previous session are not in any dictionary.
            runCatching { customWords.addAll(userFrequency.customWords()) }
            loaded = true
        }
    }

    private fun ensureSession() {
        if (session.isNotEmpty() && sessionGeneration == OpenLessRime.sessionGeneration) return
        OpenLessRime.initialize(appContext)
        session = OpenLessRime.request("create", "", "english").getString("session")
        sessionGeneration = OpenLessRime.sessionGeneration
    }

    /** Types [word] into the English session and returns its candidates in Rime's order. */
    private fun queryRime(word: String): List<String> {
        ensureSession()
        OpenLessRime.request("clear", session)
        for (letter in word) {
            if (letter !in 'a'..'z') return emptyList()
            OpenLessRime.request("key", session, letter.code.toString())
        }
        var state = OpenLessRime.request("candidates", session)
        val rows = mutableListOf<String>()
        rows += strings(state)
        val revision = state.optLong("revision")
        if (state.optBoolean("candidateHasNext")) {
            state = OpenLessRime.request("candidates", session, "8@$revision")
            rows += strings(state)
        }
        return rows
    }

    /** True when Rime itself offers [word] for its own prefix (i.e. it is in a dictionary). */
    private fun rimeKnows(word: String): Boolean =
        runCatching { queryRime(word).any { it.equals(word, ignoreCase = true) } }.getOrDefault(false)

    private fun strings(state: org.json.JSONObject): List<String> {
        val rows = state.optJSONArray("candidates") ?: return emptyList()
        return (0 until rows.length()).mapNotNull { rows.optString(it).takeIf(String::isNotEmpty) }
    }
}
