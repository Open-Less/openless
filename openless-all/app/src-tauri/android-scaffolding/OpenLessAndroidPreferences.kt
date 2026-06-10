package com.openless.app

import android.content.Context
import android.util.Log
import java.io.File
import org.json.JSONObject

/**
 * Reads Android-visible preferences without depending on the Rust coordinator.
 */
object OpenLessAndroidPreferences {
    private const val TAG = "OpenLessAndroidPrefs"
    private const val APP_DIR = "OpenLess"
    private const val PREFERENCES_FILE = "preferences.json"
    private const val KEY_OVERLAY_TRIGGER = "androidOverlayTrigger"
    private const val KEY_OVERLAY_ACTIVATION_MODE = "androidOverlayActivationMode"
    private const val KEY_OVERLAY_LEFT_SWIPE_ACTION = "androidOverlayLeftSwipeAction"
    private val VALID_OVERLAY_TRIGGERS = setOf("background", "always")
    private val VALID_OVERLAY_ACTIVATION_MODES = setOf("tap", "long_press")
    private val VALID_OVERLAY_LEFT_SWIPE_ACTIONS = setOf("translation", "style_pack")

    fun overlayTriggerMode(context: Context): String? {
        val value = readPreferenceString(context, KEY_OVERLAY_TRIGGER) ?: return null
        if (value == "keyboard") {
            return "background"
        }
        return value.takeIf { it in VALID_OVERLAY_TRIGGERS }
    }

    fun overlayActivationMode(context: Context): String {
        return readPreferenceString(context, KEY_OVERLAY_ACTIVATION_MODE)
            ?.takeIf { it in VALID_OVERLAY_ACTIVATION_MODES }
            ?: "tap"
    }

    fun overlayLeftSwipeAction(context: Context): String {
        return readPreferenceString(context, KEY_OVERLAY_LEFT_SWIPE_ACTION)
            ?.takeIf { it in VALID_OVERLAY_LEFT_SWIPE_ACTIONS }
            ?: "translation"
    }

    private fun readPreferenceString(context: Context, key: String): String? {
        for (file in preferenceFiles(context).distinctBy { it.absolutePath }) {
            if (!file.isFile) {
                continue
            }
            val value = try {
                JSONObject(file.readText()).optString(key, "")
            } catch (error: Throwable) {
                Log.w(TAG, "read ${file.absolutePath} failed", error)
                ""
            }
            if (value.isNotBlank()) {
                return value
            }
        }
        return null
    }

    private fun preferenceFiles(context: Context): List<File> {
        val files = mutableListOf<File>()
        val envDir = System.getenv("TAURI_ANDROID_APP_DATA_DIR")
        if (!envDir.isNullOrBlank()) {
            files += File(File(envDir), APP_DIR).resolve(PREFERENCES_FILE)
        }
        files += File(File(context.cacheDir, APP_DIR), PREFERENCES_FILE)
        files += File(File(context.filesDir, APP_DIR), PREFERENCES_FILE)
        return files
    }
}
