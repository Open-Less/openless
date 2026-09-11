package com.openless.app

import java.util.concurrent.Executors

/** Offline five-stroke lookup. The query work never runs on the IME main thread. */
internal class StrokeInputRepository {
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-stroke-query").apply { isDaemon = true }
    }

    private val entries = listOf(
        "你" to "psh", "好" to "ny", "我" to "psh", "是" to "hs", "的" to "p", "不" to "h",
        "了" to "z", "在" to "sh", "人" to "p", "有" to "h", "这" to "z", "个" to "p",
        "上" to "hs", "中" to "s", "国" to "s", "大" to "h", "为" to "n", "来" to "h",
        "到" to "z", "时" to "hs", "地" to "hs", "出" to "s", "要" to "h", "于" to "s",
        "可" to "h", "以" to "p", "没" to "n", "和" to "p", "说" to "n", "着" to "n",
        "看" to "h", "天" to "h", "我" to "psh", "们" to "p", "你" to "psh", "他" to "p",
        "她" to "p", "它" to "p", "这" to "z", "那" to "z", "什" to "p", "么" to "p",
        "请" to "n", "问" to "z", "谢" to "n", "再" to "h", "见" to "h", "中" to "s",
        "文" to "n", "一" to "h", "二" to "h", "三" to "h", "四" to "p", "五" to "h",
        "六" to "n", "七" to "h", "八" to "p", "九" to "p", "零" to "n",
    ).distinctBy { it.first }

    fun searchAsync(pattern: String, callback: (List<String>) -> Unit) {
        executor.execute {
            val result = if (pattern.isEmpty()) emptyList() else entries.asSequence()
                .filter { (_, code) -> matches(pattern, code) }
                .map { it.first }
                .toList()
            android.os.Handler(android.os.Looper.getMainLooper()).post { callback(result) }
        }
    }

    fun shutdown() = executor.shutdownNow()

    private fun matches(pattern: String, code: String): Boolean {
        if (pattern.length > code.length) return false
        return pattern.indices.all { index -> pattern[index] == '*' || pattern[index] == code[index] }
    }
}
