package com.openless.app

import android.content.Context
import android.text.InputType
import android.view.inputmethod.EditorInfo

internal object ImeLearningPolicy {
    const val PRIVACY_VERSION = 1
    // LitePinyin's two stores are deleted outright by OpenLessRime.initialize().
    private val stores = listOf("openless_english_frequency", "openless_english_custom_words")

    fun isPassword(inputType: Int): Boolean {
        val kind = inputType and InputType.TYPE_MASK_CLASS
        val variation = inputType and InputType.TYPE_MASK_VARIATION
        return (kind == InputType.TYPE_CLASS_TEXT && variation in listOf(
            InputType.TYPE_TEXT_VARIATION_PASSWORD, InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD)) ||
            (kind == InputType.TYPE_CLASS_NUMBER && variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD)
    }

    fun allowsLearning(inputType: Int, imeOptions: Int): Boolean =
        inputType != InputType.TYPE_NULL && !isPassword(inputType) &&
            imeOptions and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING == 0

    /** Runs before any candidate executor starts; a failed clear must be retried. */
    fun migrate(context: Context): Boolean = runCatching {
        val marker = context.getSharedPreferences("openless_learning_privacy", Context.MODE_PRIVATE)
        if (marker.getInt("version", 0) >= PRIVACY_VERSION) return true
        OpenLessMigrationState.settings {
            for (name in stores) {
                check(context.getSharedPreferences(name, Context.MODE_PRIVATE).edit().clear().commit())
            }
            check(marker.edit().putInt("version", PRIVACY_VERSION).commit())
        }
    }.getOrDefault(false)
}
