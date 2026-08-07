package com.openless.app

import android.view.accessibility.AccessibilityNodeInfo

/**
 * Pure helpers for validating editable focus targets (unit-testable without a live service).
 */
internal object OpenLessAccessibilityTarget {
    private const val ACTION_PASTE_ID = 0x00008000
    private const val ACTION_SET_TEXT_ID = 0x00200000

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
        return hasPasteOrSetTextActionIds(actions.map { it.id })
    }

    fun hasPasteOrSetTextActionIds(actionIds: Iterable<Int>): Boolean {
        return actionIds.any { id ->
            id == ACTION_PASTE_ID || id == ACTION_SET_TEXT_ID
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
        actionIds: Iterable<Int>,
    ): Boolean {
        if (isPassword) return false
        if (isEditable) return true
        if (isPasteTargetClass(className)) return true
        return hasPasteOrSetTextActionIds(actionIds)
    }

    fun isPasteTarget(
        isEditable: Boolean,
        isPassword: Boolean,
        className: String?,
        actions: List<AccessibilityNodeInfo.AccessibilityAction>,
    ): Boolean {
        return isPasteTarget(isEditable, isPassword, className, actions.map { it.id })
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
