package com.openless.app

import android.content.Context
import android.content.SharedPreferences
import java.util.concurrent.ConcurrentHashMap

/**
 * Parsed, in-memory copy of one "count,lastUsedMs" usage store, shared by
 * every reader of that store in the process.
 *
 * Ranking looks a score up for every candidate it considers — reading and
 * splitting the stored string each time made that the slow part of a
 * stroke query. Every StrokeUserFrequency/EnglishUserFrequency instance
 * for a store gets this one cache, kept
 * current by a preference listener, so a write from any instance (the
 * keyboard, the settings page clearing it, an import) is seen by all.
 */
internal class FrequencyStampCache private constructor(
    private val preferences: SharedPreferences,
    private val groupOf: ((String) -> String?)?,
) {
    data class Stamp(val count: Int, val lastUsedMs: Long)

    private val stamps = ConcurrentHashMap<String, Stamp>()
    // groupOf(key) -> keys; lets the stroke store list every character
    // picked for one code without scanning the whole store.
    private val groups = ConcurrentHashMap<String, MutableSet<String>>()
    @Volatile
    private var loaded = false

    // Held as a field: SharedPreferences only keeps a weak reference to it.
    private val listener = SharedPreferences.OnSharedPreferenceChangeListener { _, key ->
        if (key == null) invalidate() else refresh(key)
    }

    init {
        preferences.registerOnSharedPreferenceChangeListener(listener)
    }

    fun get(key: String): Stamp? {
        ensureLoaded()
        return stamps[key]
    }

    fun keysInGroup(group: String): List<String> {
        ensureLoaded()
        return groups[group]?.toList().orEmpty()
    }

    /** Call alongside the preference write, so a query right after sees it before the listener runs. */
    fun put(key: String, stamp: Stamp) {
        ensureLoaded()
        store(key, stamp)
    }

    /** After a wholesale change (clear, import) — clear() alone doesn't notify listeners before API 30. */
    fun invalidate() {
        synchronized(this) {
            loaded = false
            stamps.clear()
            groups.clear()
        }
    }

    private fun ensureLoaded() {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            preferences.all.forEach { (key, value) -> parseStamp(value as? String)?.let { store(key, it) } }
            loaded = true
        }
    }

    private fun refresh(key: String) {
        if (!loaded) return
        val stamp = parseStamp(preferences.getString(key, null))
        if (stamp == null) {
            stamps.remove(key)
            groupOf?.invoke(key)?.let { group -> groups[group]?.remove(key) }
        } else {
            store(key, stamp)
        }
    }

    private fun store(key: String, stamp: Stamp) {
        stamps[key] = stamp
        groupOf?.invoke(key)?.let { group -> groups.getOrPut(group) { ConcurrentHashMap.newKeySet() }.add(key) }
    }

    companion object {
        private val registry = HashMap<String, FrequencyStampCache>()

        /** The process-wide cache for [store]; [groupOf] only takes effect for whoever opens it first. */
        fun of(context: Context, store: String, groupOf: ((String) -> String?)? = null): FrequencyStampCache =
            synchronized(registry) {
                registry.getOrPut(store) {
                    FrequencyStampCache(context.applicationContext.getSharedPreferences(store, Context.MODE_PRIVATE), groupOf)
                }
            }

        /** "count,lastUsedMs" -> Stamp, or null for anything else. */
        fun parseStamp(value: String?): Stamp? {
            val comma = value?.indexOf(',') ?: return null
            if (comma <= 0) return null
            val count = value.substring(0, comma).toIntOrNull() ?: return null
            val last = value.substring(comma + 1).toLongOrNull() ?: return null
            return Stamp(count, last)
        }
    }
}
