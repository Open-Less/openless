package com.openless.app

/**
 * Works out how to swap a text that was edited in the draft bar back into the
 * host field: where its original sits by now, and which edit replaces it.
 *
 * Pure on purpose. [OpenLessImeService] reads the host editor and carries the
 * plan out; everything that decides *what* to replace lives here, so it can be
 * tested without an input connection.
 */
internal object ImeDraftReplacement {
    sealed interface Plan

    /** The host's selection is the original; committing the new text replaces it. */
    object ReplaceSelection : Plan

    /** Select the absolute host range [start, end), then commit the new text. */
    data class SelectRange(val start: Int, val end: Int) : Plan

    /** Delete [before] chars before the host caret and [after] chars after it, then commit the new text. */
    data class DeleteAroundCaret(val before: Int, val after: Int) : Plan

    /**
     * @param original the text the draft was opened with.
     * @param before host text before the caret (the selection start).
     * @param selected the host's selected text, empty if there is none.
     * @param after host text after the selection end.
     * @param caretOffset absolute offset of the host caret (selection start),
     *   or null if the host doesn't report offsets.
     * @param hintStart absolute offset the original started at when the draft
     *   opened, or null if it wasn't known.
     * @return null when the original is no longer in the text around the
     *   caret, or can't be reached without absolute offsets — the host field
     *   must then be left alone.
     */
    fun plan(
        original: String,
        before: String,
        selected: String,
        after: String,
        caretOffset: Int?,
        hintStart: Int?,
    ): Plan? {
        if (original.isEmpty()) return null
        val around = before + selected + after
        // Index of the host caret inside `around`.
        val caret = before.length
        val wanted = if (hintStart != null && caretOffset != null) hintStart - (caretOffset - caret) else null
        // A remembered anchor must still identify the current selection. If it
        // does not, fall through to the anchored lookup instead of replacing
        // an unrelated duplicate that happens to be selected now.
        if (selected == original && (hintStart == null || caretOffset == hintStart)) return ReplaceSelection
        val start = locate(around, original, caret, wanted)
        if (start < 0) return null
        val end = start + original.length
        return when {
            caretOffset != null -> {
                val hostStart = caretOffset - caret + start
                SelectRange(hostStart, hostStart + original.length)
            }
            // Without absolute offsets only text touching the caret can be reached.
            selected.isEmpty() && caret in start..end -> DeleteAroundCaret(caret - start, end - caret)
            else -> null
        }
    }

    /**
     * Index in [around] of the occurrence of [original] to replace, or -1 if
     * there is none. When an absolute target is known, only that exact
     * occurrence is safe: choosing a nearby duplicate could replace unrelated
     * text after the original was edited. Without offsets, choose the
     * occurrence closest to [caret] (an occurrence the caret is inside or
     * touching counts as distance 0). On a tie the later occurrence wins —
     * text that was just dictated ends at the caret.
     */
    fun locate(around: String, original: String, caret: Int, wanted: Int?): Int {
        if (original.isEmpty()) return -1
        if (wanted != null) {
            if (wanted < 0 || wanted + original.length > around.length) return -1
            return if (around.regionMatches(wanted, original, 0, original.length)) wanted else -1
        }

        var best = -1
        var bestDistance = Int.MAX_VALUE
        var index = around.indexOf(original)
        while (index >= 0) {
            val end = index + original.length
            val distance = when {
                caret < index -> index - caret
                caret > end -> caret - end
                else -> 0
            }
            if (distance <= bestDistance) {
                bestDistance = distance
                best = index
            }
            index = around.indexOf(original, index + 1)
        }
        return best
    }
}
