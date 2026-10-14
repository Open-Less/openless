package com.openless.app

/**
 * Dictation undo/replace may delete only when [expected] is exactly the text
 * immediately before the cursor. A length-only delete erases neighboring text
 * once the user backspaces the utterance away or moves the cursor into it.
 */
internal object DictationSpan {
    fun matchesBeforeCursor(beforeCursor: CharSequence?, expected: String): Boolean {
        if (expected.isEmpty()) return false
        return beforeCursor?.toString() == expected
    }

    /** Finds a surviving prefix at the cursor without repeated editor queries or substring copies. */
    fun hasRemainingPrefix(beforeCursor: CharSequence, expected: String): Boolean {
        if (beforeCursor.isEmpty() || expected.isEmpty()) return false
        val prefixes = IntArray(expected.length)
        var matched = 0
        for (index in 1 until expected.length) {
            while (matched > 0 && expected[index] != expected[matched]) {
                matched = prefixes[matched - 1]
            }
            if (expected[index] == expected[matched]) matched++
            prefixes[index] = matched
        }
        matched = 0
        for (character in beforeCursor) {
            while (matched > 0 && (matched == expected.length || character != expected[matched])) {
                matched = prefixes[matched - 1]
            }
            if (character == expected[matched]) matched++
        }
        return matched > 0
    }
}
