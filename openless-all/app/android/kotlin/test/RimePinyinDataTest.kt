package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class RimePinyinDataTest {
    private val sep = "\u0001"

    @Test
    fun fuzzyPatchAppendsTheSelectedRulesInGroupOrder() {
        assertNull(RimeFuzzyPinyin.customYaml(emptySet()))
        assertNull(RimeFuzzyPinyin.customYaml(setOf("unknown")))
        val yaml = RimeFuzzyPinyin.customYaml(setOf("ing", "zcs"))!!
        assertEquals(
            listOf(
                "patch:",
                "  \"speller/algebra/+\":",
                "    - derive/^([zcs])h/\$1/",
                "    - derive/^([zcs])([^h])/\$1h\$2/",
                "    - derive/ing\$/in/",
                "    - derive/in\$/ing/",
            ),
            yaml.lines().filter { it.isNotEmpty() && !it.startsWith("#") },
        )
    }

    @Test
    fun everyFuzzyGroupIsAPairOfDeriveRules() {
        assertEquals(RimeFuzzyPinyin.groups.size, RimeFuzzyPinyin.groups.map { it.id }.toSet().size)
        for (group in RimeFuzzyPinyin.groups) {
            assertEquals(2, group.rules.size)
            assertTrue(group.rules.all { it.startsWith("derive/") && it.count { c -> c == '/' } == 3 })
        }
    }

    private fun snapshot(name: String = "pinyin_simp", type: String = "userdb", vararg rows: String) = buildString {
        append("# Rime user dictionary\n")
        append("#@/db_name\t$name\n")
        append("#@/db_type\t$type\n")
        append("#@/tick\t42\n")
        rows.forEach { append(it).append('\n') }
    }

    @Test
    fun userDictionaryCountsLearnedRowsOnly() {
        val text = snapshot(
            rows = arrayOf("ni hao \t你好\tc=3 d=1.2 t=40", "shan chu \t删除\tc=-1 d=0.5 t=30", "zao ci \t造词\tc=1 d=0.9 t=41"),
        )
        assertEquals(2, RimeUserDictionaryFile.entryCount(text))
        assertEquals(1, RimeUserDictionaryFile.entryCount(text.replace("\n", "\r\n").replace("c=1 ", "c=0 ")))
    }

    @Test
    fun userDictionaryRejectsOtherDictionariesAndMalformedRows() {
        assertEquals(0, RimeUserDictionaryFile.entryCount(snapshot()))
        assertThrows(IllegalArgumentException::class.java) { RimeUserDictionaryFile.entryCount(snapshot(name = "luna_pinyin")) }
        assertThrows(IllegalArgumentException::class.java) { RimeUserDictionaryFile.entryCount(snapshot(type = "tabledb")) }
        assertThrows(IllegalArgumentException::class.java) { RimeUserDictionaryFile.entryCount("ni hao \t你好\tc=1\n") }
        assertThrows(IllegalArgumentException::class.java) { RimeUserDictionaryFile.entryCount(snapshot(rows = arrayOf("just text"))) }
        assertThrows(IllegalArgumentException::class.java) { RimeUserDictionaryFile.entryCount("{\"openlessSettingsExportVersion\":2}") }
    }

    @Test
    fun learningEntriesAreCheckedAndMergedByHigherCountAndLaterUse() {
        assertTrue(StrokeUserFrequency.isValidEntry("hsp${sep}王", "3,1700000000000"))
        assertFalse(StrokeUserFrequency.isValidEntry("hsp${sep}王", "0,1700000000000"))
        assertFalse(StrokeUserFrequency.isValidEntry("hsp${sep}王", "3"))
        assertFalse(StrokeUserFrequency.isValidEntry("hsp${sep}", "3,1"))
        assertFalse(StrokeUserFrequency.isValidEntry("hsp王", "3,1"))
        assertFalse(StrokeUserFrequency.isValidEntry("天".repeat(90) + "${sep}气", "3,1"))
        assertEquals("5,200", StrokeUserFrequency.mergeStamps("5,100", "2,200"))
        assertEquals("7,300", StrokeUserFrequency.mergeStamps("1,300", "7,50"))
    }

    @Test
    fun aFullStoreIsTrimmedInOneBatch() {
        assertEquals(4500, StrokeUserFrequency.trimTarget(5000))
        assertEquals(9000, StrokeUserFrequency.trimTarget(10_000))
    }

    @Test
    fun keyboardSettingsAcceptOnlyKnownKeysWithValidValues() {
        assertTrue(KeyboardDataFile.acceptsSetting("keyboardHeightDp", 300))
        assertFalse(KeyboardDataFile.acceptsSetting("keyboardHeightDp", 900))
        assertFalse(KeyboardDataFile.acceptsSetting("keyboardHeightDp", "300"))
        assertTrue(KeyboardDataFile.acceptsSetting("keyboardRaiseDp", 0))
        assertTrue(KeyboardDataFile.acceptsSetting("hapticAmplitude", 255))
        assertFalse(KeyboardDataFile.acceptsSetting("hapticAmplitude", 0))
        assertTrue(KeyboardDataFile.acceptsSetting("hapticDurationMs", 12))
        assertFalse(KeyboardDataFile.acceptsSetting("hapticDurationMs", 5000))
        assertTrue(KeyboardDataFile.acceptsSetting("cloudPinyinEnabled", false))
        assertFalse(KeyboardDataFile.acceptsSetting("cloudPinyinEnabled", 1))
        assertTrue(KeyboardDataFile.acceptsSetting("fuzzyPinyin", listOf("zcs", "ing")))
        assertTrue(KeyboardDataFile.acceptsSetting("fuzzyPinyin", emptyList<String>()))
        assertFalse(KeyboardDataFile.acceptsSetting("fuzzyPinyin", listOf("zcs", "bogus")))
        assertFalse(KeyboardDataFile.acceptsSetting("locale", "zh-CN"))
        assertFalse(KeyboardDataFile.acceptsSetting("key_cloud_note_webhook_token", "secret"))
    }

    @Test
    fun webhookAddressesMustBeBlankOrHttp() {
        assertTrue(KeyboardDataFile.acceptsWebhookUrl(""))
        assertTrue(KeyboardDataFile.acceptsWebhookUrl("https://example.com/capture_ingest.php"))
        assertFalse(KeyboardDataFile.acceptsWebhookUrl("file:///sdcard/notes"))
        assertFalse(KeyboardDataFile.acceptsWebhookUrl("javascript:alert(1)"))
        assertFalse(KeyboardDataFile.acceptsWebhookUrl("not a url"))
    }

    @Test
    fun everythingIsExportedButTheTokenIsNotImportedUnlessChosen() {
        assertEquals(KeyboardDataFile.Section.entries.toSet(), KeyboardDataFile.defaultExport)
        assertFalse(KeyboardDataFile.Section.CLOUD_NOTES in KeyboardDataFile.defaultImport)
        assertEquals(KeyboardDataFile.Section.entries.size - 1, KeyboardDataFile.defaultImport.size)
    }

    @Test
    fun strokeCodeKeysAreToldApartFromAssociationKeys() {
        assertTrue(StrokeUserFrequency.isStrokeCodeKey("hsp${sep}王"))
        assertTrue(StrokeUserFrequency.isStrokeCodeKey("h*z${sep}过"))
        assertFalse(StrokeUserFrequency.isStrokeCodeKey("天气${sep}天气预报"))
        assertFalse(StrokeUserFrequency.isStrokeCodeKey("test${sep}word"))
        assertFalse(StrokeUserFrequency.isStrokeCodeKey("${sep}空上下文"))
        assertFalse(StrokeUserFrequency.isStrokeCodeKey("hsp"))
    }
}
