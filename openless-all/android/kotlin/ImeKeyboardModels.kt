package com.openless.app

// 纯 Kotlin 键盘布局与主题模型：不依赖 Android，不持有 Service/View 引用，
// 仅供 IME 展示层消费，可在 JVM 单测中直接断言。
//
// 约定（未经真机验证，接线时需按实际交互确认）：
// - `rows()` 只返回真实按键；字母第二行两侧各 0.5 键宽的留白由 View 侧加 inset，
//   模型不生成虚假 spacer key。
// - 层切换动作按"相对当前层"解释，由状态机保证结果唯一：
//   ToggleNumbers: LETTERS→NUMBERS、NUMBERS/SYMBOLS→LETTERS（底行 ABC）
//   ToggleSymbols: NUMBERS→SYMBOLS、SYMBOLS→NUMBERS（仅数字/符号层）
//   底行键在 LETTERS 显示 "123"，其余层显示 "ABC"；第三行切换键在 NUMBERS
//   显示 "#+="、在 SYMBOLS 显示 "123"（与现有键盘一致）。
// - 语言键为显式按钮，标签表示**当前**输入语言（拼音 "中" / 英文 "EN"），
//   点击切换；不再依赖空格长按。
// - `auxiliary` 标记功能键（Shift/Backspace/层切换/语言），供 View 选择辅助色。
// - 字母键 `action.value` 已按 `letterCase` 应用大小写；`label` 与之一致。

internal enum class ImeKeyboardLayer { LETTERS, NUMBERS, SYMBOLS }

internal enum class ImeLetterCase { LOWER, SHIFT, CAPS_LOCK }

internal enum class ImeTypingLanguage { ENGLISH, PINYIN }

internal sealed class ImeKeyAction {
    data class Text(val value: String) : ImeKeyAction()
    data object Shift : ImeKeyAction()
    data object Backspace : ImeKeyAction()
    data object Space : ImeKeyAction()
    data object Language : ImeKeyAction()
    data object ToggleNumbers : ImeKeyAction()
    data object ToggleSymbols : ImeKeyAction()
    data object Enter : ImeKeyAction()
}

internal data class ImeKeySpec(
    val id: String,
    val label: String,
    val action: ImeKeyAction,
    val weight: Float = 1f,
    val secondary: String? = null,
    val auxiliary: Boolean = false,
)

internal object ImeKeyboardLayouts {
    private const val SHIFT_WEIGHT = 1.5f
    private const val BACKSPACE_WEIGHT = 1.5f
    private const val ROW_TOGGLE_WEIGHT = 1.5f
    private const val BOTTOM_MODE_WEIGHT = 1.3f
    private const val LANGUAGE_WEIGHT = 1.1f
    private const val BOTTOM_PUNCT_WEIGHT = 0.9f
    private const val SPACE_WEIGHT = 3.5f
    private const val SPACE_WIDE_WEIGHT = 4.6f
    private const val ENTER_WEIGHT = 1.8f

