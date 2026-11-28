package com.openless.app

import android.app.Activity
import android.app.ActivityManager
import android.app.Notification
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.SystemClock
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.atomic.AtomicReference
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/** Runs the actual service and Android notification lifecycle, without the Rust backend. */
@RunWith(AndroidJUnit4::class)
class OpenLessRuntimeServiceInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private var activity: Activity? = null
    private val serviceIntent get() = Intent(context, OpenLessRuntimeService::class.java)

    @Before fun setUp() {
        if (Build.VERSION.SDK_INT >= 33) {
            instrumentation.uiAutomation.grantRuntimePermission(context.packageName, "android.permission.POST_NOTIFICATIONS")
        }
        // Make starts user-visible/foreground; do not bypass Android's FGS start restrictions.
        activity = instrumentation.startActivitySync(
            Intent(context, OpenLessKeyboardSettingsActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        instrumentation.waitForIdleSync()
        assertFalse(OpenLessRuntimeService.authorizationActive())
        assertFalse(OpenLessRuntimeService.imeRequested)
        await("clean initial service state") { !running() && notification() == null }
    }

    @After fun tearDown() {
        while (OpenLessRuntimeService.authorizationActive()) OpenLessRuntimeService.releaseAuthorization(context)
        OpenLessRuntimeService.setImeRequested(context, false)
        await("runtime removed after last demand") { !running() && notification() == null }
        instrumentation.runOnMainSync { activity?.finish() }
    }

    @Test fun idleActivityDestructionAndRuntimeExitDoNotLeaveANotification() {
        val activityStarts = starts("actkill")
        OpenLessRuntimeService.notifyRuntimeActivityDestroyed(context, "finishing")
        await("activity teardown recorded") {
            starts("actkill") == activityStarts + 1
        }
        await("idle activity teardown stopped") { !running() && notification() == null }

        val exits = starts("rtexit")
        context.startService(serviceIntent.setAction("com.openless.app.action.RUNTIME_EXITED"))
        await("runtime exit recorded") { starts("rtexit") == exits + 1 }
        await("idle runtime exit stopped") { !running() && notification() == null }
    }

    @Test fun authorizationKeepsForegroundRuntimeWithNeutralTextUntilFinalRelease() {
        OpenLessRuntimeService.acquireAuthorization(context)
        awaitText("后台运行服务已就绪")
        assertTrue(foreground())
        instrumentation.runOnMainSync {
            val service = instance()!!
            assertEquals(Service.START_NOT_STICKY, service.onStartCommand(serviceIntent, 0, startId(service)))
        }
        val original = instance()
        val activityStarts = starts("actkill")
        OpenLessRuntimeService.notifyRuntimeActivityDestroyed(context, "os")
        await("active authorization teardown recorded") {
            starts("actkill") == activityStarts + 1
        }
        assertSame(original, instance())
        assertTrue(foreground())
        assertEquals("后台运行服务已就绪", text())
        OpenLessRuntimeService.acquireAuthorization(context)
        OpenLessRuntimeService.releaseAuthorization(context)
        instrumentation.waitForIdleSync()
        assertTrue(OpenLessRuntimeService.authorizationActive())
        assertTrue(foreground())
        assertEquals("后台运行服务已就绪", text())
        OpenLessRuntimeService.releaseAuthorization(context)
        await("last authorization removed") { !running() && notification() == null }
    }

    @Test fun imeAndAuthorizationOverlapRefreshesInBothDirections() {
        OpenLessRuntimeService.setImeRequested(context, true)
        awaitText("输入法面板已就绪")
        OpenLessRuntimeService.acquireAuthorization(context)
        OpenLessRuntimeService.setImeRequested(context, false)
        awaitText("后台运行服务已就绪")
        assertTrue(foreground())
        OpenLessRuntimeService.setImeRequested(context, true)
        awaitText("输入法面板已就绪")
        OpenLessRuntimeService.releaseAuthorization(context)
        instrumentation.waitForIdleSync()
        assertFalse(OpenLessRuntimeService.authorizationActive())
        assertTrue(foreground())
        assertEquals("输入法面板已就绪", text())
        OpenLessRuntimeService.setImeRequested(context, false)
        await("last IME demand removed") { !running() && notification() == null }
    }

    @Test fun queuedOldReleaseCannotStopANewerAcquire() {
        OpenLessRuntimeService.acquireAuthorization(context)
        awaitText("后台运行服务已就绪")
        val original = instance()
        assertNotNull(original)
        // Hold the Android main thread while real JNI-like workers release then acquire.
        // The old release's queued reconciliation must read the NEW demand, not stop it.
        instrumentation.runOnMainSync {
            worker { OpenLessRuntimeService.releaseAuthorization(context) }
            worker { OpenLessRuntimeService.acquireAuthorization(context) }
        }
        instrumentation.waitForIdleSync()
        awaitText("后台运行服务已就绪")
        assertTrue(OpenLessRuntimeService.authorizationActive())
        assertTrue(foreground())
        assertSame("old cleanup must not destroy the new lease's context", original, instance())
    }

    @Test fun queuedReleaseBeforeInitialForegroundStartStopsRuntimeWithoutCrashing() {
        instrumentation.runOnMainSync {
            worker { OpenLessRuntimeService.acquireAuthorization(context) }
            worker { OpenLessRuntimeService.releaseAuthorization(context) }
            // The system has queued the start while the main thread is blocked.
            // A second startService call would clear the foreground-start requirement.
            assertTrue("initial foreground start was accepted", running())
        }
        await("released-before-start runtime stopped") { !running() && notification() == null }
        // A redundant release must neither underflow nor create another service.
        OpenLessRuntimeService.releaseAuthorization(context)
        instrumentation.waitForIdleSync()
        assertFalse(OpenLessRuntimeService.authorizationActive())
        assertFalse(running())
    }

    @Test fun queuedIdleCallbackCannotStopANewerForegroundStart() {
        val exits = starts("rtexit")
        instrumentation.runOnMainSync {
            // This callback arrives first, while Android is waiting for the newer
            // foreground start to be promoted.
            context.startService(serviceIntent.setAction("com.openless.app.action.RUNTIME_EXITED"))
            worker { OpenLessRuntimeService.acquireAuthorization(context) }
            worker { OpenLessRuntimeService.releaseAuthorization(context) }
        }
        await("queued runtime exit recorded") { starts("rtexit") == exits + 1 }
        await("newer foreground start handled and stopped") { !running() && notification() == null }
    }

    @Test fun nullIntentRestartUsesOnlyLiveDemandAndRetainsRestartCounters() {
        OpenLessRuntimeService.setImeRequested(context, true)
        awaitText("输入法面板已就绪")
        val sticky = starts("sticky")
        instrumentation.runOnMainSync {
            val service = instance()!!
            val startId = startId(service)
            assertEquals(Service.START_STICKY, service.onStartCommand(null, 0, startId))
            OpenLessRuntimeService.setImeRequested(context, false)
            // Model lost process-local demand before a sticky null-intent delivery.
            assertEquals(Service.START_NOT_STICKY, service.onStartCommand(null, 0, startId))
        }
        assertEquals(sticky + 2, starts("sticky"))
        await("idle sticky restart stopped") { !running() && notification() == null }
    }

    @Test fun failedAcquireRollsBackOnlyItsOwnLease() {
        val failing = object : android.content.ContextWrapper(context) {
            override fun startForegroundService(service: Intent): android.content.ComponentName? =
                throw IllegalStateException("deliberate start rejection")
            override fun startService(service: Intent): android.content.ComponentName? =
                throw IllegalStateException("deliberate start rejection")
        }
        OpenLessRuntimeService.acquireAuthorization(context)
        awaitText("后台运行服务已就绪")
        try {
            OpenLessRuntimeService.acquireAuthorization(failing)
            throw AssertionError("failed start must propagate to JNI caller")
        } catch (expected: IllegalStateException) {
            assertEquals("deliberate start rejection", expected.message)
        }
        assertTrue(OpenLessRuntimeService.authorizationActive())
        OpenLessRuntimeService.releaseAuthorization(context)
        await("failed acquire did not leak a lease") { !running() && notification() == null }
    }

    private fun starts(key: String): Int {
        var count = 0
        // RestartStats' shared date formatter is main-thread-owned in this fixture.
        instrumentation.runOnMainSync { count = OpenLessProcessRestartStats(context, key).today() }
        return count
    }

    private fun worker(block: () -> Unit) {
        val failure = AtomicReference<Throwable?>()
        val thread = Thread { try { block() } catch (error: Throwable) { failure.set(error) } }
        thread.start()
        thread.join(3000)
        assertFalse("JNI-like worker must not wait for the main thread", thread.isAlive)
        failure.get()?.let { throw AssertionError("worker failed", it) }
    }

    // Observe the actual instance only for deterministic callback/race assertions.
    private fun instance(): OpenLessRuntimeService? =
        OpenLessRuntimeService::class.java.getDeclaredField("runningService").let {
            it.isAccessible = true
            it.get(null) as OpenLessRuntimeService?
        }

    // Simulate callbacks with the real start ID so Android accepts stopSelfResult.
    private fun startId(service: OpenLessRuntimeService): Int =
        OpenLessRuntimeService::class.java.getDeclaredField("lastStartId").let {
            it.isAccessible = true
            it.get(service) as Int
        }

    @Suppress("DEPRECATION")
    private fun record() = context.getSystemService(ActivityManager::class.java).getRunningServices(Int.MAX_VALUE)
        .firstOrNull { it.service.className == OpenLessRuntimeService::class.java.name }
    private fun running() = record() != null
    private fun foreground() = record()?.foreground == true
    private fun notification(): Notification? = context.getSystemService(NotificationManager::class.java)
        .activeNotifications.firstOrNull { it.id == 42002 }?.notification
    private fun text() = notification()?.extras?.getCharSequence(Notification.EXTRA_TEXT)?.toString()
    // Android 12+ may defer a DEFAULT FGS notification even after the service is
    // foreground. Keep the service-start assertion separate and unchanged; only
    // the notification observation accounts for the emulator's actual policy.
    // AOSP ActivityManagerConstants: deferred_fgs_notification_interval defaults
    // to 10_000ms, longer than this fixture's ordinary 5_000ms lifecycle budget.
    private val notificationDeferralMs: Long by lazy {
        if (Build.VERSION.SDK_INT < 31) 0L else {
            val descriptor = instrumentation.uiAutomation.executeShellCommand(
                "device_config get activity_manager deferred_fgs_notification_interval",
            )
            android.os.ParcelFileDescriptor.AutoCloseInputStream(descriptor).bufferedReader().use {
                it.readText().trim().toLongOrNull()?.coerceAtLeast(0L) ?: 10_000L
            }
        }
    }

    private fun awaitText(expected: String) {
        await("runtime must become foreground before notification display") { foreground() }
        await("foreground notification: $expected (system deferral=${notificationDeferralMs}ms)",
            timeoutMs = notificationDeferralMs + 5000L,
        ) { foreground() && text() == expected }
    }
    private fun await(message: String, timeoutMs: Long = 5000L, predicate: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + timeoutMs
        do {
            instrumentation.waitForIdleSync()
            if (predicate()) return
            SystemClock.sleep(25)
        } while (SystemClock.uptimeMillis() < deadline)
        assertTrue("$message; running=${running()}, foreground=${foreground()}, text=${text()}, " +
            "ime=${OpenLessRuntimeService.imeRequested}, auth=${OpenLessRuntimeService.authorizationActive()}",
            predicate(),
        )
    }
}
