package com.openless.app

import android.content.Context
import org.json.JSONObject
import java.io.File

internal object OpenLessRime {
    init { System.loadLibrary("openless_rime") }
    @Volatile var isReady = false
        private set
    @Volatile var lastInitError: String? = null
        private set
    private var initialized = false
    private val dictionaryWorker = java.util.concurrent.Executors.newSingleThreadExecutor()
    private var nextDictionaryCheck = 0L
    /** Bumped whenever the native side closed every session; a controller holding an older one starts a new one. */
    @Volatile var sessionGeneration = 0L
        private set
    external fun nativeRequest(operation: String, session: String, argument: String): String

    /** Pre-warms Rime off the main thread so the first Chinese keystroke does not stall. */
    fun preloadAsync(context: Context) {
        if (isReady) return
        val appContext = context.applicationContext
        RimeTaskQueue.executor.execute {
            runCatching { initialize(appContext) }
                .onFailure { error ->
                    lastInitError = error.message ?: error.javaClass.simpleName
                    android.util.Log.e("OpenLessRime", "preload failed: ${error.message}", error)
                }
        }
    }

    @Synchronized fun initialize(context: Context) {
        if (initialized) { refreshPublicDictionary(context); return }
        // Rime replaced LitePinyin; its stores are no longer read or backed up.
        for (name in listOf("openless_pinyin_frequency", "openless_pinyin_learned_phrases")) context.deleteSharedPreferences(name)
        val shared = File(context.filesDir, "rime-shared-1.17.0-ice-da1fbe60-v1").apply { mkdirs() }
        val files = context.assets.list("rime") ?: error("Rime assets unavailable")
        require(files.isNotEmpty())
        var maxSourceMtime = 0L
        for (name in files) {
            require(!name.contains('/') && !name.contains(".."))
            val target = File(shared, name)
            if (!target.exists()) {
                val temporary = File(shared, "$name.tmp")
                context.assets.open("rime/$name").use { input -> temporary.outputStream().use { input.copyTo(it) } }
                check(temporary.renameTo(target))
            }
            if (target.lastModified() > maxSourceMtime) maxSourceMtime = target.lastModified()
        }
        var selected = runCatching { JSONObject(OpenLessNative.nativePublicDictionary(shared.absolutePath, "select")).optString("data_path", shared.absolutePath) }
            .getOrDefault(shared.absolutePath)
        val profileDir = File(context.filesDir, "rime-user").apply { mkdirs() }
        val profile = profileDir.absolutePath
        seedPrebuiltArtifacts(context, profileDir, selected == shared.absolutePath, maxSourceMtime)
        var ready = false
        try {
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
            isReady = true
            lastInitError = null
        } catch (error: Throwable) {
            lastInitError = error.message ?: error.javaClass.simpleName
            throw error
        }
        refreshPublicDictionary(context)
    }

    /**
     * Copies host-precompiled Rime binary tables into rime-user/build on first install
     * (or data-version change) so librime skips 30–90s of on-device YAML compilation.
     */
    private fun seedPrebuiltArtifacts(
        context: Context,
        profileDir: File,
        usingBundledData: Boolean,
        maxSourceMtime: Long,
    ) {
        if (!usingBundledData) return
        val prebuilt = runCatching { context.assets.list("rime-prebuilt") }.getOrNull()
        if (prebuilt.isNullOrEmpty()) return
        val buildDir = File(profileDir, "build").apply { mkdirs() }
        val stamp = File(buildDir, ".openless-prebuilt-stamp")
        val version = "rime-1.17.0-ice-da1fbe60-v1"
        if (stamp.exists() && runCatching { stamp.readText().trim() }.getOrNull() == version) return
        val hasCustomPatch = File(profileDir, "openless_pinyin_learning.custom.yaml").exists() ||
            File(profileDir, "openless_pinyin_private.custom.yaml").exists()
        val freshMtime = maxOf(System.currentTimeMillis(), maxSourceMtime + 2000L)
        runCatching {
            for (name in prebuilt) {
                if (name.contains('/') || name.contains("..")) continue
                if (hasCustomPatch && name.endsWith(".schema.yaml")) continue
                val target = File(buildDir, name)
                val temporary = File(buildDir, "$name.tmp")
                context.assets.open("rime-prebuilt/$name").use { input ->
                    temporary.outputStream().use { input.copyTo(it) }
                }
                check(temporary.renameTo(target))
                target.setLastModified(freshMtime)
            }
            stamp.writeText(version)
        }.onFailure { error ->
            android.util.Log.w("OpenLessRime", "prebuilt seed skipped: ${error.message}")
        }
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