    private val LETTER_ROW_1 = listOf("q", "w", "e", "r", "t", "y", "u", "i", "o", "p")
    private val LETTER_ROW_1_SECONDARY = listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "0")
    private val LETTER_ROW_2 = listOf("a", "s", "d", "f", "g", "h", "j", "k", "l")
    private val LETTER_ROW_2_SECONDARY = listOf("~", "@", "#", "!", "%", "&", "*", "(", ")")
    private val LETTER_ROW_3 = listOf("z", "x", "c", "v", "b", "n", "m")
    private val LETTER_ROW_3_SECONDARY = listOf("'", "/", "-", "_", ":", ";", "?")

    // 数字/符号层沿用现有键盘排布。
    private val NUMBER_ROW_1 = listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "0")
    private val NUMBER_ROW_2 = listOf("-", "/", ":", ";", "(", ")", "$", "&", "@", "\"")
    private val NUMBER_ROW_3 = listOf(".", ",", "?", "!", "'")
    private val SYMBOL_ROW_1 = listOf("[", "]", "{", "}", "#", "%", "^", "*", "+", "=")
    private val SYMBOL_ROW_2 = listOf("_", "\\", "|", "~", "<", ">", "€", "£", "¥", "•")
    private val SYMBOL_ROW_3 = listOf(".", ",", "?", "!", "'")

    fun rows(
        layer: ImeKeyboardLayer,
        language: ImeTypingLanguage,
        letterCase: ImeLetterCase,
        enterLabel: String,
    ): List<List<ImeKeySpec>> {
        val content = when (layer) {
            ImeKeyboardLayer.LETTERS -> lettersRows(letterCase)
            ImeKeyboardLayer.NUMBERS -> numbersRows()
            ImeKeyboardLayer.SYMBOLS -> symbolsRows()
        }
        return content + listOf(bottomRow(layer, language, enterLabel))
    }

    private fun lettersRows(letterCase: ImeLetterCase): List<List<ImeKeySpec>> {
        val upper = letterCase != ImeLetterCase.LOWER
        val row1 = LETTER_ROW_1.mapIndexed { index, key ->
            letterKey("letters_r1_c${index + 1}", key, upper, LETTER_ROW_1_SECONDARY[index])
        }
        val row2 = LETTER_ROW_2.mapIndexed { index, key ->
            letterKey("letters_r2_c${index + 1}", key, upper, LETTER_ROW_2_SECONDARY[index])
        }
        val row3 = buildList {
            add(
                ImeKeySpec(
                    id = "shift",
                    label = if (letterCase == ImeLetterCase.CAPS_LOCK) "⇪" else "⇧",
                    action = ImeKeyAction.Shift,
                    weight = SHIFT_WEIGHT,
                    auxiliary = true,
                ),
            )
            LETTER_ROW_3.forEachIndexed { index, key ->
                add(letterKey("letters_r3_c${index + 1}", key, upper, LETTER_ROW_3_SECONDARY[index]))
            }
            add(backspaceKey())
        }
        return listOf(row1, row2, row3)
    }

    private fun numbersRows(): List<List<ImeKeySpec>> {
        val row1 = NUMBER_ROW_1.mapIndexed { index, value ->
            ImeKeySpec(id = "numbers_r1_c${index + 1}", label = value, action = ImeKeyAction.Text(value))
        }
        val row2 = NUMBER_ROW_2.mapIndexed { index, value ->
            ImeKeySpec(id = "numbers_r2_c${index + 1}", label = value, action = ImeKeyAction.Text(value))
        }
        val row3 = buildList {
            add(
                ImeKeySpec(
                    id = "mode_symbols",
                    label = "#+=",
                    action = ImeKeyAction.ToggleSymbols,
                    weight = ROW_TOGGLE_WEIGHT,
                    auxiliary = true,
                ),
            )
            addAll(numberActionKeys())
            add(backspaceKey())
        }
        return listOf(row1, row2, row3)
    }

    private fun symbolsRows(): List<List<ImeKeySpec>> {
        val row1 = SYMBOL_ROW_1.mapIndexed { index, value ->
            ImeKeySpec(id = "symbols_r1_c${index + 1}", label = value, action = ImeKeyAction.Text(value))
        }
        val row2 = SYMBOL_ROW_2.mapIndexed { index, value ->
            ImeKeySpec(id = "symbols_r2_c${index + 1}", label = value, action = ImeKeyAction.Text(value))
        }
        val row3 = buildList {
            add(
                ImeKeySpec(
                    id = "mode_numbers_row3",
                    label = "123",
                    action = ImeKeyAction.ToggleSymbols,
                    weight = ROW_TOGGLE_WEIGHT,
                    auxiliary = true,
                ),
            )
            addAll(numberActionKeys())
            add(backspaceKey())
        }
        return listOf(row1, row2, row3)
    }

    /** 数字/符号层第三行的 `. , ? ! '`，两层共用同一排布。 */
    private fun numberActionKeys(): List<ImeKeySpec> = NUMBER_ROW_3.mapIndexed { index, value ->
        ImeKeySpec(
            id = "punct_r3_c${index + 1}",
            label = value,
            action = ImeKeyAction.Text(value),
        )
    }

    private fun bottomRow(
        layer: ImeKeyboardLayer,
        language: ImeTypingLanguage,
        enterLabel: String,
    ): List<ImeKeySpec> = buildList {
        add(
            ImeKeySpec(
                id = "mode_numbers",
                // 底行在 LETTERS 显示 123；数字/符号层显示 ABC 以回到字母层。
                label = if (layer == ImeKeyboardLayer.LETTERS) "123" else "ABC",
                action = ImeKeyAction.ToggleNumbers,
                weight = BOTTOM_MODE_WEIGHT,
                auxiliary = true,
            ),
        )
        add(
            ImeKeySpec(
                id = "language",
                // 标签表示当前输入语言；点击切换（显式按钮，不依赖空格长按）。
                label = if (language == ImeTypingLanguage.PINYIN) "中" else "EN",
                action = ImeKeyAction.Language,
                weight = LANGUAGE_WEIGHT,
                auxiliary = true,
            ),
        )
        if (layer == ImeKeyboardLayer.LETTERS) {
            val comma = if (language == ImeTypingLanguage.PINYIN) "，" else ","
            add(
                ImeKeySpec(
                    id = "bottom_comma",
                    label = comma,
                    action = ImeKeyAction.Text(comma),
                    weight = BOTTOM_PUNCT_WEIGHT,
                    auxiliary = true,
                ),
            )
        }
        add(
            ImeKeySpec(
                id = "space",
                label = "Space",
                action = ImeKeyAction.Space,
                weight = if (layer == ImeKeyboardLayer.LETTERS) SPACE_WEIGHT else SPACE_WIDE_WEIGHT,
            ),
        )
        if (layer == ImeKeyboardLayer.LETTERS) {
            val period = if (language == ImeTypingLanguage.PINYIN) "。" else "."
            add(
                ImeKeySpec(
                    id = "bottom_period",
                    label = period,
                    action = ImeKeyAction.Text(period),
                    weight = BOTTOM_PUNCT_WEIGHT,
                    auxiliary = true,
                ),
            )
        }
        add(
            ImeKeySpec(
                id = "enter",
                label = enterLabel,
                action = ImeKeyAction.Enter,
                weight = ENTER_WEIGHT,
            ),
        )
    }

    private fun letterKey(
        id: String,
        key: String,
        upper: Boolean,
        secondary: String?,
    ): ImeKeySpec {
        val value = if (upper) key.uppercase() else key
        return ImeKeySpec(
            id = id,
            label = value,
            action = ImeKeyAction.Text(value),
            secondary = secondary,
        )
    }

    private fun backspaceKey(): ImeKeySpec = ImeKeySpec(
        id = "backspace",
        label = "⌫",
        action = ImeKeyAction.Backspace,
        weight = BACKSPACE_WEIGHT,
        auxiliary = true,
    )
}

