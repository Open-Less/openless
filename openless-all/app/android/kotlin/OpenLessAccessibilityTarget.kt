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

    fun hasPasteOrSetTextAction(actions: List<AccessibilityNodeInfo.AccessibilityAction>): Boolean {
        return actions.any { action ->
            action.id == AccessibilityNodeInfo.ACTION_PASTE ||
                action.id == AccessibilityNodeInfo.ACTION_SET_TEXT
        }
    }

    fun isPasteTargetClass(className: String?): Boolean {
        if (className.isNullOrEmpty()) return false
        return className.endsWith("EditText") ||
            className.endsWith("AutoCompleteTextView") ||
            className.contains("WebView")
    }

    fun isPasteTarget(
        isEditable: Boolean,
        isPassword: Boolean,
        className: String?,
        actions: List<AccessibilityNodeInfo.AccessibilityAction>,
    ): Boolean {
        if (isPassword) return false
        if (isEditable) return true
        if (isPasteTargetClass(className)) return true
        return hasPasteOrSetTextAction(actions)
    }

    fun isPasteTarget(node: AccessibilityNodeInfo): Boolean {
        return isPasteTarget(
            isEditable = node.isEditable,
            isPassword = node.isPassword,
            className = node.className?.toString(),
            actions = node.actionList,
        )
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
        if (!isPasteTarget(cached)) return false
        val activePackage = activeRoot.packageName?.toString()
        val nodePackage = cached.packageName?.toString()
        if (nodePackage.isNullOrEmpty() || activePackage.isNullOrEmpty()) return false
        if (nodePackage != activePackage) return false
        return passesWindowChecks(cached.windowId, activeRoot.windowId)
    }
}
