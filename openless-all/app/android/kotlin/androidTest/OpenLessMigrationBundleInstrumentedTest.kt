package com.openless.app

import android.content.Context
import android.content.ContextWrapper
import android.content.SharedPreferences
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.assertFalse
import org.json.JSONObject
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class OpenLessMigrationBundleInstrumentedTest {
    private fun isolatedContext(): Context {
        val target = InstrumentationRegistry.getInstrumentation().targetContext
        val prefix = "migration-test-${java.util.UUID.randomUUID()}-"
        return object : ContextWrapper(target) {
            override fun getApplicationContext(): Context = this
            override fun getSharedPreferences(name: String, mode: Int): SharedPreferences = target.getSharedPreferences(prefix + name, mode)
        }
    }

    @Test
    fun failedAndroidWritesCanRestoreEveryStoreAndOptionalField() {
        val context = isolatedContext()
        OpenLessMigrationState.initialize(context, false)
        context.getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE).edit().putString("unrelated", "keep").commit()
        val capture = JSONObject(OpenLessMigrationState.capture(context))
        val before = capture.getJSONObject("value").toString()
        val next = JSONObject(before).apply {
            getJSONObject("preferences").put("locale", "zh-CN").put("themeMode", "dark")
            getJSONObject("strokeFrequency").put("new-word", "1,2")
            getJSONObject("pinyinLearnedPhrases").put("xx", "new-word,3")
        }.toString()
        for (stage in listOf("preferences", "strokeFrequency", "pinyinLearnedPhrases")) {
            val lease = OpenLessMigrationState.acquire(JSONObject(OpenLessMigrationState.capture(context)).getString("revision"))
            try {
                val result = runCatching {
                    OpenLessMigrationState.restore(lease) {
                        OpenLessSettingsExport.replacePortable(context, next) { if (it == stage) error("injected write failure") }
                    }
                }
                assertTrue(result.isFailure)
                OpenLessMigrationState.replace(context, lease, before)
                assertEquals(before, OpenLessMigrationState.captureLocked(context, lease))
                assertFalse(context.getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE).contains("locale"))
                assertEquals("keep", context.getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE).getString("unrelated", null))
            } finally { OpenLessMigrationState.finish(lease, false) }
        }
    }

    @Test
    fun writerFenceDefersLearningRejectsSettingsAndDropsOldMirrors() {
        val context = isolatedContext()
        OpenLessMigrationState.initialize(context, false)
        val captured = JSONObject(OpenLessMigrationState.capture(context))
        val oldMirror = OpenLessMigrationState.mirrorRevision()!!
        val lease = OpenLessMigrationState.acquire(captured.getString("revision"))
        try {
            assertFalse(OpenLessMigrationState.settings { error("must not run") })
            StrokeUserFrequency(context).record("test", "word")
            assertEquals(0, JSONObject(OpenLessMigrationState.captureLocked(context, lease)).getJSONObject("strokeFrequency").length())
            OpenLessMigrationState.replace(context, lease, captured.getJSONObject("value").toString())
        } finally { OpenLessMigrationState.finish(lease, false) }
        assertEquals(1, JSONObject(OpenLessMigrationState.capture(context)).getJSONObject("value").getJSONObject("strokeFrequency").length())
        var oldMirrorApplied = false
        OpenLessMigrationState.mirror(oldMirror) { oldMirrorApplied = true }
        assertFalse(oldMirrorApplied)
        assertTrue(runCatching { OpenLessMigrationState.acquire(captured.getString("revision")) }.isFailure)
    }

    @Test
    fun failedRecoveryKeepsWritersFencedUntilRecoveryFinishes() {
        val context = isolatedContext()
        OpenLessMigrationState.initialize(context, false)
        val snapshot = JSONObject(OpenLessMigrationState.capture(context))
        val lease = OpenLessMigrationState.acquire(snapshot.getString("revision"))
        OpenLessMigrationState.finish(lease, true)
        assertFalse(OpenLessMigrationState.settings { error("must not run") })
        val recovery = OpenLessMigrationState.acquire("")
        try { OpenLessMigrationState.replace(context, recovery, snapshot.getJSONObject("value").toString()) }
        finally { OpenLessMigrationState.finish(recovery, false) }
        assertTrue(OpenLessMigrationState.settings {})
    }

    @Test
    fun portableSchemaDoesNotCoerceWrongTypesOrAcceptOutdatedLearning() {
        val context = isolatedContext()
        OpenLessMigrationState.initialize(context, false)
        val valid = JSONObject(OpenLessMigrationState.capture(context)).getJSONObject("value").toString()
        assertTrue(OpenLessSettingsExport.validatePortable(valid))
        for ((key, value) in listOf("hapticEnabled" to "true", "hapticAmplitude" to 0, "hapticAmplitude" to 1.5, "locale" to 1, "themeMode" to "invalid")) {
            val invalid = JSONObject(valid).apply { getJSONObject("preferences").put(key, value) }
            assertFalse(OpenLessSettingsExport.validatePortable(invalid.toString()))
        }
        assertFalse(OpenLessSettingsExport.validatePortable(JSONObject(valid).put("learningPrivacyVersion", 0).toString()))
    }

    @Test
    fun legacyPinyinBackupRoundTripsUnderTheMigrationFence() {
        val context = isolatedContext()
        OpenLessMigrationState.initialize(context, false)
        // Complete the privacy cleanup before seeding a compatible legacy backup.
        OpenLessMigrationState.capture(context)
        val backup = LegacyPinyinBackup(context)
        backup.importAll(mapOf("before" to "旧词,2"))
        val before = JSONObject(OpenLessMigrationState.capture(context))
        assertEquals("旧词,2", before.getJSONObject("value").getJSONObject("pinyinLearnedPhrases").getString("before"))
        val next = JSONObject(before.getJSONObject("value").toString()).apply {
            put("pinyinLearnedPhrases", JSONObject().put("after", "新词,3"))
        }
        val lease = OpenLessMigrationState.acquire(before.getString("revision"))
        try {
            assertTrue(runCatching { backup.importAll(mapOf("unexpected" to "blocked,1")) }.isFailure)
            assertEquals(mapOf("before" to "旧词,2"), backup.exportAll())
            OpenLessMigrationState.replace(context, lease, next.toString())
            assertEquals(mapOf("after" to "新词,3"), backup.exportAll())
            OpenLessMigrationState.replace(context, lease, before.getJSONObject("value").toString())
            assertEquals(mapOf("before" to "旧词,2"), backup.exportAll())
        } finally { OpenLessMigrationState.finish(lease, false) }
    }

    @Test
    fun appContextIsAvailableAndSharePolicyIsRedacted() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        assertEquals("com.openless.app", context.packageName)
        assertTrue(OpenLessMigrationBundlePolicy.validateExport("share_redacted", false, null))
    }
}
