package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import java.util.LinkedHashMap
import java.util.concurrent.Executors

/** Independent confirmed-text -> phrase predictor; it never reads stroke codes. */
internal class StrokePhraseRepository(context: Context) {
    private class Node {
        val children = HashMap<Char, Node>()
        val top = ArrayList<String>(NODE_TOP_N)
    }

    private val appContext = context.applicationContext
    private val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-phrase-query").apply { isDaemon = true }
    }
    private val root = Node()
    private val cache = object : LinkedHashMap<String, List<String>>(CACHE_SIZE, .75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, List<String>>?) = size > CACHE_SIZE
    }
    private var loaded = false

    fun searchAsync(prefix: String, callback: (List<String>) -> Unit) {
        if (prefix.isEmpty()) return callback(emptyList())
        executor.execute {
            ensureLoaded()
            val result = synchronized(cache) { cache[prefix] } ?: find(prefix).also {
                synchronized(cache) { cache[prefix] = it }
            }
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
                    lines.forEach { phrase ->
                        if (phrase.length in 2..8) insert(phrase)
                    }
                }
            }
            loaded = true
        }
    }

    private fun insert(phrase: String) {
        var node = root
        phrase.forEach { character ->
            node = node.children.getOrPut(character) { Node() }
            if (!node.top.contains(phrase) && node.top.size < NODE_TOP_N) node.top += phrase
        }
    }

    private fun find(prefix: String): List<String> {
        var node = root
        prefix.forEach { character -> node = node.children[character] ?: return emptyList() }
        return node.top.toList()
    }

    private companion object {
        const val NODE_TOP_N = 12
        const val CACHE_SIZE = 64
    }
}
