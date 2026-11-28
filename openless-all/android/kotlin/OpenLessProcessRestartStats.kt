package com.openless.app

import android.content.Context
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Today's count of how many times one specific OS process has (re)started
 * — a quick, adb-free signal the user can check in Settings for whether
 * the background-kill mitigations (Phase 1/2/3) are actually helping. A
 * cold process start looks the same here whether it's the first launch of
 * the day or the OS/OEM force-killing and restarting it — that ambiguity
 * doesn't matter for this purpose.
 *
 * Only ever tracks *today*, and only for the currently-installed build:
 * OpenLessApplication resets every category back to 0 whenever
 * OpenLessBuildInfo.VERSION changes, since a count from a previous build
 * (or a previous day) isn't meaningful to keep comparing against.
 *
 * [MAIN] (default process, hosts OpenLessImeService + the Tauri backend) and
 * [ACCESSIBILITY] (the `:accessibility` process declared in the manifest)
 * are tracked separately, since the crash/kill debugging that motivated this
 * counter treated them as two distinct, independently-restartable processes
 * sharing one SharedPreferences file — recordStart()/today() only ever
 * touch keys under their own [processKey] prefix.
 */
class OpenLessProcessRestartStats(context: Context, private val processKey: String) {
    private val preferences = context.applicationContext.getSharedPreferences(STORE, Context.MODE_PRIVATE)

    fun recordStart() {
        val key = todayKey()
        preferences.edit().putInt(key, preferences.getInt(key, 0) + 1).apply()
        prune()
    }

    fun today(): Int = preferences.getInt(todayKey(), 0)

    /** Zeroes today's count — used when a version bump makes it no longer meaningful to compare against. */
    fun resetToday() {
        preferences.edit().remove(todayKey()).apply()
    }

    /** Drops any key under this processKey other than today's — yesterday's (or an older build's) count has nothing left worth showing. */
    private fun prune() {
        val keep = todayKey()
        val prefix = "$processKey|"
        val edit = preferences.edit()
        var changed = false
        for (key in preferences.all.keys) {
            if (!key.startsWith(prefix)) continue // never touch the other process's keys
            if (key != keep) {
                edit.remove(key)
                changed = true
            }
        }
        if (changed) edit.apply()
    }

    private fun todayKey(): String = "$processKey|${FORMAT.format(Date())}"

    companion object {
        const val MAIN = "main"
        const val ACCESSIBILITY = "accessibility"
        private const val STORE = "openless_process_restart_stats"
        private val FORMAT = SimpleDateFormat("yyyy-MM-dd", Locale.US)
    }
}
