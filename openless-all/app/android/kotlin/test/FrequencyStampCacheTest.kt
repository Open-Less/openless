package com.openless.app

import android.content.SharedPreferences
import java.lang.reflect.Proxy
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.FutureTask
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FrequencyStampCacheTest {
    @Test fun aPreferenceRefreshCannotOverwriteANewerWrite() {
        val cache = afterConcurrentRefresh { values, cache ->
            values["word"] = "2,2000"
            cache.put("word", FrequencyStampCache.Stamp(2, 2000))
        }
        assertEquals(FrequencyStampCache.Stamp(2, 2000), cache.get("word"))
    }

    @Test fun aPreferenceRefreshCannotRestoreAClearedEntry() {
        val cache = afterConcurrentRefresh { values, cache ->
            values.clear()
            cache.invalidate()
        }
        assertNull(cache.get("word"))
    }

    private fun afterConcurrentRefresh(
        change: (MutableMap<String, String>, FrequencyStampCache) -> Unit,
    ): FrequencyStampCache {
        val values = ConcurrentHashMap<String, String>().apply { put("word", "1,1000") }
        val snapshotRead = CountDownLatch(1)
        val resumeRefresh = CountDownLatch(1)
        var listener: SharedPreferences.OnSharedPreferenceChangeListener? = null
        val preferences = Proxy.newProxyInstance(
            SharedPreferences::class.java.classLoader,
            arrayOf(SharedPreferences::class.java),
        ) { _, method, args ->
            when (method.name) {
                "getAll" -> HashMap(values)
                "getString" -> {
                    val snapshot = values[args!![0] as String]
                    snapshotRead.countDown()
                    check(resumeRefresh.await(5, TimeUnit.SECONDS))
                    snapshot
                }
                "registerOnSharedPreferenceChangeListener" -> {
                    listener = args!![0] as SharedPreferences.OnSharedPreferenceChangeListener
                    null
                }
                else -> error(method.name)
            }
        } as SharedPreferences
        val constructor = FrequencyStampCache::class.java.declaredConstructors
            .single { it.parameterCount == 2 }.apply { isAccessible = true }
        val cache = constructor.newInstance(preferences, null) as FrequencyStampCache
        assertEquals(FrequencyStampCache.Stamp(1, 1000), cache.get("word"))
        val refresh = FutureTask { listener!!.onSharedPreferenceChanged(preferences, "word") }
        val write = FutureTask { change(values, cache) }
        val writer = Thread(write)
        try {
            Thread(refresh).start()
            assertTrue(snapshotRead.await(5, TimeUnit.SECONDS))
            writer.start()
            // The old implementation finishes the write; the fixed one waits on the cache monitor.
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
            while (!write.isDone && writer.state != Thread.State.BLOCKED && System.nanoTime() < deadline) {
                Thread.yield()
            }
            assertTrue("Writer must finish or wait for the refresh", write.isDone || writer.state == Thread.State.BLOCKED)
        } finally {
            resumeRefresh.countDown()
        }
        refresh.get(5, TimeUnit.SECONDS)
        write.get(5, TimeUnit.SECONDS)
        return cache
    }
}
