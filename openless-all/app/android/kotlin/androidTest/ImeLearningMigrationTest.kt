package com.openless.app

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ImeLearningMigrationTest {
    @Test fun clearsOnlyLegacyAutomaticLearningAndRunsOnce() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val stores = listOf("openless_english_frequency", "openless_english_custom_words", "openless_pinyin_frequency", "openless_pinyin_learned_phrases")
        val marker = context.getSharedPreferences("openless_learning_privacy", Context.MODE_PRIVATE)
        assertTrue(marker.edit().clear().commit())
        val unrelated = context.getSharedPreferences("openless_clipboard_history", Context.MODE_PRIVATE)
        assertTrue(unrelated.edit().putString("keep", "fixture").commit())
        for (name in stores) assertTrue(context.getSharedPreferences(name, Context.MODE_PRIVATE).edit().putString("legacy", "secret").commit())
        assertTrue(ImeLearningPolicy.migrate(context))
        for (name in stores) assertTrue(context.getSharedPreferences(name, Context.MODE_PRIVATE).all.isEmpty())
        assertEquals("fixture", unrelated.getString("keep", null))
        val learned = context.getSharedPreferences(stores.first(), Context.MODE_PRIVATE)
        assertTrue(learned.edit().putString("new", "ordinary").commit())
        assertTrue(ImeLearningPolicy.migrate(context))
        assertEquals("ordinary", learned.getString("new", null))
    }
    @Test fun invalidAndLegacyCategoriesDoNotOverwriteSettings() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val prefs = context.getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE)
        assertTrue(prefs.edit().putInt("key_haptic_amplitude", 80).commit())
        val result = OpenLessSettingsExport.import(context,
            """{"openlessSettingsExportVersion":2,"haptic":{"enabled":true,"amplitude":999,"durationMs":12}}""",
            setOf(OpenLessSettingsExport.Category.HAPTIC))
        assertTrue(result.applied.isEmpty())
        assertTrue(result.errors.containsKey(OpenLessSettingsExport.Category.HAPTIC))
        assertEquals(80, prefs.getInt("key_haptic_amplitude", 0))
        val legacy = OpenLessSettingsExport.import(context,
            """{"openlessSettingsExportVersion":1,"pinyinLearnedPhrases":{"mm":"legacy,3"}}""",
            setOf(OpenLessSettingsExport.Category.PINYIN_LEARNED_PHRASES))
        assertTrue(legacy.applied.isEmpty())
        assertTrue(legacy.errors.isNotEmpty())
    }

}
