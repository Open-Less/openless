package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger

/**
 * Five-stroke lookup backed by Rime's `stroke` schema.
 *
 * The dictionary lives in Rime (`third_party/rime-stroke/stroke.dict.yaml`,
 * compiled into the shared data directory), so this file no longer carries or
 * loads any stroke corpus. What stays here is the part Rime has no opinion on:
 *
 * * the `*` wildcard — Rime's table translator matches prefixes exactly, so a
 *   pattern containing `*` is expanded into every concrete code it can stand
 *   for and the per-code results are merged (five queries per wildcard, capped);
 * * personal ranking — a character the user hand-picked for a code stays ahead
 *   of Rime's own order, which is a habit rather than a corpus fact.
 */
internal class StrokeInputRepository(context: Context) {
    private val appContext = context.applicationContext
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-stroke-query").apply { isDaemon = true }
    }
    private val main = Handler(Looper.getMainLooper())

    // Last-resort candidates when Rime cannot answer (session failure). Small on
    // purpose: this is not a dictionary, it only keeps the row non-empty.
    private val builtInEntries = listOf(
        "你" to "psh", "好" to "ny", "我" to "psh", "是" to "hs", "的" to "p", "不" to "h",
        "了" to "z", "在" to "sh", "人" to "p", "有" to "h", "这" to "z", "个" to "p",
        "上" to "hs", "中" to "s", "国" to "s", "大" to "h", "为" to "n", "来" to "h",
    )

    /** Personal per-code preference; kept in preferences, never a corpus. */
    private val userFrequency = StrokeUserFrequency.strokeCodes(context)

    private var session = ""
    private var sessionGeneration = -1L
    private val loading = Any()
    private val queryEpoch = AtomicInteger(0)

    /** Warm the Rime session so the first stroke key does not pay session creation. */
    fun preloadAsync() {
        executor.execute { runCatching { ensureSession() } }
    }

    fun searchAsync(pattern: String, predictionContext: String, callback: (List<String>) -> Unit) {
        val epoch = queryEpoch.incrementAndGet()
        executor.execute {
            val result = if (pattern.isEmpty()) emptyList() else runCatching {
                rankWithUserPicks(pattern, RimePrediction.rank(queryRime(pattern), predictionContext))
            }.getOrElse { failure ->
                android.util.Log.w("OpenLessStroke", "rime stroke query failed: ${failure.javaClass.simpleName}")
                rankWithUserPicks(pattern, fallbackCandidates(pattern))
            }
            main.post { if (epoch == queryEpoch.get()) callback(result) }
        }
    }

    /**
     * Records a pick of `candidate` for `pattern`, off the caller's thread —
     * when it was hand-picked over the top result ([wasTop] false), or when
     * it is on top only because of earlier picks.
     */
    fun recordPick(pattern: String, candidate: String, wasTop: Boolean) {
        executor.execute {
            if (shouldRecordStrokePick(wasTop, userFrequency.score(pattern, candidate) > 0.0)) {
                userFrequency.record(pattern, candidate)
            }
        }
    }

    fun shutdown() {
        queryEpoch.incrementAndGet()
        executor.execute {
            if (session.isNotEmpty()) runCatching { OpenLessRime.request("destroy", session) }
            session = ""
        }
        executor.shutdown()
    }

    private fun ensureSession() {
        if (session.isNotEmpty() && sessionGeneration == OpenLessRime.sessionGeneration) return
        synchronized(loading) {
            if (session.isNotEmpty() && sessionGeneration == OpenLessRime.sessionGeneration) return
            OpenLessRime.initialize(appContext)
            // Dictionary maintenance closes every native session; start a new one.
            session = OpenLessRime.request("create", "", "stroke").getString("session")
            sessionGeneration = OpenLessRime.sessionGeneration
        }
    }

    /**
     * Every concrete code a wildcard pattern can stand for. Rime has no wildcard
     * input, so `h*s` becomes `hhs`, `hss`, …; the expansion is capped because
     * each code costs one Rime round trip and codes are short in practice.
     */
    private fun expandWildcards(pattern: String): List<String> {
        if ('*' !in pattern) return listOf(pattern)
        var codes = listOf("")
        for (stroke in pattern) {
            codes = if (stroke == '*') {
                codes.flatMap { prefix -> STROKE_KEYS.map { prefix + it } }.take(MAX_EXPANSIONS)
            } else {
                codes.map { it + stroke }
            }
            if (codes.size > MAX_EXPANSIONS) codes = codes.take(MAX_EXPANSIONS)
        }
        return codes
    }

    private fun queryRime(pattern: String): List<String> {
        ensureSession()
        val merged = LinkedHashSet<String>()
        for (code in expandWildcards(pattern)) {
            if (merged.size >= MAX_CANDIDATES) break
            merged += queryCode(code)
        }
        return merged.toList()
    }

    /** Types one concrete code into the stroke session and reads its candidates. */
    private fun queryCode(code: String): List<String> {
        OpenLessRime.request("clear", session)
        for (stroke in code) {
            if (stroke !in STROKE_KEYS) return emptyList()
            OpenLessRime.request("key", session, stroke.code.toString())
        }
        var state = OpenLessRime.request("candidates", session)
        val rows = mutableListOf<String>()
        rows += strings(state)
        // page_size is 8; one more page covers the row we render.
        val revision = state.optLong("revision")
        if (state.optBoolean("candidateHasNext")) {
            state = OpenLessRime.request("candidates", session, "8@$revision")
            rows += strings(state)
        }
        return rows
    }

    private fun strings(state: org.json.JSONObject): List<String> {
        val rows = state.optJSONArray("candidates") ?: return emptyList()
        return (0 until rows.length()).mapNotNull { rows.optString(it).takeIf(String::isNotEmpty) }
    }

    /** Personal picks first (highest score first), then Rime's own order. */
    private fun rankWithUserPicks(pattern: String, candidates: List<String>): List<String> {
        val entries = candidates.map { it to pattern }
        return rankStrokeCandidates(entries, pattern, userFrequency.picksFor(pattern), MAX_CANDIDATES) { 0L }
    }

    private fun fallbackCandidates(pattern: String): List<String> =
        builtInEntries.filter { strokeCodeMatches(pattern, it.second) }.map { it.first }

    private companion object {
        const val MAX_CANDIDATES = 36
        const val MAX_EXPANSIONS = 25
        val STROKE_KEYS = listOf('h', 's', 'p', 'n', 'z')
    }
}

