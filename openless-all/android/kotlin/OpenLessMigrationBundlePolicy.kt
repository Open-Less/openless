package com.openless.app

/** Pure Android-side guardrails mirrored by the Rust bundle policy. */
object OpenLessMigrationBundlePolicy {
    const val FORMAT = "openless.android.migration-bundle"
    const val MAX_BUNDLE_BYTES = 32 * 1024 * 1024

    fun validateExport(mode: String, includeCredentials: Boolean, password: String?): Boolean {
        if (mode != "share_redacted" && mode != "personal_migration") return false
        if (mode == "share_redacted" && includeCredentials) return false
        if (!includeCredentials) return password.isNullOrEmpty()
        return mode == "personal_migration" && !password.isNullOrEmpty()
    }

    fun credentialSummary(credentialsEncrypted: Boolean): String =
        if (credentialsEncrypted) "password_protected" else "not_included"

    fun canonicalLocale(value: String): String {
        val normalized = value.trim()
        if (normalized.equals("system", ignoreCase = true)) return "system"
        val locale = java.util.Locale.forLanguageTag(normalized.replace('_', '-'))
        return when (locale.language.lowercase(java.util.Locale.ROOT)) {
            "zh" -> if (
                locale.country.uppercase(java.util.Locale.ROOT) in setOf("TW", "HK", "MO") ||
                    locale.script.equals("Hant", ignoreCase = true)
            ) {
                "zh-TW"
            } else {
                "zh-CN"
            }
            "en", "ja", "ko", "es", "fr", "de" -> locale.language.lowercase(java.util.Locale.ROOT)
            else -> "system"
        }
    }
}
