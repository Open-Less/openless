package com.openless.app

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.util.Log
import androidx.annotation.Keep
import org.json.JSONObject
import rikka.shizuku.Shizuku
import rikka.sui.Sui

/**
 * Optional Shizuku integration for accessibility diagnostics, recovery, and paste injection.
 * Complements [OpenLessAccessibilityService]; paste tier 2 uses [injectPasteKey].
 */
@Keep
object OpenLessShizukuBridge {
    private const val TAG = "OpenLessShizuku"
    private const val KEYCODE_PASTE = "279"
    private const val SHIZUKU_PACKAGE = "moe.shizuku.privileged.api"
    private const val RECOVERY_BIND_TIMEOUT_MS = 5_000L
    private const val RECOVERY_BIND_POLL_MS = 250L
    private val ANDROID_PACKAGE_REGEX =
        Regex("^[a-zA-Z][a-zA-Z0-9_]*(\\.[a-zA-Z][a-zA-Z0-9_]*)*$")

    @Volatile
    private var binderWasAuthorized = false

    @Volatile
    private var binderDead = false

    @Volatile
    private var lastPermissionMessageKey: String? = null

    @JvmStatic
    fun setLastPermissionMessageKey(key: String) {
        lastPermissionMessageKey = key
    }

    private fun consumeLastPermissionMessageKey(): String? {
        return lastPermissionMessageKey?.also { lastPermissionMessageKey = null }
    }

    private val binderReceivedListener = Shizuku.OnBinderReceivedListener {
        binderDead = false
        Log.i(TAG, "Shizuku binder received")
    }

    private val binderDeadListener = Shizuku.OnBinderDeadListener {
        binderDead = binderWasAuthorized
        Log.i(TAG, "Shizuku binder dead wasAuthorized=$binderWasAuthorized")
    }

    @JvmStatic
    fun initialize() {
        Shizuku.addBinderReceivedListener(binderReceivedListener)
        Shizuku.addBinderDeadListener(binderDeadListener)
    }

    @JvmStatic
    @Keep
    fun getStatusJson(context: Context): String {
        val legacyBackend = isLegacyShizukuBackend()
        val state = detectState(context)
        val accessibility = diagnoseAccessibility(context)
        val messageKey = resolveStatusMessageKey(legacyBackend, state, accessibility)
        val json = JSONObject()
            .put("state", state.name)
            .put("messageKey", messageKey)
            .put(
                "accessibility",
                JSONObject()
                    .put("registered", accessibility.registered)
                    .put("operational", accessibility.operational)
                    .put("messageKey", accessibility.messageKey),
            )
        consumeLastPermissionMessageKey()?.let { key ->
            json.put("lastPermissionMessageKey", key)
        }
        return json.toString()
    }

    @JvmStatic
    @Keep
    fun requestPermission(context: Context): Boolean {
        if (!isShizukuBackendAvailable(context)) {
            return false
        }
        if (isLegacyShizukuBackend()) {
            setLastPermissionMessageKey("unsupported_backend")
            return false
        }
        return try {
            val intent = Intent(context, ShizukuPermissionActivity::class.java).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
            true
        } catch (error: Throwable) {
            Log.w(TAG, "launch Shizuku permission activity failed", error)
            false
        }
    }

