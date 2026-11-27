package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLessMigrationBundleContractTest {
    @Test
    fun shareModeNeverCarriesCredentials() {
        assertTrue(OpenLessMigrationBundlePolicy.validateExport("share_redacted", false, null))
        assertFalse(OpenLessMigrationBundlePolicy.validateExport("share_redacted", true, "secret"))
        assertFalse(OpenLessMigrationBundlePolicy.validateExport("share_redacted", false, "secret"))
    }

    @Test
    fun personalCredentialsRequirePassword() {
        assertFalse(OpenLessMigrationBundlePolicy.validateExport("personal_migration", true, null))
        assertFalse(OpenLessMigrationBundlePolicy.validateExport("personal_migration", true, ""))
        assertTrue(OpenLessMigrationBundlePolicy.validateExport("personal_migration", true, "password"))
        assertTrue(OpenLessMigrationBundlePolicy.validateExport("personal_migration", false, null))
    }

    @Test
    fun canonicalizesRegionalLocalesForBundleSchema() {
        assertEquals("en", OpenLessMigrationBundlePolicy.canonicalLocale("en-US"))
        assertEquals("en", OpenLessMigrationBundlePolicy.canonicalLocale("en-us"))
        assertEquals("zh-TW", OpenLessMigrationBundlePolicy.canonicalLocale("zh-HK"))
        assertEquals("system", OpenLessMigrationBundlePolicy.canonicalLocale("xx-ZZ"))
    }

    @Test
    fun unknownModeIsRejected() {
        assertFalse(OpenLessMigrationBundlePolicy.validateExport("legacy_json", false, null))
    }
}
