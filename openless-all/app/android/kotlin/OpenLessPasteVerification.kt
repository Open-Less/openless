package com.openless.app

/**
 * Pure helpers for verifying accessibility paste actually changed editor text.
 */
internal object OpenLessPasteVerification {
    fun pasteAppearsApplied(
        beforeText: String,
        afterText: String,
        clipboardText: String,
    ): Boolean {
        if (clipboardText.isEmpty()) return false
        if (afterText.contains(clipboardText)) return true
        return afterText.length > beforeText.length && afterText.endsWith(clipboardText)
    }
}
