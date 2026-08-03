package com.openless.app

import android.view.accessibility.AccessibilityNodeInfo

/**
 * Pure helpers for validating editable focus targets (unit-testable without a live service).
 */
internal object OpenLessAccessibilityTarget {
    fun passesEditableFocusChecks(
        isEditable: Boolean,
        isFocused: Boolean,
        nodePackage: String?,
        activePackage: String?,
    ): Boolean {
        if (!isEditable || !isFocused) return false
        if (nodePackage.isNullOrEmpty()) return false
        if (activePackage.isNullOrEmpty()) return false
        return nodePackage == activePackage
    }

    fun passesWindowChecks(cachedWindowId: Int, activeWindowId: Int): Boolean {
        if (cachedWindowId < 0 || activeWindowId < 0) return false
        return cachedWindowId == activeWindowId
    }

    /**
     * Limited cache validation without tree walks or pseudo node identity.
     * Caller must prefer [AccessibilityNodeInfo.findFocus] first.
     */
    fun isValidCachedEditable(
        cached: AccessibilityNodeInfo,
        activeRoot: AccessibilityNodeInfo,
    ): Boolean {
        if (!cached.refresh()) return false
        val activePackage = activeRoot.packageName?.toString()
        if (!passesEditableFocusChecks(
                cached.isEditable,
                cached.isFocused,
                cached.packageName?.toString(),
                activePackage,
            )
        ) {
            return false
        }
        return passesWindowChecks(cached.windowId, activeRoot.windowId)
    }
}
