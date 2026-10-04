package com.openless.app

/**
 * Spoken edit replacement stays armed after the edit panel closes so the
 * in-flight result can still replace the original span. That arming must not
 * survive a failed session or a later tap on the main mic: both look like a
 * fresh dictation, and [OpenLessImeService] would otherwise delete by the old
 * span's length.
 */
internal data class PendingEditReplacement(
    val awaiting: Boolean,
    val panelVisible: Boolean,
    val original: String?,
    val replacesWhole: Boolean,
    val addToDictionary: Boolean,
) {
    fun releaseIfPanelDismissed(): PendingEditReplacement {
        if (!awaiting || panelVisible) return this
        return copy(
            awaiting = false,
            original = null,
            replacesWhole = false,
            addToDictionary = false,
        )
    }
}
