package com.openless.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class OpenLessMigrationBundleInstrumentedTest {
    @Test
    fun appContextIsAvailableAndSharePolicyIsRedacted() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        assertEquals("com.openless.app", context.packageName)
        assertTrue(OpenLessMigrationBundlePolicy.validateExport("share_redacted", false, null))
    }
}