/** A stroke code matches a typed pattern when it starts with it, '*' standing for any one stroke. */
internal fun strokeCodeMatches(pattern: String, code: String): Boolean {
    if (pattern.length > code.length) return false
    return pattern.indices.all { index -> pattern[index] == '*' || pattern[index] == code[index] }
}

/**
 * Candidates for [pattern]: the user's own picks for this exact code first
 * (highest score first, corpus frequency breaking ties), then the remaining
 * matches from [entries] — which must already be in Rime's candidate order —
 * each character once, until [limit]. A personal pick always outranks the
 * engine: keeping it behind a fresh corpus would make "学过的字" pointless.
 */
internal fun rankStrokeCandidates(
    entries: List<Pair<String, String>>,
    pattern: String,
    picks: List<Pair<String, Double>>,
    limit: Int,
    frequencyOf: (String) -> Long,
): List<String> {
    val result = LinkedHashSet<String>()
    picks.filter { it.second > 0.0 }
        .sortedWith(compareByDescending<Pair<String, Double>> { it.second }.thenByDescending { frequencyOf(it.first) })
        .forEach { if (result.size < limit) result += it.first }
    for ((char, code) in entries) {
        if (result.size >= limit) break
        if (char !in result && strokeCodeMatches(pattern, code)) result += char
    }
    return result.toList()
}

/** See StrokeInputRepository.recordPick(): a correction always counts, a top pick only once it was learned. */
internal fun shouldRecordStrokePick(wasTop: Boolean, alreadyLearned: Boolean): Boolean = !wasTop || alreadyLearned
