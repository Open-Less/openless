package com.openless.app

/**
 * Pure contract for a captured accessibility selection. The node itself remains
 * in the service; this value is only a short-lived target fingerprint.
 */
internal data class OpenLessAccessibilitySelection(
    val packageName: String,
    val windowId: Int,
    val selectionStart: Int,
    val selectionEnd: Int,
    val sourceText: String,
) {
    val normalizedStart: Int = minOf(selectionStart, selectionEnd)
    val normalizedEnd: Int = maxOf(selectionStart, selectionEnd)

    /** A captured editor field is valid even when the caret has no selection. */
    fun isFieldUsable(): Boolean =
        packageName.isNotBlank() &&
            windowId >= 0 &&
            normalizedStart >= 0 &&
            normalizedEnd >= normalizedStart &&
            normalizedEnd <= sourceText.length

    fun isUsable(): Boolean =
        isFieldUsable() && normalizedEnd > normalizedStart

    fun matches(
        sameNode: Boolean,
        packageName: String?,
        windowId: Int,
        currentText: String?,
        currentSelectionStart: Int,
        currentSelectionEnd: Int,
    ): Boolean {
        if (!sameNode || !isFieldUsable() || packageName != this.packageName || windowId != this.windowId) {
            return false
        }
        val text = currentText ?: return false
        if (text != sourceText) return false
        return minOf(currentSelectionStart, currentSelectionEnd) == normalizedStart &&
            maxOf(currentSelectionStart, currentSelectionEnd) == normalizedEnd
    }

    fun replaceIn(source: String, replacement: String): String? {
        if (!isUsable() || source != sourceText) return null
        return buildString(source.length - (normalizedEnd - normalizedStart) + replacement.length) {
            append(source, 0, normalizedStart)
            append(replacement)
            append(source, normalizedEnd, source.length)
        }
    }

    /** Replaces the captured selection, or the complete field when only a caret was captured. */
    fun replaceTarget(source: String, replacement: String): String? {
        if (!isFieldUsable() || source != sourceText) return null
        if (normalizedStart == normalizedEnd) return replacement
        return replaceIn(source, replacement)
    }
}
