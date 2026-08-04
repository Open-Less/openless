package com.openless.app

import android.content.Context
import android.os.Build
import android.util.Log
import androidx.annotation.Keep
import org.json.JSONObject
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/**
 * Runs in a Shizuku UserService process with shell/root identity.
 * Best-effort accessibility recovery — Secure Settings writes are not compare-and-set.
 */
@Keep
class OpenLessShizukuUserService @JvmOverloads constructor(
    private val appPackage: String = "",
) : IOpenLessShizukuUserService.Stub() {

    @Keep
    constructor(context: Context) : this(context.packageName)

    override fun destroy() {
        Log.i(TAG, "destroy")
        System.exit(0)
    }

    override fun injectPasteKey(): Boolean {
        return runPasteKeyInjection() is ShellResult.Success
    }

    override fun recoverAccessibilityService(serviceComponent: String): String {
        if (!OpenLessShizukuBridge.isValidServiceComponent(serviceComponent)) {
            return recoveryJson(
                OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                "invalid_component",
            )
        }

        for (attempt in 0 until MAX_RECOVERY_ATTEMPTS) {
            when (val attemptResult = attemptRecovery(serviceComponent, attempt)) {
                is RecoveryAttemptResult.Success -> {
                    return recoveryJson(
                        OpenLessShizukuBridge.RecoveryOutcome.Success,
                        "success",
                    )
                }
                is RecoveryAttemptResult.Retry -> {
                    // Try again after a concurrent settings change.
                }
                is RecoveryAttemptResult.Failure -> {
                    return recoveryJson(attemptResult.outcome, attemptResult.messageKey)
                }
            }
        }

        return recoveryJson(
            OpenLessShizukuBridge.RecoveryOutcome.WriteRejected,
            "concurrent_change",
        )
    }

    private fun attemptRecovery(
        serviceComponent: String,
        attempt: Int,
    ): RecoveryAttemptResult {
        val preWrite = readSnapshot()
            ?: return RecoveryAttemptResult.Failure(
                OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                "read_failed",
            )

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && appPackage.isNotBlank()) {
            if (!allowRestrictedSettingsForApp()) {
                Log.w(TAG, "ACCESS_RESTRICTED_SETTINGS appops step failed attempt=$attempt")
            }
        }

        val immediate = readSnapshot()
            ?: return RecoveryAttemptResult.Failure(
                OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                "read_failed",
            )
        if (OpenLessShizukuBridge.preWriteSnapshotChanged(preWrite, immediate)) {
            Log.i(TAG, "pre-put snapshot changed attempt=$attempt")
            return RecoveryAttemptResult.Retry
        }

        val prePut = readSnapshot()
            ?: return RecoveryAttemptResult.Failure(
                OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                "read_failed",
            )
        if (OpenLessShizukuBridge.preWriteSnapshotChanged(immediate, prePut)) {
            Log.i(TAG, "immediate pre-put snapshot changed attempt=$attempt")
            return RecoveryAttemptResult.Retry
        }

        if (OpenLessShizukuBridge.requiresManualRecovery(prePut, serviceComponent)) {
            return RecoveryAttemptResult.Failure(
                OpenLessShizukuBridge.RecoveryOutcome.WriteRejected,
                "manual_required",
            )
        }

        val merged = OpenLessShizukuBridge.mergeEnabledAccessibilityServices(
            prePut.services,
            serviceComponent,
        )
        if (merged.isBlank()) {
            return RecoveryAttemptResult.Failure(
                OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                "merge_failed",
            )
        }

        if (!putSecureSetting(KEY_ENABLED_SERVICES, merged)) {
            return RecoveryAttemptResult.Failure(
                OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                "write_services_failed",
            )
        }

        val postServicesPut = readEnabledServices()
            ?: run {
                val rollback = rollbackWrittenState(
                    WrittenState(merged, null),
                    prePut,
                )
                return failureAfterRollback(
                    rollback,
                    prePut,
                    WrittenState(merged, null),
                    OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                    "readback_failed",
                )
            }
        if (!OpenLessShizukuBridge.servicesListsEqual(postServicesPut, merged)) {
            Log.w(TAG, "services changed immediately after write attempt=$attempt")
            rollbackWrittenState(
                WrittenState(merged, null),
                prePut,
            )
            return RecoveryAttemptResult.Retry
        }

        val writtenEnabled = "1"
        if (!putSecureSetting(KEY_ACCESSIBILITY_ENABLED, writtenEnabled)) {
            val rollback = rollbackWrittenState(
                WrittenState(merged, writtenEnabled),
                prePut,
            )
            return failureAfterRollback(
                rollback,
                prePut,
                WrittenState(merged, writtenEnabled),
                OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                "write_enabled_failed",
            )
        }

        val readback = readEnabledServices()
            ?: run {
                val rollback = rollbackWrittenState(
                    WrittenState(merged, writtenEnabled),
                    prePut,
                )
                return failureAfterRollback(
                    rollback,
                    prePut,
                    WrittenState(merged, writtenEnabled),
                    OpenLessShizukuBridge.RecoveryOutcome.ShellFailed,
                    "readback_failed",
                )
            }

        if (!OpenLessShizukuBridge.verifyReadbackExact(readback, merged)) {
            val rollback = rollbackWrittenState(
                WrittenState(merged, writtenEnabled),
                prePut,
            )
            val failureCause = if (!OpenLessShizukuBridge.readbackContainsComponent(readback, serviceComponent)) {
                "oem_rollback"
            } else {
                "concurrent_change"
            }
            return failureAfterRollback(
                rollback,
                prePut,
                WrittenState(merged, writtenEnabled),
                OpenLessShizukuBridge.RecoveryOutcome.WriteRejected,
                failureCause,
            )
        }

        return RecoveryAttemptResult.Success
    }

    private fun rollbackWrittenState(
        written: WrittenState,
        snapshot: OpenLessShizukuBridge.AccessibilitySettingsSnapshot,
    ): RollbackOutcome {
        val servicesResult = rollbackServicesIfUnchanged(written.services, snapshot.services)
        val enabledResult = if (written.enabled == null) {
            OpenLessShizukuBridge.EnabledRollbackResult.Skipped
        } else if (
            OpenLessShizukuBridge.shouldRollbackEnabledAfterServices(
                servicesResult,
                written.enabled,
                snapshot.enabled,
            )
        ) {
            rollbackEnabledIfUnchanged(
                written.enabled,
                snapshot.enabled,
                snapshot.services,
            )
        } else {
            Log.w(TAG, "skip enabled rollback: services rollback=${servicesResult.name}")
            OpenLessShizukuBridge.EnabledRollbackResult.SkippedDueToServicesConflict
        }
        return RollbackOutcome(servicesResult, enabledResult)
    }

    private fun failureAfterRollback(
        rollback: RollbackOutcome,
        snapshot: OpenLessShizukuBridge.AccessibilitySettingsSnapshot,
        written: WrittenState,
        outcome: OpenLessShizukuBridge.RecoveryOutcome,
        failureCause: String,
    ): RecoveryAttemptResult.Failure {
        val messageKey = OpenLessShizukuBridge.recoveryFailureMessageKey(
            OpenLessShizukuBridge.RecoveryRollbackStatus(
                rollback.services,
                rollback.enabled,
            ),
            wroteEnabled = written.enabled != null,
            baselineEnabled = snapshot.enabled,
            failureCause = failureCause,
        )
        return RecoveryAttemptResult.Failure(outcome, messageKey)
    }

    private fun rollbackServicesIfUnchanged(
        writtenServices: String,
        baselineServices: String,
    ): OpenLessShizukuBridge.ServicesRollbackResult {
        val current = readEnabledServices()
            ?: return OpenLessShizukuBridge.ServicesRollbackResult.ReadFailed

        if (OpenLessShizukuBridge.servicesListsEqual(writtenServices, baselineServices)) {
            return OpenLessShizukuBridge.evaluateUnchangedServicesWriteRollback(
                current,
                baselineServices,
            )
        }

        if (!OpenLessShizukuBridge.servicesListsEqual(current, writtenServices)) {
            Log.w(TAG, "skip services rollback: current differs from written")
            return OpenLessShizukuBridge.ServicesRollbackResult.Conflict
        }
        return if (putSecureSetting(KEY_ENABLED_SERVICES, baselineServices)) {
            OpenLessShizukuBridge.ServicesRollbackResult.Restored
        } else {
            OpenLessShizukuBridge.ServicesRollbackResult.WriteFailed
        }
    }

    private fun rollbackEnabledIfUnchanged(
        writtenEnabled: String,
        baselineEnabled: String,
        baselineServices: String,
    ): OpenLessShizukuBridge.EnabledRollbackResult {
        if (writtenEnabled == baselineEnabled) {
            return OpenLessShizukuBridge.EnabledRollbackResult.AlreadyBaseline
        }
        if (writtenEnabled == "1" && baselineEnabled != "1") {
            Log.w(TAG, "skip enabled rollback: refusing to auto-disable global accessibility")
            return OpenLessShizukuBridge.EnabledRollbackResult.SkippedDueToServicesConflict
        }
        val currentEnabled = readAccessibilityEnabled()
            ?: return OpenLessShizukuBridge.EnabledRollbackResult.ReadFailed
        if (currentEnabled != writtenEnabled) {
            Log.w(TAG, "skip enabled rollback: current differs from written")
            return OpenLessShizukuBridge.EnabledRollbackResult.Skipped
        }
        val currentServices = readEnabledServices()
            ?: return OpenLessShizukuBridge.EnabledRollbackResult.ReadFailed
        if (!OpenLessShizukuBridge.servicesListsEqual(currentServices, baselineServices)) {
            Log.w(TAG, "skip enabled rollback: services changed before enabled put")
            return OpenLessShizukuBridge.EnabledRollbackResult.SkippedDueToServicesConflict
        }
        return if (putSecureSetting(KEY_ACCESSIBILITY_ENABLED, baselineEnabled)) {
            OpenLessShizukuBridge.EnabledRollbackResult.Restored
        } else {
            OpenLessShizukuBridge.EnabledRollbackResult.WriteFailed
        }
    }

    private fun readSnapshot(): OpenLessShizukuBridge.AccessibilitySettingsSnapshot? {
        val services = readEnabledServices() ?: return null
        val enabled = readAccessibilityEnabled() ?: return null
        return OpenLessShizukuBridge.AccessibilitySettingsSnapshot(services, enabled)
    }

    private fun readEnabledServices(): String? {
        return when (val result = runSettingsGet(KEY_ENABLED_SERVICES)) {
            is ShellResult.Failure -> null
            is ShellResult.Success -> result.value
        }
    }

    private fun readAccessibilityEnabled(): String? {
        return when (val result = runSettingsGet(KEY_ACCESSIBILITY_ENABLED)) {
            is ShellResult.Failure -> null
            is ShellResult.Success -> result.value
        }
    }

    private fun putSecureSetting(key: String, value: String): Boolean {
        if (!isAllowedSecureKey(key)) {
            return false
        }
        return runSettingsPut(key, value) is ShellResult.Success
    }

    private fun allowRestrictedSettingsForApp(): Boolean {
        if (appPackage.isBlank() || !OpenLessShizukuBridge.isValidAndroidPackageName(appPackage)) {
            return false
        }
        return runProcess(
            listOf("cmd", "appops", "set", appPackage, "ACCESS_RESTRICTED_SETTINGS", "allow"),
        ) is ShellResult.Success
    }

    private fun runPasteKeyInjection(): ShellResult {
        return runProcess(listOf("input", "keyevent", KEYCODE_PASTE))
    }

    private fun runSettingsGet(key: String): ShellResult {
        if (!isAllowedSecureKey(key)) {
            return ShellResult.Failure
        }
        return when (val result = runProcess(listOf("settings", "get", "secure", key))) {
            is ShellResult.Failure -> ShellResult.Failure
            is ShellResult.Success -> ShellResult.Success(normalizeSettingsOutput(result.value))
        }
    }

    private fun runSettingsPut(key: String, value: String): ShellResult {
        if (!isAllowedSecureKey(key)) {
            return ShellResult.Failure
        }
        return runProcess(listOf("settings", "put", "secure", key, value))
    }

    private fun normalizeSettingsOutput(raw: String): String {
        val trimmed = raw.trim()
        if (trimmed.isEmpty() || trimmed == "null") {
            return ""
        }
        return trimmed
    }

    private fun isAllowedSecureKey(key: String): Boolean {
        return key == KEY_ENABLED_SERVICES || key == KEY_ACCESSIBILITY_ENABLED
    }

    private fun runProcess(command: List<String>): ShellResult {
        if (command.isEmpty() || command.any { it.isBlank() }) {
            return ShellResult.Failure
        }
        return try {
            val process = ProcessBuilder(command)
                .redirectErrorStream(true)
                .start()
            val reader = Executors.newSingleThreadExecutor()
            val outputTask = reader.submit(Callable { process.inputStream.bufferedReader().readText() })
            try {
                if (!process.waitFor(SHELL_TIMEOUT_SEC, TimeUnit.SECONDS)) {
                    process.destroyForcibly()
                    return ShellResult.Failure
                }
                if (process.exitValue() != 0) {
                    return ShellResult.Failure
                }
                ShellResult.Success(outputTask.get(1, TimeUnit.SECONDS).orEmpty())
            } catch (_: TimeoutException) {
                process.destroyForcibly()
                ShellResult.Failure
            } finally {
                reader.shutdownNow()
            }
        } catch (error: Throwable) {
            Log.w(TAG, "privileged process failed", error)
            ShellResult.Failure
        }
    }

    private fun recoveryJson(
        outcome: OpenLessShizukuBridge.RecoveryOutcome,
        messageKey: String,
    ): String {
        return JSONObject()
            .put("outcome", outcome.name)
            .put("messageKey", messageKey)
            .toString()
    }

    private data class WrittenState(
        val services: String,
        val enabled: String?,
    )

    private data class RollbackOutcome(
        val services: OpenLessShizukuBridge.ServicesRollbackResult,
        val enabled: OpenLessShizukuBridge.EnabledRollbackResult,
    )

    private sealed class RecoveryAttemptResult {
        data object Success : RecoveryAttemptResult()
        data object Retry : RecoveryAttemptResult()
        data class Failure(
            val outcome: OpenLessShizukuBridge.RecoveryOutcome,
            val messageKey: String,
        ) : RecoveryAttemptResult()
    }

    private sealed class ShellResult {
        data class Success(val value: String) : ShellResult()
        data object Failure : ShellResult()
    }

    companion object {
        private const val TAG = "OpenLessShizukuUserSvc"
        private const val SHELL_TIMEOUT_SEC = 10L
        private const val KEY_ENABLED_SERVICES = "enabled_accessibility_services"
        private const val KEY_ACCESSIBILITY_ENABLED = "accessibility_enabled"
        private const val MAX_RECOVERY_ATTEMPTS = 3
        private const val KEYCODE_PASTE = "279"
    }
}
