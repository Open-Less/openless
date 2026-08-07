package com.openless.app

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

    /**
     * Mirrors Rust [normalize_component_key]: expands `pkg/.Class` to `pkg/pkg.Class`.
     */
    internal fun normalizeComponentKey(component: String): String? {
        val trimmed = component.trim()
        val slash = trimmed.indexOf('/')
        if (slash <= 0 || slash == trimmed.lastIndex) {
            return null
        }
        val packageName = trimmed.substring(0, slash).trim()
        val className = trimmed.substring(slash + 1).trim()
        if (packageName.isEmpty() || className.isEmpty()) {
            return null
        }
        if (className.any { it.isWhitespace() || it == '\n' || it == '\r' }) {
            return null
        }
        if (!isValidAndroidPackageName(packageName)) {
            return null
        }
        val fullClassName = if (className.startsWith(".")) {
            packageName + className
        } else {
            className
        }
        if (fullClassName.any { it.isWhitespace() || it == '\n' || it == '\r' || it == '/' }) {
            return null
        }
        return "$packageName/$fullClassName"
    }

    internal fun componentIdsEqual(left: String, right: String): Boolean {
        val leftKey = normalizeComponentKey(left)
        val rightKey = normalizeComponentKey(right)
        if (leftKey != null && rightKey != null) {
            return leftKey == rightKey
        }
        return left.trim() == right.trim()
    }

    internal fun enabledListContains(services: String, targetComponent: String): Boolean {
        return parseServiceEntries(services).any { componentIdsEqual(it, targetComponent) }
    }

    private fun isValidAndroidPackageName(packageName: String): Boolean {
        if (packageName.isEmpty()) return false
        val segments = packageName.split('.')
        if (segments.isEmpty() || segments[0].isEmpty()) return false
        if (!segments[0][0].isLetter()) return false
        return segments.all { segment ->
            segment.isNotEmpty() &&
                segment[0].isLetter() &&
                segment.all { ch -> ch.isLetterOrDigit() || ch == '_' }
        }
    }
}
