package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.util.LinkedHashMap
import java.util.concurrent.Executors

/** Independent confirmed-text -> phrase predictor; it never reads stroke codes. */
internal class StrokePhraseRepository(context: Context) {
    data class Candidate(val text: String, val baseWeight: Int)

    private class Node {
        val children = HashMap<Char, Node>()
        val top = ArrayList<Candidate>(NODE_TOP_N)
    }

    private val appContext = context.applicationContext
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-phrase-query").apply { isDaemon = true }
    }
    private val root = Node()
    private val cache = object : LinkedHashMap<String, List<Candidate>>(CACHE_SIZE, .75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, List<Candidate>>?) = size > CACHE_SIZE
    }
    private var loaded = false

    fun searchAsync(prefix: String, packageName: String, callback: (List<Candidate>) -> Unit) {
        if (prefix.isEmpty()) return callback(emptyList())
        executor.execute {
            ensureLoaded()
            val result = (synchronized(cache) { cache[prefix] } ?: find(prefix).also {
                synchronized(cache) { cache[prefix] = it }
            }).sortedWith(compareByDescending<Candidate> { it.baseWeight + userFrequency.score(packageName, prefix, it.text).toInt() })
            Handler(Looper.getMainLooper()).post { callback(result) }
        }
    }

    fun shutdown() = executor.shutdownNow()

    private fun ensureLoaded() {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            runCatching {
                appContext.assets.open("phrases.dict.tsv").bufferedReader().useLines { lines ->
                    lines.forEach { line ->
                        val parts = line.split('\t', limit = 2)
                        val phrase = parts.getOrNull(0) ?: return@forEach
                        val weight = parts.getOrNull(1)?.toIntOrNull() ?: 0
                        if (phrase.length in 2..8 && weight > 0) insert(Candidate(phrase, weight))
                    }
                }
            }
            loaded = true
        }
    }

    private fun insert(candidate: Candidate) {
        var node = root
        candidate.text.forEach { character ->
            node = node.children.getOrPut(character) { Node() }
            if (node.top.none { it.text == candidate.text }) {
                node.top += candidate
                node.top.sortByDescending { it.baseWeight }
                if (node.top.size > NODE_TOP_N) node.top.removeAt(node.top.lastIndex)
            }
        }
    }

    private fun find(prefix: String): List<Candidate> {
        var node = root
        prefix.forEach { character -> node = node.children[character] ?: return emptyList() }
        return node.top.toList()
    }

    private companion object {
        const val NODE_TOP_N = 12
        const val CACHE_SIZE = 64
    }

    private val userFrequency = StrokeUserFrequency(context)
}