/**
 * 键盘语义色（ARGB）。参照 iOS 键盘的语义分层，accent 为蓝色强调而非整片红色；
 * error 仅用于错误态。所有值不透明。
 */
internal data class ImePalette(
    val surface: Int,
    val key: Int,
    val auxiliary: Int,
    val accent: Int,
    val text: Int,
    val secondaryText: Int,
    val separator: Int,
    val pressed: Int,
    val error: Int,
) {
    companion object {
        fun resolve(dark: Boolean): ImePalette = if (dark) {
            ImePalette(
                surface = 0xFF1C1C1EL.toInt(),
                key = 0xFF41454CL.toInt(),
                auxiliary = 0xFF2F333BL.toInt(),
                accent = 0xFF709EFFL.toInt(),
                text = 0xFFFFFFFFL.toInt(),
                secondaryText = 0xFF9FA5AEL.toInt(),
                separator = 0xFF3A3A3CL.toInt(),
                pressed = 0xFF5A6069L.toInt(),
                error = 0xFFFF453AL.toInt(),
            )
        } else {
            ImePalette(
                surface = 0xFFD5D8DEL.toInt(),
                key = 0xFFFFFFFFL.toInt(),
                auxiliary = 0xFFD1D9E3L.toInt(),
                accent = 0xFF2B5EEDL.toInt(),
                text = 0xFF000000L.toInt(),
                secondaryText = 0xFF6B7280L.toInt(),
                separator = 0xFFB9BEC7L.toInt(),
                pressed = 0xFFE5E7EBL.toInt(),
                error = 0xFFD70015L.toInt(),
            )
        }
    }
}
