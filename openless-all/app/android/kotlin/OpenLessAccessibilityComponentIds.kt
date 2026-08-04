package com.openless.app

import android.content.ComponentName

/**
 * Normalizes Android accessibility service component ids for comparison.
 * Settings.Secure may store short forms (`pkg/.Class`) while callers often use full class names.
 */
internal object OpenLessAccessibilityComponentIds {
    internal fun parseServiceEntries(raw: String?): LinkedHashSet<String> {
        val entries = LinkedHashSet<String>()
        raw
            ?.split(':')
            ?.map { it.trim() }
            ?.filter { it.isNotEmpty() && it != "null" }
            ?.forEach { entries.add(it) }
        return entries
    }

    internal fun componentIdsEqual(left: String, right: String): Boolean {
        val leftComponent = ComponentName.unflattenFromString(left.trim())
        val rightComponent = ComponentName.unflattenFromString(right.trim())
        if (leftComponent != null && rightComponent != null) {
            return leftComponent == rightComponent
        }
        return left.trim() == right.trim()
    }

    internal fun enabledListContains(services: String, targetComponent: String): Boolean {
        return parseServiceEntries(services).any { componentIdsEqual(it, targetComponent) }
    }
}
