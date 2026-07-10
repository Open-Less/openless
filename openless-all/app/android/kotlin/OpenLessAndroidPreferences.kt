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
    private const val KEY_OVERLAY_CANCEL_SWIPE_DIRECTION = "androidOverlayCancelSwipeDirection"
    private const val KEY_OVERLAY_SIZE_DP = "androidOverlaySizeDp"
    private const val DEFAULT_OVERLAY_SIZE_DP = 72
    private const val MIN_OVERLAY_SIZE_DP = 48
    private const val MAX_OVERLAY_SIZE_DP = 120
    private val VALID_OVERLAY_TRIGGERS = setOf("background", "always")
    private val VALID_OVERLAY_ACTIVATION_MODES = setOf("tap", "long_press")
    private val VALID_OVERLAY_LEFT_SWIPE_ACTIONS = setOf("translation", "style_pack")
    private val VALID_OVERLAY_CANCEL_SWIPE_DIRECTIONS = setOf("up", "down")

    fun overlayTriggerMode(context: Context): String? {
        val value = readPreferenceString(context, KEY_OVERLAY_TRIGGER) ?: return null
        if (value == "keyboard") {
            return "background"
        }
        return value.takeIf { it in VALID_OVERLAY_TRIGGERS }
    }

    /** True when preferences still store legacy `"keyboard"` (before migration). */
    fun isKeyboardOverlayTrigger(context: Context): Boolean {
        return readPreferenceString(context, KEY_OVERLAY_TRIGGER) == "keyboard"
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

    fun overlayCancelSwipeDirection(context: Context): String {
        return readPreferenceString(context, KEY_OVERLAY_CANCEL_SWIPE_DIRECTION)
            ?.takeIf { it in VALID_OVERLAY_CANCEL_SWIPE_DIRECTIONS }
            ?: "up"
    }

    fun overlaySizeDp(context: Context): Int {
        return readPreferenceInt(context, KEY_OVERLAY_SIZE_DP)
            ?.coerceIn(MIN_OVERLAY_SIZE_DP, MAX_OVERLAY_SIZE_DP)
            ?: DEFAULT_OVERLAY_SIZE_DP
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

    private fun readPreferenceInt(context: Context, key: String): Int? {
        for (file in preferenceFiles(context).distinctBy { it.absolutePath }) {
            if (!file.isFile) {
                continue
            }
            val value = try {
                val json = JSONObject(file.readText())
                if (json.has(key)) json.optInt(key) else null
            } catch (error: Throwable) {
                Log.w(TAG, "read ${file.absolutePath} failed", error)
                null
            }
            if (value != null) {
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
