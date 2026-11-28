package com.openless.app

/** Android InputType class bits; flags/variations do not turn numeric fields into voice pages. */
internal object ImeEditorFieldPolicy {
    fun prefersNumeric(inputType: Int): Boolean = when (inputType and 0x0000000f) {
        0x2, 0x3, 0x4 -> true // TYPE_CLASS_NUMBER / PHONE / DATETIME
        else -> false
    }
}
