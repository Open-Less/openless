package com.openless.app

import android.content.Context
import org.json.JSONObject
import java.io.File

internal object OpenLessRime {
    init { System.loadLibrary("openless_rime") }
    private var initialized = false
    private val dictionaryWorker = java.util.concurrent.Executors.newSingleThreadExecutor()
    private var nextDictionaryCheck = 0L
    /** Bumped whenever the native side closed every session; a controller holding an older one starts a new one. */
    @Volatile var sessionGeneration = 0L
        private set
    external fun nativeRequest(operation: String, session: String, argument: String): String
    @Synchronized fun initialize(context: Context) {
        if (initialized) { refreshPublicDictionary(context); return }
        // Rime replaced LitePinyin; its stores are no longer read or backed up.
        for (name in listOf("openless_pinyin_frequency", "openless_pinyin_learned_phrases")) context.deleteSharedPreferences(name)
        val shared = File(context.filesDir, "rime-shared-1.17.0-ice-da1fbe60-v1").apply { mkdirs() }
        val files = context.assets.list("rime") ?: error("Rime assets unavailable")
        require(files.isNotEmpty())
        for (name in files) {
            require(!name.contains('/') && !name.contains(".."))
            val target = File(shared, name)
            if (!target.exists()) {
                val temporary = File(shared, "$name.tmp")
                context.assets.open("rime/$name").use { input -> temporary.outputStream().use { input.copyTo(it) } }
                check(temporary.renameTo(target))
            }
        }
        var selected = runCatching { JSONObject(OpenLessNative.nativePublicDictionary(shared.absolutePath, "select")).optString("data_path", shared.absolutePath) }
            .getOrDefault(shared.absolutePath)
        val profile = File(context.filesDir, "rime-user").absolutePath
        var ready = false
        repeat(3) {
            if (!ready) {
                if (selected != shared.absolutePath && !selected.startsWith(shared.absolutePath + "/public-updates-v1/")) selected = shared.absolutePath
                try { request("initialize", selected, profile); ready = true }
                catch (error: Exception) {
                    if (selected == shared.absolutePath) throw error
                    selected = if (it == 1) shared.absolutePath else runCatching {
                        JSONObject(OpenLessNative.nativePublicDictionary(shared.absolutePath, "reject")).optString("data_path", shared.absolutePath)
                    }.getOrDefault(shared.absolutePath)
                }
            }
        }
        check(ready)
        initialized = true
        refreshPublicDictionary(context)
    }
    private fun refreshPublicDictionary(context: Context) {
        if (!context.getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE).getBoolean("public_dictionary_updates", true)) return
        val now = System.currentTimeMillis()
        if (now < nextDictionaryCheck) return
        nextDictionaryCheck = now + java.util.concurrent.TimeUnit.DAYS.toMillis(1)
        val base = File(context.filesDir, "rime-shared-1.17.0-ice-da1fbe60-v1").absolutePath
        dictionaryWorker.execute {
            runCatching { OpenLessNative.nativePublicDictionary(base, "update") }
                .onFailure { android.util.Log.w("OpenLessRime", "public dictionary refresh unavailable; using local data") }
        }
    }
    fun request(operation: String, session: String = "", argument: String = ""): JSONObject {
        val response = JSONObject(nativeRequest(operation, session, argument))
        check(response.optBoolean("ok")) { response.optString("error", "Rime request failed") }
        return response
    }

    private fun closingSessions(operation: String, argument: String): JSONObject =
        try { request(operation, "", argument) } finally { sessionGeneration++ }

    /** Writes the pinyin user dictionary snapshot to [target]; false when nothing has been learned yet. Off the main thread. */
    fun exportUserDictionary(context: Context, target: File): Boolean = RimeTaskQueue.run {
        initialize(context)
        closingSessions("user_dict_export", target.absolutePath).optBoolean("exported")
    }

    /** Validates and merges learned entries only; imported deletion markers cannot remove local words. Off the main thread. */
    fun importUserDictionary(context: Context, source: File) = RimeTaskQueue.run {
        val snapshot = RimeUserDictionaryFile.forImport(source.readText())
        val normalized = File.createTempFile("pinyin-import-", ".userdb.txt", context.cacheDir)
        try {
            normalized.writeText(snapshot)
            initialize(context)
            closingSessions("user_dict_import", normalized.absolutePath)
            Unit
        } finally { normalized.delete() }
    }

    /** Rewrites both pinyin schemas' patches for [groups] and redeploys them. Off the main thread. */
    fun applyFuzzyPinyin(context: Context, groups: Set<String>) = RimeTaskQueue.run {
        initialize(context)
        val patch = RimeFuzzyPinyin.customYaml(groups)
        for (schema in listOf("openless_pinyin_learning", "openless_pinyin_private")) {
            val file = File(context.filesDir, "rime-user/$schema.custom.yaml")
            if (patch == null) file.delete() else file.writeText(patch)
        }
        closingSessions("redeploy", "")
        Unit
    }
}
