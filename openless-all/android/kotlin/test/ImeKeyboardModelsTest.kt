package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 纯 JVM 断言：布局数据、层切换键、大小写、权重与主题不透明度。
 * 不涉及 Android 框架，也不验证真机渲染。
 */
class ImeKeyboardModelsTest {

    private fun rows(
        layer: ImeKeyboardLayer = ImeKeyboardLayer.LETTERS,
        language: ImeTypingLanguage = ImeTypingLanguage.ENGLISH,
        letterCase: ImeLetterCase = ImeLetterCase.LOWER,
        enterLabel: String = "Enter",
    ): List<List<ImeKeySpec>> = ImeKeyboardLayouts.rows(layer, language, letterCase, enterLabel)

    private fun allKeys(
        layer: ImeKeyboardLayer = ImeKeyboardLayer.LETTERS,
        language: ImeTypingLanguage = ImeTypingLanguage.ENGLISH,
        letterCase: ImeLetterCase = ImeLetterCase.LOWER,
    ): List<ImeKeySpec> = rows(layer, language, letterCase).flatten()

    private fun List<ImeKeySpec>.findByAction(action: ImeKeyAction): ImeKeySpec? =
        firstOrNull { it.action == action }

    private fun List<ImeKeySpec>.findByLabel(label: String): ImeKeySpec? =
        firstOrNull { it.label == label }

    private fun textValue(action: ImeKeyAction): String? = (action as? ImeKeyAction.Text)?.value

    // --- 结构 ---

    @Test
    fun lettersLayoutHasThreeTypingRowsPlusBottomRow() {
        val layout = rows()
        assertEquals(4, layout.size)
        assertEquals(10, layout[0].size)
        assertEquals(9, layout[1].size) // 两侧半键留白由 View 加 inset，模型不出 spacer
        assertEquals(9, layout[2].size) // Shift + 7 字母 + Backspace
        assertEquals(6, layout[3].size) // 123 / 中EN / , / Space / . / Enter
    }

    @Test
    fun lettersBottomRowMatchesLanguagePunctuation() {
        val zhBottom = rows(language = ImeTypingLanguage.PINYIN)[3]
        assertEquals(listOf("123", "中", "，", "Space", "。", "Enter"), zhBottom.map { it.label })
        val enBottom = rows(language = ImeTypingLanguage.ENGLISH)[3]
        assertEquals(listOf("123", "EN", ",", "Space", ".", "Enter"), enBottom.map { it.label })
        val numBottom = rows(layer = ImeKeyboardLayer.NUMBERS, language = ImeTypingLanguage.PINYIN)[3]
        assertEquals(listOf("ABC", "中", "Space", "Enter"), numBottom.map { it.label })
    }

