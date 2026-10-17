package com.openless.app

import android.content.Context
import org.json.JSONObject
import java.io.File

internal object OpenLessRime {
    init { System.loadLibrary("openless_rime") }
    private var initialized = false
    external fun nativeRequest(operation: String, session: String, argument: String): String
    @Synchronized fun initialize(context: Context) {
        if (initialized) return
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
        request("initialize", shared.absolutePath, File(context.filesDir, "rime-user").absolutePath)
        initialized = true
    }
    fun request(operation: String, session: String = "", argument: String = ""): JSONObject {
        val response = JSONObject(nativeRequest(operation, session, argument))
        check(response.optBoolean("ok")) { response.optString("error", "Rime request failed") }
        return response
    }
}