    @JvmStatic
    @Keep
    fun openShizukuApp(context: Context): Boolean {
        if (isShizukuManagerInstalled(context)) {
            val launch = context.packageManager.getLaunchIntentForPackage(SHIZUKU_PACKAGE)
            if (launch != null) {
                return try {
                    context.startActivity(launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                    true
                } catch (error: Throwable) {
                    Log.w(TAG, "open Shizuku app failed", error)
                    false
                }
            }
        }
        return try {
            val market = Intent(
                Intent.ACTION_VIEW,
                Uri.parse("market://details?id=$SHIZUKU_PACKAGE"),
            ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(market)
            true
        } catch (error: Throwable) {
            Log.w(TAG, "open Shizuku store listing failed", error)
            false
        }
    }

    @JvmStatic
    @Keep
    fun injectPasteKey(context: Context): Boolean {
        if (isLegacyShizukuBackend()) {
            return false
        }
        if (detectState(context) != ShizukuState.Authorized) {
            return false
        }
        if (injectPasteKeyViaShizukuShell()) {
            return true
        }
        return OpenLessShizukuUserServiceClient.withPasteService(context) { service ->
            service.injectPasteKey()
        } == true
    }

    /**
     * MTK/Xiaomi ROMs NPE in UserService app_process startup; Shizuku.newProcess is private
     * but callable via reflection and does not spawn com.openless.app:* processes.
     */
    internal fun injectPasteKeyViaShizukuShell(): Boolean {
        if (!Shizuku.pingBinder()) {
            return false
        }
        return try {
            val method = Shizuku::class.java.getDeclaredMethod(
                "newProcess",
                Array<String>::class.java,
                Array<String>::class.java,
                String::class.java,
            )
            method.isAccessible = true
            @Suppress("UNCHECKED_CAST")
            val process = method.invoke(
                null,
                arrayOf("input", "keyevent", KEYCODE_PASTE),
                null,
                null,
            ) as Process
            val exitCode = process.waitFor()
            exitCode == 0
        } catch (error: Throwable) {
            Log.w(TAG, "inject paste via Shizuku.newProcess reflection failed", error)
            false
        }
    }

    @JvmStatic
    @Keep
    fun recoverAccessibilityJson(context: Context, confirmed: Boolean): String {
        if (!confirmed) {
            return recoveryJson(RecoveryOutcome.UserNotConfirmed, "user_not_confirmed")
        }
        if (isLegacyShizukuBackend()) {
            return recoveryJson(RecoveryOutcome.ShizukuUnavailable, "unsupported_backend")
        }
        if (detectState(context) != ShizukuState.Authorized) {
            return recoveryJson(RecoveryOutcome.ShizukuUnavailable, "shizuku_unavailable")
        }

        val serviceComponent = OpenLessAccessibilityService.serviceComponentId()
        if (!isValidServiceComponent(serviceComponent)) {
            return recoveryJson(RecoveryOutcome.ShellFailed, "invalid_component")
        }

        val recoveryPayload = OpenLessShizukuUserServiceClient.withRecoveryLock {
            val raw = OpenLessShizukuUserServiceClient.withService(context) { service ->
                service.recoverAccessibilityService(serviceComponent)
            } ?: return@withRecoveryLock recoveryJson(
                RecoveryOutcome.ShizukuUnavailable,
                "service_connect_failed",
            )
            raw
        } ?: return recoveryJson(
            RecoveryOutcome.ShellFailed,
            "recovery_in_progress",
        )

        val (outcome, messageKey) = parseRecoveryPayload(recoveryPayload)
            ?: return recoveryJson(RecoveryOutcome.ShellFailed, "parse_failed")

        if (outcome != RecoveryOutcome.Success) {
            return recoveryJson(outcome, messageKey)
        }

        if (!waitForAccessibilityOperational(context)) {
            return recoveryJson(RecoveryOutcome.ServiceNotBound, "service_not_bound")
        }

        return recoveryJson(RecoveryOutcome.Success, "success")
    }

    internal fun detectState(context: Context): ShizukuState {
        if (Shizuku.pingBinder()) {
            binderDead = false
            if (isLegacyShizukuBackend()) {
                return ShizukuState.NotRunning
            }
            return try {
                if (Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED) {
                    binderWasAuthorized = true
                    ShizukuState.Authorized
                } else {
                    ShizukuState.NotAuthorized
                }
            } catch (error: Throwable) {
                Log.w(TAG, "Shizuku permission check failed", error)
                ShizukuState.NotRunning
            }
        }

        if (binderDead && binderWasAuthorized) {
            return ShizukuState.BinderDead
        }

        if (isShizukuBackendAvailable(context)) {
            return ShizukuState.NotRunning
        }

        return ShizukuState.NotInstalled
    }

    internal fun shizukuStateWithoutLiveBinder(
        binderDeadAfterAuthorization: Boolean,
        backendAvailable: Boolean,
    ): ShizukuState {
        if (binderDeadAfterAuthorization) {
            return ShizukuState.BinderDead
        }
        return if (backendAvailable) {
            ShizukuState.NotRunning
        } else {
            ShizukuState.NotInstalled
        }
    }

    internal fun diagnoseAccessibility(context: Context): AccessibilityDiagnosis {
        val registered = OpenLessAccessibilityService.isEnabled(context)
        val operational = registered && OpenLessAccessibilityService.pingAccessibilityProcess(context)
        val messageKey = when {
            operational -> "operational"
            registered -> "registered_stale"
            else -> "not_registered"
        }
        return AccessibilityDiagnosis(registered, operational, messageKey)
    }

    internal fun parseServiceEntries(raw: String?): LinkedHashSet<String> {
        val entries = LinkedHashSet<String>()
        raw
            ?.split(':')
            ?.map { it.trim() }
            ?.filter { it.isNotEmpty() && it != "null" }
            ?.forEach { entries.add(it) }
        return entries
    }

    internal fun mergeEnabledAccessibilityServices(current: String?, serviceComponent: String): String {
        val normalizedComponent = serviceComponent.trim()
        if (normalizedComponent.isEmpty()) return ""
        val canonicalOpenLess = canonicalizeServiceEntry(normalizedComponent)
        val entries = parseServiceEntries(current).toMutableList()
        val hasOpenLess = entries.any { componentsEqual(it, canonicalOpenLess) }
        if (!hasOpenLess) {
            entries.add(canonicalOpenLess)
        }
        return entries.joinToString(":")
    }

    internal data class AccessibilitySettingsSnapshot(
        val services: String,
        val enabled: String,
    )

    internal fun preWriteSnapshotChanged(
        baseline: AccessibilitySettingsSnapshot,
        observed: AccessibilitySettingsSnapshot,
    ): Boolean {
        return !servicesListsEqual(baseline.services, observed.services) ||
            baseline.enabled != observed.enabled
    }

    internal enum class ServicesRollbackResult {
        Restored,
        AlreadyBaseline,
        Conflict,
        ReadFailed,
        WriteFailed,
    }

    internal enum class EnabledRollbackResult {
        Restored,
        AlreadyBaseline,
        Skipped,
        SkippedDueToServicesConflict,
        ReadFailed,
        WriteFailed,
    }

    internal fun shouldRollbackEnabledAfterServices(
        servicesRollback: ServicesRollbackResult,
        writtenEnabled: String,
        baselineEnabled: String,
    ): Boolean {
        if (writtenEnabled == baselineEnabled) {
            return false
        }
        // Never auto-disable global accessibility during rollback. Concurrent services may
        // have been enabled after our write and still depend on accessibility_enabled=1.
        if (writtenEnabled == "1" && baselineEnabled != "1") {
            return false
        }
        return when (servicesRollback) {
            ServicesRollbackResult.Restored,
            ServicesRollbackResult.AlreadyBaseline,
            -> true
            ServicesRollbackResult.Conflict,
            ServicesRollbackResult.ReadFailed,
            ServicesRollbackResult.WriteFailed,
            -> false
        }
    }

    internal fun evaluateUnchangedServicesWriteRollback(
        currentServices: String,
        baselineServices: String,
    ): ServicesRollbackResult {
        return if (servicesListsEqual(currentServices, baselineServices)) {
            ServicesRollbackResult.AlreadyBaseline
        } else {
            ServicesRollbackResult.Conflict
        }
    }

    internal fun requiresManualRecovery(
        snapshot: AccessibilitySettingsSnapshot,
        openLessComponent: String,
    ): Boolean {
        if (snapshot.enabled == "1") {
            return false
        }
        val normalizedComponent = openLessComponent.trim()
        return parseServiceEntries(snapshot.services).any {
            !componentsEqual(it, normalizedComponent)
        }
    }

    internal data class RecoveryRollbackStatus(
        val services: ServicesRollbackResult,
        val enabled: EnabledRollbackResult,
    )

    internal fun recoveryFailureMessageKey(
        rollback: RecoveryRollbackStatus,
        wroteEnabled: Boolean,
        baselineEnabled: String,
        failureCause: String,
    ): String {
        return if (isRollbackComplete(rollback, wroteEnabled, baselineEnabled)) {
            failureCause
        } else {
            "partial_rollback"
        }
    }

    internal fun isRollbackComplete(
        rollback: RecoveryRollbackStatus,
        wroteEnabled: Boolean,
        baselineEnabled: String,
    ): Boolean {
        val servicesComplete = rollback.services == ServicesRollbackResult.Restored ||
            rollback.services == ServicesRollbackResult.AlreadyBaseline
        if (!wroteEnabled) {
            return servicesComplete
        }
        if (baselineEnabled == "1") {
            return servicesComplete && (
                rollback.enabled == EnabledRollbackResult.Restored ||
                    rollback.enabled == EnabledRollbackResult.AlreadyBaseline ||
                    rollback.enabled == EnabledRollbackResult.Skipped
                )
        }
        return false
    }

    internal fun shellQuote(value: String): String {
        return "'" + value.replace("'", "'\\''") + "'"
    }

    internal fun normalizeComponentKey(component: String): String? {
        val trimmed = component.trim()
        val slash = trimmed.indexOf('/')
        if (slash <= 0 || slash == trimmed.lastIndex) {
            return null
        }
        val packageName = trimmed.substring(0, slash)
        val className = trimmed.substring(slash + 1)
        if (className.isEmpty() || className.any { it.isWhitespace() || it == '\n' || it == '\r' }) {
            return null
        }
        if (!isValidAndroidPackageName(packageName)) {
            return null
        }
        val fullClassName = if (className.startsWith('.')) {
            packageName + className
        } else {
            className
        }
        if (fullClassName.any { it.isWhitespace() || it == '\n' || it == '\r' || it == '/' }) {
            return null
        }
        return "$packageName/$fullClassName"
    }

    internal fun canonicalizeServiceEntry(entry: String): String {
        return normalizeComponentKey(entry) ?: entry.trim()
    }

    internal fun componentsEqual(left: String, right: String): Boolean {
        val leftKey = normalizeComponentKey(left)
        val rightKey = normalizeComponentKey(right)
        if (leftKey != null && rightKey != null) {
            return leftKey == rightKey
        }
        return left.trim() == right.trim()
    }

    internal fun normalizedEntrySet(entries: Collection<String>): Set<String> {
        val normalized = LinkedHashSet<String>()
        for (entry in entries) {
            normalized.add(canonicalizeServiceEntry(entry))
        }
        return normalized
    }

    internal fun servicesListsEqual(left: String?, right: String?): Boolean {
        return normalizedEntrySet(parseServiceEntries(left)) ==
            normalizedEntrySet(parseServiceEntries(right))
    }

    internal fun readbackContainsComponent(readback: String, serviceComponent: String): Boolean {
        return parseServiceEntries(readback).any { componentsEqual(it, serviceComponent) }
    }

    internal fun verifyReadback(readback: String, serviceComponent: String): Boolean {
        return readbackContainsComponent(readback, serviceComponent)
    }

    internal fun verifyReadbackPreserves(
        readback: String,
        serviceComponent: String,
        originalEntries: Set<String>,
    ): Boolean {
        if (!readbackContainsComponent(readback, serviceComponent)) {
            return false
        }
        val readbackEntries = parseServiceEntries(readback)
        return originalEntries.all { baseline ->
            readbackEntries.any { componentsEqual(it, baseline) }
        }
    }

    internal fun verifyReadbackExact(readback: String, expectedMerged: String): Boolean {
        return servicesListsEqual(readback, expectedMerged)
    }

    internal fun isLegacyShizukuBackend(): Boolean {
        if (!Shizuku.pingBinder()) {
            return false
        }
        return try {
            Shizuku.isPreV11()
        } catch (error: Throwable) {
            Log.w(TAG, "Shizuku pre-v11 check failed", error)
            true
        }
    }

    internal fun resolveStatusMessageKey(
        legacyBackend: Boolean,
        state: ShizukuState,
        accessibility: AccessibilityDiagnosis,
    ): String {
        if (legacyBackend) {
            return "unsupported_backend"
        }
        return stateMessageKey(state, accessibility)
    }

    internal fun isValidServiceComponent(component: String): Boolean {
        if (component != component.trim()) {
            return false
        }
        val trimmed = component.trim()
        val slash = trimmed.indexOf('/')
        if (slash <= 0 || slash == trimmed.lastIndex) {
            return false
        }
        val packageName = trimmed.substring(0, slash)
        val className = trimmed.substring(slash + 1)
        if (className.isEmpty() || className.any { it.isWhitespace() || it == '\n' || it == '\r' }) {
            return false
        }
        return isValidAndroidPackageName(packageName)
    }

    internal fun isValidAndroidPackageName(packageName: String): Boolean {
        return ANDROID_PACKAGE_REGEX.matches(packageName)
    }

    private fun parseRecoveryPayload(json: String): Pair<RecoveryOutcome, String>? {
        return try {
            val value = JSONObject(json)
            val outcome = RecoveryOutcome.valueOf(value.getString("outcome"))
            val messageKey = value.optString("messageKey", value.optString("message", "unknown"))
            outcome to messageKey
        } catch (_: Throwable) {
            null
        }
    }

    private fun isShizukuBackendAvailable(context: Context): Boolean {
        return isShizukuManagerInstalled(context) || isSuiAvailable()
    }

    private fun isShizukuManagerInstalled(context: Context): Boolean {
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                context.packageManager.getPackageInfo(
                    SHIZUKU_PACKAGE,
                    PackageManager.PackageInfoFlags.of(0),
                )
            } else {
                @Suppress("DEPRECATION")
                context.packageManager.getPackageInfo(SHIZUKU_PACKAGE, 0)
            }
            true
        } catch (_: PackageManager.NameNotFoundException) {
            false
        }
    }