    @Test
    fun lettersRowOneCarriesDigitSecondaries() {
        val row = rows()[0]
        assertEquals(listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "0"), row.map { it.secondary })
        assertEquals(listOf("q", "w", "e", "r", "t", "y", "u", "i", "o", "p"), row.map { it.label })
    }

    @Test
    fun lettersRowTwoAndThreeCarrySymbolSecondaries() {
        val secondaries = rows().drop(1)
        assertEquals(
            listOf("~", "@", "#", "!", "%", "&", "*", "(", ")"),
            secondaries[0].map { it.secondary },
        )
        assertEquals(
            listOf("'", "/", "-", "_", ":", ";", "?"),
            secondaries[1].filter { it.secondary != null }.map { it.secondary },
        )
    }

    @Test
    fun lettersRowThreeStartsWithShiftAndEndsWithBackspace() {
        val row = rows()[2]
        assertEquals(ImeKeyAction.Shift, row.first().action)
        assertEquals(ImeKeyAction.Backspace, row.last().action)
        assertTrue(row.first().auxiliary)
        assertTrue(row.last().auxiliary)
    }

    @Test
    fun numbersLayerMirrorsExistingLayout() {
        val layout = rows(layer = ImeKeyboardLayer.NUMBERS)
        assertEquals(4, layout.size)
        assertEquals(listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "0"), layout[0].map { it.label })
        assertEquals(
            listOf("-", "/", ":", ";", "(", ")", "$", "&", "@", "\""),
            layout[1].map { it.label },
        )
        assertEquals("#+=", layout[2].first().label)
        assertEquals(listOf(".", ",", "?", "!", "'"), layout[2].drop(1).dropLast(1).map { it.label })
        assertEquals(ImeKeyAction.Backspace, layout[2].last().action)
    }

    @Test
    fun symbolsLayerMirrorsExistingLayout() {
        val layout = rows(layer = ImeKeyboardLayer.SYMBOLS)
        assertEquals(listOf("[", "]", "{", "}", "#", "%", "^", "*", "+", "="), layout[0].map { it.label })
        assertEquals(
            listOf("_", "\\", "|", "~", "<", ">", "€", "£", "¥", "•"),
            layout[1].map { it.label },
        )
        assertEquals("123", layout[2].first().label)
    }

    // --- 大小写 ---

    @Test
    fun textValuesFollowLetterCase() {
        val lower = allKeys(letterCase = ImeLetterCase.LOWER).findByLabel("q")
        val shift = allKeys(letterCase = ImeLetterCase.SHIFT).findByLabel("Q")
        val caps = allKeys(letterCase = ImeLetterCase.CAPS_LOCK).findByLabel("Q")
        assertEquals("q", lower?.let { textValue(it.action) })
        assertEquals("Q", shift?.let { textValue(it.action) })
        assertEquals("Q", caps?.let { textValue(it.action) })
        assertEquals("Q", shift?.label)
        assertEquals("Q", caps?.label)
    }

    @Test
    fun letterCaseDoesNotAlterPunctuationOrSecondaries() {
        val shifted = allKeys(letterCase = ImeLetterCase.SHIFT)
        // 数字/符号只作为 secondary 存在，不因大小写变化。
        val q = shifted.findByLabel("Q")
        assertEquals("1", q?.secondary)
        val a = shifted.findByLabel("A")
        assertEquals("~", a?.secondary)

        val numbers = allKeys(layer = ImeKeyboardLayer.NUMBERS, letterCase = ImeLetterCase.SHIFT)
        assertEquals("1", numbers.findByLabel("1")?.let { textValue(it.action) })
    }

    // --- 层切换键 ---

    @Test
    fun bottomModeKeyTogglesNumbersAndLabelsReflectLayer() {
        val lettersBottom = rows(layer = ImeKeyboardLayer.LETTERS)[3].findByAction(ImeKeyAction.ToggleNumbers)
        assertEquals("123", lettersBottom?.label)
        assertEquals(1.3f, lettersBottom!!.weight, 0.0001f)

        for (layer in listOf(ImeKeyboardLayer.NUMBERS, ImeKeyboardLayer.SYMBOLS)) {
            val bottom = rows(layer = layer)[3].findByAction(ImeKeyAction.ToggleNumbers)
            assertEquals("ABC", bottom?.label)
            assertEquals("mode_numbers", bottom?.id)
        }
    }

    @Test
    fun rowThreeToggleSwitchesBetweenNumbersAndSymbols() {
        val numbersToggle = rows(layer = ImeKeyboardLayer.NUMBERS)[2].findByAction(ImeKeyAction.ToggleSymbols)
        assertEquals("#+=", numbersToggle?.label)
        assertEquals("mode_symbols", numbersToggle?.id)

        val symbolsToggle = rows(layer = ImeKeyboardLayer.SYMBOLS)[2].findByAction(ImeKeyAction.ToggleSymbols)
        assertEquals("123", symbolsToggle?.label)
        assertEquals("mode_numbers_row3", symbolsToggle?.id)
    }

    // --- 底行其余按键 ---

    @Test
    fun languageKeyIsExplicitAndShowsCurrentLanguage() {
        val pinyin = rows(language = ImeTypingLanguage.PINYIN)[3].findByAction(ImeKeyAction.Language)
        assertEquals("中", pinyin?.label)
        assertEquals(1.1f, pinyin!!.weight, 0.0001f)
        assertTrue(pinyin.auxiliary)

        val english = rows(language = ImeTypingLanguage.ENGLISH)[3].findByAction(ImeKeyAction.Language)
        assertEquals("EN", english?.label)
    }

    @Test
    fun spaceAndEnterArePresentWithSpecWeights() {
        val bottom = rows(enterLabel = "搜索")[3]
        val space = bottom.findByAction(ImeKeyAction.Space)
        assertEquals("Space", space?.label)
        assertEquals(3.5f, space!!.weight, 0.0001f)
        assertNull(space.secondary)

        val enter = bottom.findByAction(ImeKeyAction.Enter)
        assertEquals("搜索", enter?.label)
        assertEquals(1.8f, enter!!.weight, 0.0001f)
    }

    @Test
    fun everyLayoutHasExactlyOneSpaceEnterAndLanguageKey() {
        for (layer in ImeKeyboardLayer.entries) {
            for (language in ImeTypingLanguage.entries) {
                val keys = allKeys(layer = layer, language = language)
                assertEquals(1, keys.count { it.action == ImeKeyAction.Space })
                assertEquals(1, keys.count { it.action == ImeKeyAction.Enter })
                assertEquals(1, keys.count { it.action == ImeKeyAction.Language })
            }
        }
    }

    // --- 通用不变量 ---

    @Test
    fun idsAreUniqueAndStableAcrossEveryConfiguration() {
        for (layer in ImeKeyboardLayer.entries) {
            for (language in ImeTypingLanguage.entries) {
                for (letterCase in ImeLetterCase.entries) {
                    val ids = allKeys(layer = layer, language = language, letterCase = letterCase)
                        .map { it.id }
                    assertEquals(
                        "duplicate id in $layer/$language/$letterCase",
                        ids.size,
                        ids.toSet().size,
                    )
                    assertTrue("blank id in $layer/$language/$letterCase", ids.none { it.isBlank() })
                }
            }
        }
    }

    @Test
    fun allWeightsArePositive() {
        for (layer in ImeKeyboardLayer.entries) {
            for (letterCase in ImeLetterCase.entries) {
                allKeys(layer = layer, letterCase = letterCase).forEach { key ->
                    assertTrue("non-positive weight on ${key.id}", key.weight > 0f)
                    assertTrue("unexpectedly large weight on ${key.id}", key.weight <= 5f)
                }
            }
        }
    }

    @Test
    fun textActionsKeepTheirVisibleValue() {
        for (layer in ImeKeyboardLayer.entries) {
            for (letterCase in ImeLetterCase.entries) {
                allKeys(layer = layer, letterCase = letterCase).forEach { key ->
                    val value = textValue(key.action)
                    if (value != null) {
                        assertEquals("label/action mismatch on ${key.id}", key.label, value)
                    }
                }
            }
        }
    }

    @Test
    fun secondariesOnlyAppearOnLetterLayers() {
        for (layer in listOf(ImeKeyboardLayer.NUMBERS, ImeKeyboardLayer.SYMBOLS)) {
            assertTrue(allKeys(layer = layer).all { it.secondary == null })
        }
    }

    // --- 主题 ---

    @Test
    fun paletteIsOpaqueInBothThemes() {
        for (dark in listOf(true, false)) {
            val palette = ImePalette.resolve(dark)
            val colors = listOf(
                palette.surface, palette.key, palette.auxiliary, palette.accent,
                palette.text, palette.secondaryText, palette.separator,
                palette.pressed, palette.error,
            )
            colors.forEach { color ->
                assertEquals("alpha must be 0xFF for $color (dark=$dark)", 0xFF, (color ushr 24) and 0xFF)
            }
        }
    }

    @Test
    fun accentIsBlueWhileErrorStaysRed() {
        for (dark in listOf(true, false)) {
            val palette = ImePalette.resolve(dark)
            val accentBlue = palette.accent and 0xFF
            val accentRed = (palette.accent ushr 16) and 0xFF
            assertTrue("accent should lean blue (dark=$dark)", accentBlue > accentRed)

            val errorRed = (palette.error ushr 16) and 0xFF
            val errorBlue = palette.error and 0xFF
            assertTrue("error should lean red (dark=$dark)", errorRed > errorBlue)
        }
    }

    @Test
    fun darkAndLightPalettesAreDistinct() {
        val dark = ImePalette.resolve(true)
        val light = ImePalette.resolve(false)
        assertNotEquals(dark.surface, light.surface)
        assertNotEquals(dark.key, light.key)
        assertNotEquals(dark.auxiliary, light.auxiliary)
        assertNotEquals(dark.text, light.text)
        assertNotEquals(dark.separator, light.separator)
        assertNotEquals(dark.pressed, light.pressed)
    }

    @Test
    fun separatorAndKeyStayDistinguishable() {
        for (dark in listOf(true, false)) {
            val palette = ImePalette.resolve(dark)
            assertNotEquals("separator must differ from key surface", palette.key, palette.separator)
            assertNotEquals("pressed must differ from key surface", palette.key, palette.pressed)
        }
    }
}
