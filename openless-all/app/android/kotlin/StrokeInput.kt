package com.openless.app

import android.content.Context
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicReference

/** Offline five-stroke lookup. The query work never runs on the IME main thread. */
internal class StrokeInputRepository(context: Context) {
    private val appContext = context.applicationContext
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-stroke-query").apply { isDaemon = true }
    }

    private val builtInEntries = listOf(
        "你" to "psh", "好" to "ny", "我" to "psh", "是" to "hs", "的" to "p", "不" to "h",
        "了" to "z", "在" to "sh", "人" to "p", "有" to "h", "这" to "z", "个" to "p",
        "上" to "hs", "中" to "s", "国" to "s", "大" to "h", "为" to "n", "来" to "h",
        "到" to "z", "时" to "hs", "地" to "hs", "出" to "s", "要" to "h", "于" to "s",
        "可" to "h", "以" to "p", "没" to "n", "和" to "p", "说" to "n", "着" to "n",
        "看" to "h", "天" to "h", "我" to "psh", "们" to "p", "你" to "psh", "他" to "p",
        "她" to "p", "它" to "p", "这" to "z", "那" to "z", "什" to "p", "么" to "p",
        "请" to "n", "问" to "z", "谢" to "n", "再" to "h", "见" to "h", "中" to "s",
        // Each stroke of the entry below, in order: dot, horizontal, vertical, fold, horizontal,
        // vertical, slant, dot, horizontal, slant, fold, dot.
        "就" to "nhszhspnhpzn",
        "文" to "n", "一" to "h", "二" to "h", "三" to "h", "四" to "p", "五" to "h",
        "六" to "n", "七" to "h", "八" to "p", "九" to "p", "零" to "n",
    )

    // Every list in here is in corpus-frequency order (see ensureLoaded()),
    // so a query only walks it until it has enough matches.
    private val index = AtomicReference<Map<String, List<Pair<String, String>>>>(emptyMap())
    private val frequency = AtomicReference<Map<String, Long>>(emptyMap())
    private val codesByChar = AtomicReference<Map<String, List<String>>>(emptyMap())
    private val loading = Any()

    // Personal per-code preference, learned from picking a non-top candidate.
    // Kept global (not per-target-app, unlike phrase association) since which
    // character you mean for a given stroke code is a habit, not context —
    // sharing StrokeUserFrequency's store/cap with the phrase repository is
    // intentional, both are "how you personally type" data.
    private val userFrequency = StrokeUserFrequency(context)

    /** Warm the offline index before the first stroke key is pressed. */
    fun preloadAsync() {
        executor.execute { ensureLoaded() }
    }

    private fun loadEntries(context: Context): List<Pair<String, String>> {
        // A character may have more than one valid stroke sequence in Rime.
        // Keep every code here; deduplicate only the rendered character list
        // after filtering, otherwise valid alias codes (a character with two
        // sequences) vanish.
        return runCatching {
            context.assets.open("stroke.dict.tsv").bufferedReader().useLines { lines ->
                lines.mapNotNull { line ->
                    val parts = line.split('\t', limit = 2)
                    if (parts.size == 2 && parts[0].isNotEmpty() && parts[1].all { it in "hspnz" }) {
                        parts[0] to parts[1]
                    } else null
                }.toList()
            }
        }.getOrElse { builtInEntries }
    }

    /** Corpus character frequency, used to rank candidates by how common they are. */
    private fun loadFrequency(context: Context): Map<String, Long> = runCatching {
        context.assets.open("stroke-frequency.tsv").bufferedReader().useLines { lines ->
            lines.mapNotNull { line ->
                val parts = line.split('\t', limit = 2)
                val weight = parts.getOrNull(1)?.toLongOrNull()
                if (parts.size == 2 && parts[0].isNotEmpty() && weight != null) parts[0] to weight else null
            }.toMap()
        }
    }.getOrElse { emptyMap() }

    fun searchAsync(pattern: String, callback: (List<String>) -> Unit) {
        executor.execute {
            ensureLoaded()
            val startedAt = android.os.SystemClock.elapsedRealtime()
            val result = if (pattern.isEmpty()) emptyList() else {
                val freq = frequency.get()
                val codes = codesByChar.get()
                // Only characters still spelled by this code: the store
                // outlives dictionary updates.
                val picks = userFrequency.picksFor(pattern).filter { (char, _) ->
                    codes[char]?.any { strokeCodeMatches(pattern, it) } == true
                }
                rankStrokeCandidates(
                    index.get().getOrDefault(pattern.takeWhile { it != '*' }.take(PREFIX_INDEX_LENGTH), emptyList()),
                    pattern,
                    picks,
                    MAX_CANDIDATES,
                ) { freq[it] ?: 0L }
            }
            android.util.Log.d(
                "OpenLessStroke",
                "query pattern=$pattern results=${result.size} elapsed=${android.os.SystemClock.elapsedRealtime() - startedAt}ms",
            )
            android.os.Handler(android.os.Looper.getMainLooper()).post { callback(result) }
        }
    }

    /**
     * Records a pick of `candidate` for `pattern`, off the caller's thread —
     * when it was hand-picked over the top result ([wasTop] false), or when
     * it is on top only because of earlier picks. The latter keeps a learned
     * character's score fresh: recording corrections alone let it decay and
     * lose its place to the next one-off pick. Picking a corpus-top
     * character that was never learned records nothing.
     */
    fun recordPick(pattern: String, candidate: String, wasTop: Boolean) {
        executor.execute {
            if (shouldRecordStrokePick(wasTop, userFrequency.score(pattern, candidate) > 0.0)) {
                userFrequency.record(pattern, candidate)
            }
        }
    }

    fun shutdown() = executor.shutdownNow()

    private fun ensureLoaded() {
        if (index.get().isNotEmpty()) return
        synchronized(loading) {
            if (index.get().isNotEmpty()) return
            val startedAt = android.os.SystemClock.elapsedRealtime()
            val freq = loadFrequency(appContext)
            // Sorted once here (stable, so equal counts keep file order) and
            // bucketed in that order: every bucket comes out frequency-sorted
            // without sorting each one, and no query ever sorts again.
            val entries = loadEntries(appContext).sortedByDescending { freq[it.first] ?: 0L }
            val buckets = HashMap<String, MutableList<Pair<String, String>>>()
            val codes = HashMap<String, MutableList<String>>()
            buckets[""] = entries.toMutableList()
            entries.forEach { entry ->
                val code = entry.second
                (1..minOf(PREFIX_INDEX_LENGTH, code.length)).forEach { length ->
                    buckets.getOrPut(code.substring(0, length)) { ArrayList() }.add(entry)
                }
                codes.getOrPut(entry.first) { ArrayList(1) }.add(code)
            }
            frequency.set(freq)
            codesByChar.set(codes)
            index.set(buckets)
            android.util.Log.i(
                "OpenLessStroke",
                "index ready entries=${entries.size} frequency=${frequency.get().size} " +
                    "elapsed=${android.os.SystemClock.elapsedRealtime() - startedAt}ms",
            )
        }
    }

    private companion object {
        const val PREFIX_INDEX_LENGTH = 4
        const val MAX_CANDIDATES = 36
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
 * matches from [entries] — which must already be in corpus-frequency order —
 * each character once, until [limit]. A personal pick always outranks the
 * corpus: counts there span 1 to several million, so adding a personal score
 * would never move a common character.
 *
 * Same order as sorting every match by (score, frequency), but it stops as
 * soon as the row is full — the first stroke alone matches up to ~48k
 * entries, and scoring and sorting all of them made that keystroke slow.
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
