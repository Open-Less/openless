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
        // 就: 点、横、竖、折、横、竖、撇、点、横、撇、折、点。
        "就" to "nhszhspnhpzn",
        "文" to "n", "一" to "h", "二" to "h", "三" to "h", "四" to "p", "五" to "h",
        "六" to "n", "七" to "h", "八" to "p", "九" to "p", "零" to "n",
    )

    private val index = AtomicReference<Map<String, List<Pair<String, String>>>>(emptyMap())
    private val loading = Any()

    private fun loadEntries(context: Context): List<Pair<String, String>> {
        val table = runCatching {
            context.assets.open("stroke.dict.tsv").bufferedReader().useLines { lines ->
                lines.mapNotNull { line ->
                    val parts = line.split('\t', limit = 2)
                    if (parts.size == 2 && parts[0].isNotEmpty() && parts[1].all { it in "hspnz" }) {
                        parts[0] to parts[1]
                    } else null
                }.toList()
            }
        }.getOrElse { builtInEntries }
        val preferred = listOf(
            "一", "王", "二", "三", "十", "丁", "七", "大", "天", "人", "不", "有", "中", "国", "上", "下",
            "个", "了", "是", "的", "我", "你", "他", "她", "们", "在", "要", "来", "看", "去", "就",
        ).withIndex().associate { it.value to it.index }
        // A character may have more than one valid stroke sequence in Rime.
        // Keep every code here; deduplicate only the rendered character list
        // after filtering, otherwise valid aliases such as 过/hsnnzn vanish.
        return table.withIndex()
            .sortedWith(compareBy({ preferred[it.value.first] ?: Int.MAX_VALUE }, { it.index }))
            .map { it.value }
    }

    fun searchAsync(pattern: String, callback: (List<String>) -> Unit) {
        executor.execute {
            ensureLoaded()
            val lookupKey = pattern.takeWhile { it != '*' }.take(PREFIX_INDEX_LENGTH)
            val result = if (pattern.isEmpty()) emptyList() else index.get()
                .getOrDefault(lookupKey, emptyList())
                .asSequence()
                .filter { (_, code) -> matches(pattern, code) }
                .map { it.first }
                .distinct()
                .take(MAX_CANDIDATES)
                .toList()
            android.os.Handler(android.os.Looper.getMainLooper()).post { callback(result) }
        }
    }

    fun shutdown() = executor.shutdownNow()

    private fun ensureLoaded() {
        if (index.get().isNotEmpty()) return
        synchronized(loading) {
            if (index.get().isNotEmpty()) return
            val entries = loadEntries(appContext)
            val buckets = HashMap<String, MutableList<Pair<String, String>>>()
            buckets[""] = entries.toMutableList()
            entries.forEach { entry ->
                val code = entry.second
                (1..minOf(PREFIX_INDEX_LENGTH, code.length)).forEach { length ->
                    buckets.getOrPut(code.substring(0, length)) { ArrayList() }.add(entry)
                }
            }
            index.set(buckets)
        }
    }

    private fun matches(pattern: String, code: String): Boolean {
        if (pattern.length > code.length) return false
        return pattern.indices.all { index -> pattern[index] == '*' || pattern[index] == code[index] }
    }

    private companion object {
        const val PREFIX_INDEX_LENGTH = 4
        const val MAX_CANDIDATES = 36
    }
}
