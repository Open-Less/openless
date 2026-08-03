package com.openless.app

import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLessShizukuBridgeTest {
    private val serviceComponent = "com.openless.app/com.openless.app.OpenLessAccessibilityService"
    private val thirdParty = "com.example/.OtherService"

    @Test
    fun mergePreservesThirdPartyServices() {
        val current = "$thirdParty:com.foo/.AnotherService"
        val merged = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(current, serviceComponent)
        assertTrue(merged.contains(thirdParty))
        assertTrue(merged.contains("com.foo/.AnotherService"))
        assertTrue(merged.contains(serviceComponent))
    }

    @Test
    fun mergeAppendsWhenEmpty() {
        val merged = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(null, serviceComponent)
        assertEquals(serviceComponent, merged)
    }

    @Test
    fun mergeDoesNotDuplicateOpenLess() {
        val current = serviceComponent
        val merged = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(current, serviceComponent)
        assertEquals(serviceComponent, merged)
    }

    @Test
    fun shellQuoteEscapesSingleQuotes() {
        val quoted = OpenLessShizukuBridge.shellQuote("a'b:c")
        assertEquals("'a'\\''b:c'", quoted)
    }

    @Test
    fun verifyReadbackAcceptsMergedComponent() {
        val readback = "$thirdParty:$serviceComponent"
        assertTrue(OpenLessShizukuBridge.verifyReadback(readback, serviceComponent))
    }

    @Test
    fun verifyReadbackPreservesOriginalServices() {
        val original = OpenLessShizukuBridge.parseServiceEntries(thirdParty)
        val readback = serviceComponent
        assertFalse(OpenLessShizukuBridge.verifyReadbackPreserves(readback, serviceComponent, original))
    }

    @Test
    fun verifyReadbackPreservesWhenAllOriginalServicesRemain() {
        val original = OpenLessShizukuBridge.parseServiceEntries("$thirdParty:com.foo/.AnotherService")
        val readback = "$thirdParty:com.foo/.AnotherService:$serviceComponent"
        assertTrue(OpenLessShizukuBridge.verifyReadbackPreserves(readback, serviceComponent, original))
    }

    @Test
    fun verifyReadbackRejectsMissingComponent() {
        assertFalse(OpenLessShizukuBridge.verifyReadback(thirdParty, serviceComponent))
        assertFalse(OpenLessShizukuBridge.verifyReadback("", serviceComponent))
        assertFalse(OpenLessShizukuBridge.verifyReadback("null", serviceComponent))
    }

    @Test
    fun validatesServiceComponentFormat() {
        assertTrue(OpenLessShizukuBridge.isValidServiceComponent(serviceComponent))
        assertFalse(OpenLessShizukuBridge.isValidServiceComponent("bad"))
        assertFalse(OpenLessShizukuBridge.isValidServiceComponent("com.foo/.Svc\n"))
    }

    @Test
    fun shouldRollbackEnabledOnlyAfterServicesRollbackSucceeded() {
        assertFalse(
            OpenLessShizukuBridge.shouldRollbackEnabledAfterServices(
                OpenLessShizukuBridge.ServicesRollbackResult.Conflict,
                "1",
                "0",
            ),
        )
        assertFalse(
            OpenLessShizukuBridge.shouldRollbackEnabledAfterServices(
                OpenLessShizukuBridge.ServicesRollbackResult.Restored,
                "1",
                "0",
            ),
        )
        assertFalse(
            OpenLessShizukuBridge.shouldRollbackEnabledAfterServices(
                OpenLessShizukuBridge.ServicesRollbackResult.AlreadyBaseline,
                "1",
                "0",
            ),
        )
        assertFalse(
            OpenLessShizukuBridge.shouldRollbackEnabledAfterServices(
                OpenLessShizukuBridge.ServicesRollbackResult.AlreadyBaseline,
                "1",
                "1",
            ),
        )
    }

    @Test
    fun unchangedServicesWriteRollbackRequiresCurrentBaselineMatch() {
        val baseline = serviceComponent
        val withThirdParty = "$serviceComponent:$thirdParty"
        assertEquals(
            OpenLessShizukuBridge.ServicesRollbackResult.AlreadyBaseline,
            OpenLessShizukuBridge.evaluateUnchangedServicesWriteRollback(baseline, baseline),
        )
        assertEquals(
            OpenLessShizukuBridge.ServicesRollbackResult.Conflict,
            OpenLessShizukuBridge.evaluateUnchangedServicesWriteRollback(withThirdParty, baseline),
        )
    }

    @Test
    fun requiresManualRecoveryWhenGlobalDisabledWithThirdPartyServices() {
        val snapshot = OpenLessShizukuBridge.AccessibilitySettingsSnapshot(
            "$thirdParty:$serviceComponent",
            "0",
        )
        assertTrue(OpenLessShizukuBridge.requiresManualRecovery(snapshot, serviceComponent))
        assertFalse(
            OpenLessShizukuBridge.requiresManualRecovery(
                OpenLessShizukuBridge.AccessibilitySettingsSnapshot(serviceComponent, "0"),
                serviceComponent,
            ),
        )
        assertFalse(
            OpenLessShizukuBridge.requiresManualRecovery(
                OpenLessShizukuBridge.AccessibilitySettingsSnapshot(thirdParty, "1"),
                serviceComponent,
            ),
        )
    }

    @Test
    fun recoveryFailureMessageKeyPrefersPartialRollbackWhenEnabledLeftOn() {
        val rollback = OpenLessShizukuBridge.RecoveryRollbackStatus(
            OpenLessShizukuBridge.ServicesRollbackResult.Restored,
            OpenLessShizukuBridge.EnabledRollbackResult.SkippedDueToServicesConflict,
        )
        assertEquals(
            "partial_rollback",
            OpenLessShizukuBridge.recoveryFailureMessageKey(
                rollback,
                wroteEnabled = true,
                baselineEnabled = "0",
                failureCause = "readback_failed",
            ),
        )
        assertEquals(
            "readback_failed",
            OpenLessShizukuBridge.recoveryFailureMessageKey(
                rollback,
                wroteEnabled = false,
                baselineEnabled = "0",
                failureCause = "readback_failed",
            ),
        )
    }

    @Test
    fun shizukuStateWithoutLiveBinderRequiresPriorAuthorizationForBinderDead() {
        assertEquals(
            OpenLessShizukuBridge.ShizukuState.BinderDead,
            OpenLessShizukuBridge.shizukuStateWithoutLiveBinder(
                binderDeadAfterAuthorization = true,
                backendAvailable = true,
            ),
        )
        assertEquals(
            OpenLessShizukuBridge.ShizukuState.NotRunning,
            OpenLessShizukuBridge.shizukuStateWithoutLiveBinder(
                binderDeadAfterAuthorization = false,
                backendAvailable = true,
            ),
        )
    }

    @Test
    fun preWriteSnapshotChangedDetectsServiceOrEnabledDrift() {
        val baseline = OpenLessShizukuBridge.AccessibilitySettingsSnapshot("a/.A", "0")
        assertFalse(
            OpenLessShizukuBridge.preWriteSnapshotChanged(
                baseline,
                OpenLessShizukuBridge.AccessibilitySettingsSnapshot("a/.A", "0"),
            ),
        )
        assertTrue(
            OpenLessShizukuBridge.preWriteSnapshotChanged(
                baseline,
                OpenLessShizukuBridge.AccessibilitySettingsSnapshot("a/.A:b/.B", "0"),
            ),
        )
        assertTrue(
            OpenLessShizukuBridge.preWriteSnapshotChanged(
                baseline,
                OpenLessShizukuBridge.AccessibilitySettingsSnapshot("a/.A", "1"),
            ),
        )
    }

    @Test
    fun normalizeComponentKeyTreatsShortAndFullFormsAsEqual() {
        val full = "com.openless.app/com.openless.app.OpenLessAccessibilityService"
        val shortForm = "com.openless.app/.OpenLessAccessibilityService"
        assertEquals(full, OpenLessShizukuBridge.normalizeComponentKey(shortForm))
        assertEquals(full, OpenLessShizukuBridge.normalizeComponentKey(full))
        assertTrue(OpenLessShizukuBridge.componentsEqual(shortForm, full))
    }

    @Test
    fun normalizeComponentKeyRejectsInvalidEntries() {
        assertEquals(null, OpenLessShizukuBridge.normalizeComponentKey("bad"))
        assertEquals(null, OpenLessShizukuBridge.normalizeComponentKey("com.foo;rm/.Svc"))
        assertEquals(
            "com.foo/com.foo.Svc",
            OpenLessShizukuBridge.canonicalizeServiceEntry("com.foo/.Svc"),
        )
        assertEquals(
            "com.foo/com.foo.Svc",
            OpenLessShizukuBridge.canonicalizeServiceEntry("com.foo/com.foo.Svc"),
        )
        assertEquals(
            "not-a-component",
            OpenLessShizukuBridge.canonicalizeServiceEntry("not-a-component"),
        )
    }

    @Test
    fun mergeDoesNotDuplicateOpenLessShortForm() {
        val shortForm = "com.openless.app/.OpenLessAccessibilityService"
        val merged = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(shortForm, serviceComponent)
        assertEquals(shortForm, merged)
    }

    @Test
    fun mergeTreatsEquivalentComponentIdsAsSame() {
        val shortForm = "com.openless.app/.OpenLessAccessibilityService"
        val merged = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(null, shortForm)
        val mergedAgain = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(merged, serviceComponent)
        assertTrue(OpenLessShizukuBridge.servicesListsEqual(mergedAgain, shortForm))
        assertEquals(1, OpenLessShizukuBridge.parseServiceEntries(mergedAgain).size)
    }

    @Test
    fun requiresManualRecoveryIgnoresEquivalentOpenLessEntry() {
        val shortForm = "com.openless.app/.OpenLessAccessibilityService"
        val snapshot = OpenLessShizukuBridge.AccessibilitySettingsSnapshot(shortForm, "0")
        assertFalse(OpenLessShizukuBridge.requiresManualRecovery(snapshot, serviceComponent))
    }

    @Test
    fun verifyReadbackExactRejectsConcurrentlyAddedServices() {
        val merged = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(null, serviceComponent)
        val readback = "$merged:$thirdParty"
        assertFalse(OpenLessShizukuBridge.verifyReadbackExact(readback, merged))
    }

    @Test
    fun verifyReadbackExactAcceptsExpectedMergedSet() {
        val merged = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(thirdParty, serviceComponent)
        assertTrue(OpenLessShizukuBridge.verifyReadbackExact(merged, merged))
        val shortFormReadback = "$thirdParty:com.openless.app/.OpenLessAccessibilityService"
        assertTrue(OpenLessShizukuBridge.verifyReadbackExact(shortFormReadback, merged))
    }

    @Test
    fun resolveStatusMessageKeyUsesUnsupportedBackendForLegacyShizuku() {
        val accessibility = OpenLessShizukuBridge.AccessibilityDiagnosis(
            registered = false,
            operational = false,
            messageKey = "not_registered",
        )
        assertEquals(
            "unsupported_backend",
            OpenLessShizukuBridge.resolveStatusMessageKey(
                legacyBackend = true,
                state = OpenLessShizukuBridge.ShizukuState.NotRunning,
                accessibility = accessibility,
            ),
        )
        assertEquals(
            "not_running",
            OpenLessShizukuBridge.resolveStatusMessageKey(
                legacyBackend = false,
                state = OpenLessShizukuBridge.ShizukuState.NotRunning,
                accessibility = accessibility,
            ),
        )
    }

    @Test
    fun rejectsShellInjectionInPackageName() {
        assertFalse(OpenLessShizukuBridge.isValidAndroidPackageName("com.foo;rm"))
        assertFalse(OpenLessShizukuBridge.isValidAndroidPackageName("com.foo|bar"))
        assertFalse(OpenLessShizukuBridge.isValidAndroidPackageName("com.foo\nbar"))
        assertTrue(OpenLessShizukuBridge.isValidAndroidPackageName("com.openless.app"))
    }
}
