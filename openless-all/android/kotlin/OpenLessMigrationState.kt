package com.openless.app

import android.content.Context
import androidx.annotation.Keep
import org.json.JSONObject
import java.util.UUID

/** The Core journal owns the decision; this gate only fences Android writers. */
@Keep
object OpenLessMigrationState {
    private val lock = Any()
    private var revision = UUID.randomUUID().toString()
    private var token: String? = null
    private var recoveryRequired = false
    private lateinit var app: Context
    private var restoringThread: Thread? = null
    private val learning = ArrayDeque<() -> Unit>()

    @Keep @JvmStatic
    fun initialize(context: Context, recovering: Boolean) = synchronized(lock) {
        attach(context)
        check(token == null)
        recoveryRequired = recoveryRequired && recovering
        persistFence(recoveryRequired)
        if (!recoveryRequired) drainLearning()
    }

    fun attach(context: Context) = synchronized(lock) {
        if (!::app.isInitialized) {
            app = context.applicationContext
            recoveryRequired = app.getSharedPreferences("openless_migration_gate", Context.MODE_PRIVATE).getBoolean("recovering", false)
        }
    }

    private fun persistFence(recovering: Boolean) {
        check(app.getSharedPreferences("openless_migration_gate", Context.MODE_PRIVATE).edit().putBoolean("recovering", recovering).commit()) { "migration_android_fence_write_failed" }
    }

    fun settings(write: () -> Unit): Boolean = synchronized(lock) {
        if (restoringThread === Thread.currentThread()) {
            write()
            return@synchronized true
        }
        if (token != null || recoveryRequired) return@synchronized false
        try { write() } finally { revision = UUID.randomUUID().toString() }
        true
    }

    fun learn(write: () -> Unit) = synchronized(lock) {
        if (token != null || recoveryRequired) learning.addLast(write)
        else {
            try { write() } finally { revision = UUID.randomUUID().toString() }
        }
    }

    // A mirror is sampled again later; never replay an obsolete WebView value.
    fun mirrorRevision(): String? = synchronized(lock) {
        if (token != null || recoveryRequired) null else revision
    }

    fun mirror(expectedRevision: String, write: () -> Unit) = synchronized(lock) {
        if (expectedRevision == revision) settings(write)
    }

    @Keep @JvmStatic
    fun capture(context: Context): String = synchronized(lock) {
        attach(context)
        check(token == null && !recoveryRequired) { "migration_android_busy" }
        check(ImeLearningPolicy.migrate(context)) { "migration_learning_cleanup_failed" }
        JSONObject().put("revision", revision)
            .put("value", JSONObject(OpenLessSettingsExport.exportPortable(context))).toString()
    }

    @Keep @JvmStatic
    fun acquire(expectedRevision: String): String = synchronized(lock) {
        check(token == null) { "migration_android_busy" }
        if (expectedRevision.isNotEmpty()) {
            check(!recoveryRequired && expectedRevision == revision) { "migration_bundle_stale_preview" }
        }
        persistFence(true)
        UUID.randomUUID().toString().also { token = it }
    }

    @Keep @JvmStatic
    fun captureLocked(context: Context, lease: String): String = synchronized(lock) {
        check(token == lease) { "migration_android_invalid_lease" }
        OpenLessSettingsExport.exportPortable(context)
    }

    internal fun restore(lease: String, write: () -> Unit) = synchronized(lock) {
        check(token == lease) { "migration_android_invalid_lease" }
        restoringThread = Thread.currentThread()
        try { write() } finally {
            restoringThread = null
            revision = UUID.randomUUID().toString()
        }
    }

    @Keep @JvmStatic
    fun replace(context: Context, lease: String, json: String) = restore(lease) {
        OpenLessSettingsExport.replacePortable(context, json)
    }

    @Keep @JvmStatic
    fun finish(lease: String, recovering: Boolean) = synchronized(lock) {
        check(token == lease) { "migration_android_invalid_lease" }
        try { persistFence(recovering) } catch (error: Throwable) {
            token = null
            recoveryRequired = true
            throw error
        }
        token = null
        recoveryRequired = recovering
        if (!recovering) drainLearning()
    }

    private fun drainLearning() {
        while (learning.isNotEmpty()) {
            learning.removeFirst().invoke()
            revision = UUID.randomUUID().toString()
        }
    }
}
