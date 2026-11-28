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
            if (session.isNotEmpty()) {
                // Same queue as maintenance: never destroy while a redeploy or
                // dictionary import is already closing sessions.
                runCatching { RimeTaskQueue.run { OpenLessRime.request("destroy", session) } }
            }
            session = ""
        }
        executor.shutdown()
    }

    private fun ensureSession() {
        if (session.isNotEmpty() && sessionGeneration == OpenLessRime.sessionGeneration) return
        synchronized(loading) {
            if (session.isNotEmpty() && sessionGeneration == OpenLessRime.sessionGeneration) return
            OpenLessRime.initialize(appContext)
            // Dictionary maintenance closes every native session. Creating ours
            // on the maintenance queue keeps the handle and the generation it
            // was created in one atomic step: otherwise a redeploy landing
            // between the two made a destroyed handle look current forever.
            val created = OpenLessRime.createSession("stroke")
            session = created.first
            sessionGeneration = created.second
        }
    }

    private fun queryRime(pattern: String): List<String> = RimeTaskQueue.run {
        // Serialized with maintenance for the same reason as session creation:
        // a redeploy landing mid-query would close the session under us.
        ensureSession()
        val merged = LinkedHashSet<String>()
        for (code in expandStrokePattern(pattern)) {
            if (merged.size >= MAX_CANDIDATES) break
            merged += queryCode(code)
        }
        merged.toList()
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
    }
}

/** The five stroke keys, in Rime's table order. */
private val STROKE_KEYS = listOf('h', 's', 'p', 'n', 'z')

/** Four wildcards = 625 codes, which covers every realistic pattern. */
private const val MAX_EXHAUSTIVE_WILDCARDS = 4

/**
 * Every concrete code a wildcard pattern can stand for. Rime has no wildcard
 * input, so `h*s` becomes `hhs`, `hss`, … Up to [MAX_EXHAUSTIVE_WILDCARDS]
 * wildcards are enumerated completely. A longer pattern would need 5^k codes, and
 * keeping only the first N of those pins the leading wildcards to `h` and hides
 * real matches — a fresh `***phhspnpphs` must still reach `nhnphhspnpphs` (its
 * six-entry code). Beyond four, the first four still vary independently and every
 * later wildcard follows the combination index, so no position becomes literal.
 */
internal fun expandStrokePattern(pattern: String): List<String> {
    val wildcards = pattern.count { it == '*' }
    if (wildcards == 0) return listOf(pattern)
    val expanded = minOf(wildcards, MAX_EXHAUSTIVE_WILDCARDS)
    var combinations = 1
    repeat(expanded) { combinations *= STROKE_KEYS.size }
    return List(combinations) { combination -> concreteStrokeCode(pattern, expanded, combination) }
}

/**
 * One combination of [pattern]: its first [expanded] wildcards count up in mixed
 * radix, and any further wildcard follows the combination index so it is never
 * pinned to a single stroke.
 */
private fun concreteStrokeCode(pattern: String, expanded: Int, combination: Int): String {
    val code = StringBuilder(pattern.length)
    var remaining = combination
    var seen = 0
    for (stroke in pattern) {
        if (stroke != '*') {
            code.append(stroke)
            continue
        }
        if (seen < expanded) {
            var divisor = 1
            repeat(expanded - seen - 1) { divisor *= STROKE_KEYS.size }
            code.append(STROKE_KEYS[(remaining / divisor) % STROKE_KEYS.size])
            remaining %= divisor
        } else {
            code.append(STROKE_KEYS[combination % STROKE_KEYS.size])
        }
        seen++
    }
    return code.toString()
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
