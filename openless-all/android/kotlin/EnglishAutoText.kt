package com.openless.app

import android.text.InputType

/**
 * The English keyboard's small typing conveniences — sentence-start
 * capitals, double-space full stop, punctuation pulling in the space a
 * candidate pick added — as pure decisions on the text around the caret.
 * [OpenLessImeService] reads the field and carries them out.
 */
internal object EnglishAutoText {
    /** Marks that take the place of a candidate's trailing space: "word " + "," becomes "word, ". */
    private val SPACE_SWALLOWING_PUNCTUATION = setOf(".", ",", "!", "?", ";", ":")
    private val SENTENCE_ENDS = setOf('.', '!', '?')

    /** How much text before the caret [isSentenceStart] needs to see. */
    const val SENTENCE_LOOKBEHIND = 64

    /**
     * True at the start of the field, of a line, or after a sentence-ending
     * mark followed by whitespace. [before] is up to [SENTENCE_LOOKBEHIND]
     * characters before the caret.
     */
    fun isSentenceStart(before: CharSequence): Boolean {
        if (before.isBlank()) return true
        val last = before.last()
        if (last == '\n') return true
        if (!last.isWhitespace()) return false
        val mark = before.trimEnd().lastOrNull() ?: return true
        return mark in SENTENCE_ENDS
    }

    /** Fields where a capital nobody asked for would be wrong: addresses, URLs, passwords, numbers. */
    fun allowsAutoCapitals(inputType: Int): Boolean {
        if (inputType and InputType.TYPE_MASK_CLASS != InputType.TYPE_CLASS_TEXT) return false
        return when (inputType and InputType.TYPE_MASK_VARIATION) {
            InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_URI,
            InputType.TYPE_TEXT_VARIATION_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD -> false
            else -> true
        }
    }

    /** Preserve the capitalization already typed when a completion replaces its prefix. */
    fun candidateText(word: String, typedPrefix: CharSequence, capsLock: Boolean): String = when {
        capsLock || (typedPrefix.length > 1 && typedPrefix.all { it.isUpperCase() }) -> word.uppercase()
        typedPrefix.firstOrNull()?.isUpperCase() == true -> word.replaceFirstChar { it.titlecase() }
        else -> word
    }

    /**
     * A second quick space turns "word " into "word. ". Only right after a
     * letter or digit and its single space — never after punctuation, a
     * second space or a line break.
     */
    fun doubleSpaceMakesFullStop(before: CharSequence): Boolean {
        if (before.length < 2 || before.last() != ' ') return false
        return before[before.length - 2].isLetterOrDigit()
    }

    fun swallowsAutoSpace(char: String): Boolean = char in SPACE_SWALLOWING_PUNCTUATION
}