    private fun isSuiAvailable(): Boolean {
        return try {
            Sui.isSui()
        } catch (_: Throwable) {
            false
        }
    }

    private fun stateMessageKey(
        state: ShizukuState,
        accessibility: AccessibilityDiagnosis,
    ): String {
        return when (state) {
            ShizukuState.NotInstalled -> "not_installed"
            ShizukuState.NotRunning -> "not_running"
            ShizukuState.NotAuthorized -> "not_authorized"
            ShizukuState.BinderDead -> "binder_dead"
            ShizukuState.Authorized -> when {
                accessibility.operational -> "authorized_operational"
                accessibility.registered -> "authorized_registered_stale"
                else -> "authorized_can_recover"
            }
        }
    }

    private fun waitForAccessibilityOperational(context: Context): Boolean {
        val deadline = System.currentTimeMillis() + RECOVERY_BIND_TIMEOUT_MS
        while (System.currentTimeMillis() < deadline) {
            if (OpenLessAccessibilityService.pingAccessibilityProcess(context)) {
                return true
            }
            try {
                Thread.sleep(RECOVERY_BIND_POLL_MS)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                return false
            }
        }
        return OpenLessAccessibilityService.pingAccessibilityProcess(context)
    }

    private fun recoveryJson(outcome: RecoveryOutcome, messageKey: String): String {
        return JSONObject()
            .put("outcome", outcome.name)
            .put("messageKey", messageKey)
            .toString()
    }

    enum class ShizukuState {
        NotInstalled,
        NotRunning,
        NotAuthorized,
        Authorized,
        BinderDead,
    }

    data class AccessibilityDiagnosis(
        val registered: Boolean,
        val operational: Boolean,
        val messageKey: String,
    )

    enum class RecoveryOutcome {
        Success,
        WriteRejected,
        ServiceNotBound,
        ShizukuUnavailable,
        UserNotConfirmed,
        ShellFailed,
    }
}
