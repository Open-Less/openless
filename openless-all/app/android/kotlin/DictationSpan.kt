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
}
