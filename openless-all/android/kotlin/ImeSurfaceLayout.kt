package com.openless.app

/** Pixel-only sizing policy. Long drafts scroll instead of covering the keys. */
internal data class ImeSurfaceSize(val keyboard: Int, val draft: Int) {
    val total: Int get() = keyboard + draft
}

internal object ImeSurfaceLayout {
    fun resolve(
        preferredKeyboard: Int,
        desiredDraft: Int,
        available: Int,
        minimumKeyboard: Int,
    ): ImeSurfaceSize {
        val limit = available.coerceAtLeast(0)
        val preferred = preferredKeyboard.coerceAtLeast(0).coerceAtMost(limit)
        val reservedKeys = minimumKeyboard.coerceAtLeast(0).coerceAtMost(preferred)
        val draft = desiredDraft.coerceAtLeast(0).coerceAtMost(limit - reservedKeys)
        val keyboard = preferred.coerceAtMost(limit - draft)
        return ImeSurfaceSize(keyboard, draft)
    }
}
