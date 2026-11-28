package com.openless.app

import android.content.Context
import android.content.ContextWrapper
import android.content.SharedPreferences
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * 真实 `OpenLessImeService` 的会话帧接线测试（不是 model/门禁单测的复读）。
 *
 * 边界与安全：
 * - 使用 isolated [ContextWrapper] 把 `getSharedPreferences` 重定向到测试命名空间，
 *   并预置 `key_haptic_enabled=false`，不写用户偏好、不振动；
 * - `new OpenLessImeService()` 后只做 `attachBaseContext`（沿用
 *   `ImeAutoTextIntegrationTest` 的模式），**绝不调用 onCreate/onDestroy**；
 * - 只调用已在源码中审读过的纯状态路径：
 *   `onCapsuleStateChangedWithSession` / `onImeSessionEvent(started|completed 拒绝路径)` /
 *   `ImeScopedFrameGate`，不调用 `toggleDictation`/`cancelImeSession`/`sendImeCommand`
 *   等会发 Native 命令的方法；不发送 start/stop/cancel；
 * - 所有断言在 `runOnMainSync` 之外执行（主线程内只做调用与状态采集）。
 *
 * 覆盖的真实路径：`imeFrameGate.accept` 归属判定 → `applyOwnedCapsuleFrame`
 * （经 `ImeVoiceFramePolicy`）→ `voiceUiState()` 投影 → `recording`/`processing`/
 * `latestVoiceLevel`/`externalRecording` 等真实字段。
 */
@RunWith(AndroidJUnit4::class)
class ImeSessionScopeInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    /** 隔离偏好：所有写入落到 `ime_scope_test__*` 命名空间，绝不碰真实用户偏好。 */
    private class IsolatedPrefsContext(base: Context) : ContextWrapper(base) {
        override fun getSharedPreferences(name: String?, mode: Int): SharedPreferences =
            super.getSharedPreferences("ime_scope_test__${name ?: "null"}", mode)
    }

    /** 主线程内采集的真实 Service 状态快照（断言在主线程外做）。 */
    private data class ScopeState(
        val recording: Boolean,
        val processing: Boolean,
        val latestVoiceLevel: Float,
        val externalRecording: Boolean,
        val externalProcessing: Boolean,
        val activeImeOwner: ImeRequestOwner?,
        val imeSessionId: String?,
        val imeStopRequested: Boolean,
        val imeCaptureEnded: Boolean,
        val pendingCloudArm: Boolean,
        val pendingImeStop: Boolean,
        val cloudNoteArmed: Boolean,
        val state: String,
        val lastDictationText: String?,
        val snapshot: ImeVoiceUiSnapshot,
    )

    // --- 构造与反射 ---

    private fun newService(): OpenLessImeService {
        val isolated = IsolatedPrefsContext(instrumentation.targetContext)
        isolated.getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE).edit()
            .putBoolean("key_haptic_enabled", false)
            .putLong("key_haptic_duration_ms", 1L)
            .putInt("key_haptic_amplitude", 1)
            .commit()
        val service = OpenLessImeService()
        ContextWrapper::class.java
            .getDeclaredMethod("attachBaseContext", Context::class.java)
            .apply { isAccessible = true }
            .invoke(service, isolated)
        return service
    }

    private fun setField(service: OpenLessImeService, name: String, value: Any?) {
        OpenLessImeService::class.java.getDeclaredField(name).apply { isAccessible = true }.set(service, value)
    }

    private fun getField(service: OpenLessImeService, name: String): Any? =
        OpenLessImeService::class.java.getDeclaredField(name).apply { isAccessible = true }.get(service)

    private fun call(service: OpenLessImeService, name: String, parameterType: Class<*>, argument: Any?): Any? =
        OpenLessImeService::class.java.getDeclaredMethod(name, parameterType)
            .apply { isAccessible = true }
            .invoke(service, argument)

    private fun gate(service: OpenLessImeService): ImeScopedFrameGate =
        getField(service, "imeFrameGate") as ImeScopedFrameGate

    private fun snapshot(service: OpenLessImeService): ScopeState = ScopeState(
        recording = service.recording,
        processing = getField(service, "processing") as Boolean,
        latestVoiceLevel = getField(service, "latestVoiceLevel") as Float,
        externalRecording = getField(service, "externalRecording") as Boolean,
        externalProcessing = getField(service, "externalProcessing") as Boolean,
        activeImeOwner = getField(service, "activeImeOwner") as ImeRequestOwner?,
        imeSessionId = getField(service, "imeSessionId") as String?,
        imeStopRequested = getField(service, "imeStopRequested") as Boolean,
        imeCaptureEnded = getField(service, "imeCaptureEnded") as Boolean,
        pendingCloudArm = getField(service, "pendingCloudArm") as Boolean,
        pendingImeStop = getField(service, "pendingImeStop") as Boolean,
        cloudNoteArmed = getField(service, "cloudNoteArmed") as Boolean,
        state = getField(service, "state") as String,
        lastDictationText = getField(service, "lastDictationText") as String?,
        snapshot = OpenLessImeService::class.java.getDeclaredMethod("voiceUiState")
            .apply { isAccessible = true }
            .invoke(service) as ImeVoiceUiSnapshot,
    )

    /** 绑定 owner（可选同时把 gate 绑定到真实 sessionId）。 */
    private fun arm(
        service: OpenLessImeService,
        epoch: Long,
        requestId: Long,
        boundSessionId: String? = null,
    ): ImeRequestOwner {
        val owner = ImeRequestOwner(epoch, requestId)
        setField(service, "sessionEpoch", epoch)
        setField(service, "imeRequestId", requestId)
        setField(service, "activeImeOwner", owner)
        setField(service, "microphoneGranted", true)
        gate(service).begin(owner)
        if (boundSessionId != null) {
            gate(service).bind(owner, boundSessionId)
            setField(service, "imeSessionId", boundSessionId)
        }
        return owner
    }

    private fun startedJson(requestId: Long, sessionId: String): String =
        """{"kind":"started","requestId":$requestId,"sessionId":"$sessionId"}"""

    // --- 测试 ---

    @Test
    fun ownedRecordingFrameUsesRealServiceSourceNoFakeBody() {
        var observed: ScopeState? = null
        instrumentation.runOnMainSync {
            val service = newService()
            arm(service, epoch = 7L, requestId = 70L, boundSessionId = "sess-A")

            service.onCapsuleStateChangedWithSession("recording", "再次点击结束", 0.35f, "sess-A")
            observed = snapshot(service)
        }
        val state = requireNotNull(observed)
        assertTrue(state.recording)
        assertFalse(state.processing)
        assertEquals(0.35f, state.latestVoiceLevel, 0.0001f)
        assertFalse(state.externalRecording)
        assertFalse(state.externalProcessing)
        assertEquals(ImeVoicePhase.RECORDING, state.snapshot.phase)
        assertEquals(0.35f, state.snapshot.audioLevel, 0.0001f)
        assertEquals("speaking", state.state)
    }

    @Test
    fun foreignIdAndUnidentifiedFramesCannotChangeOwnFlagsOrRms() {
        var observed: ScopeState? = null
        instrumentation.runOnMainSync {
            val service = newService()
            arm(service, epoch = 7L, requestId = 70L, boundSessionId = "sess-A")
            setField(service, "latestVoiceLevel", 0.42f)

            service.onCapsuleStateChangedWithSession("recording", "other", 0.9f, "sess-B")
            service.onCapsuleStateChangedWithSession("recording", "noid", 0.8f, null)
            observed = snapshot(service)
        }
        val state = requireNotNull(observed)
        assertFalse("FOREIGN/UNIDENTIFIED 帧不得开启自有录音", state.recording)
        assertEquals("RMS 不得被其他会话改写", 0.42f, state.latestVoiceLevel, 0.0001f)
        assertFalse(state.externalRecording)
        assertFalse(state.externalProcessing)
        assertEquals(ImeVoicePhase.IDLE, state.snapshot.phase)
    }

    @Test
    fun afterGateFinishLateOwnedFrameIsStaleNotExternalBusy() {
        var observed: ScopeState? = null
        instrumentation.runOnMainSync {
            val service = newService()
            val owner = arm(service, epoch = 7L, requestId = 70L, boundSessionId = "sess-A")
            gate(service).finish(owner)

            service.onCapsuleStateChangedWithSession("recording", "late", 0.9f, "sess-A")
            observed = snapshot(service)
        }
        val state = requireNotNull(observed)
        assertFalse(state.recording)
        assertFalse("STALE 晚帧不得变成 external busy", state.externalRecording)
        assertFalse(state.externalProcessing)
        assertEquals("RMS 保持 0", 0f, state.latestVoiceLevel, 0.0001f)
        assertEquals(ImeVoicePhase.IDLE, state.snapshot.phase)
    }

    @Test
    fun stopRequestedAndCaptureEndedLateRecordingDoesNotResurrect() {
        var observed: ScopeState? = null
        instrumentation.runOnMainSync {
            val service = newService()
            arm(service, epoch = 7L, requestId = 70L, boundSessionId = "sess-A")
            setField(service, "imeStopRequested", true)
            setField(service, "imeCaptureEnded", true)
            setField(service, "recording", false)
            setField(service, "processing", false)

            service.onCapsuleStateChangedWithSession("recording", "late", 0.9f, "sess-A")
            observed = snapshot(service)
        }
        val state = requireNotNull(observed)
        assertFalse("stop 已提交后的 late recording 不得复活", state.recording)
        assertFalse(state.processing)
        assertTrue(state.imeCaptureEnded)
        assertEquals(0f, state.latestVoiceLevel, 0.0001f)
    }

    @Test
    fun ownedDoneWithStopSubmittedHoldsProcessingAndOwnerUntilScopedResult() {
        var observed: ScopeState? = null
        instrumentation.runOnMainSync {
            val service = newService()
            arm(service, epoch = 7L, requestId = 70L, boundSessionId = "sess-A")
            setField(service, "imeStopRequested", true)
            setField(service, "imeCaptureEnded", true)
            setField(service, "recording", false)
            setField(service, "processing", false)

            service.onCapsuleStateChangedWithSession("done", "done-cue", 0f, "sess-A")
            observed = snapshot(service)
        }
        val state = requireNotNull(observed)
        assertTrue("done 只是 cue，应等待 scoped 结果", state.processing)
        assertFalse(state.recording)
        assertNotNull("等待 scoped 结果期间必须保留 owner", state.activeImeOwner)
        assertEquals("sess-A", state.imeSessionId)
        assertNull("不得写入任何听写文本", state.lastDictationText)
        assertEquals("thinking", state.state)
    }

    @Test
    fun startedEventFlushesOnlyBufferedMatchingFrame() {
        var before: ScopeState? = null
        var buffered: ScopeState? = null
        var bufferedCount = -1
        var after: ScopeState? = null
        var boundAfter = false
        instrumentation.runOnMainSync {
            val service = newService()
            arm(service, epoch = 7L, requestId = 70L) // owner 就绪但尚未 + 不知道真实 sessionId
            before = snapshot(service)

            // started 之前的真实帧：A 匹配、B 不匹配；全部只能进缓冲。
            service.onCapsuleStateChangedWithSession("recording", "pre-A", 0.4f, "sess-A")
            service.onCapsuleStateChangedWithSession("recording", "pre-B", 0.9f, "sess-B")
            buffered = snapshot(service)
            bufferedCount = gate(service).bufferedCount

            call(service, "onImeSessionEvent", String::class.java, startedJson(70L, "sess-A"))
            after = snapshot(service)
            boundAfter = gate(service).isBound(ImeRequestOwner(7L, 70L), "sess-A")
        }
        val initial = requireNotNull(before)
        assertFalse("默认不得 pending cloud", initial.pendingCloudArm)
        assertFalse(initial.cloudNoteArmed)
        assertFalse("默认不得 pending stop", initial.pendingImeStop)
        assertEquals(ImeVoicePhase.IDLE, initial.snapshot.phase)
        val bufferedState = requireNotNull(buffered)
        assertFalse("started 前不得开启录音", bufferedState.recording)
        assertEquals(0f, bufferedState.latestVoiceLevel, 0.0001f)
        assertEquals("A/B 两帧都应进缓冲", 2, bufferedCount)
        val flushed = requireNotNull(after)
        assertTrue("started 后只 flush 匹配 id 的缓冲帧", flushed.recording)
        assertEquals("必须使用 A 的电平(0.4)而非 B(0.9)", 0.4f, flushed.latestVoiceLevel, 0.0001f)
        assertEquals("sess-A", flushed.imeSessionId)
        assertTrue(boundAfter)
        assertFalse("started 处理不得凭空 arm cloud", flushed.pendingCloudArm)
        assertFalse(flushed.cloudNoteArmed)
        assertFalse("started 处理不得发出 pending stop", flushed.pendingImeStop)
        assertEquals(ImeVoicePhase.RECORDING, flushed.snapshot.phase)
        assertEquals("speaking", flushed.state)
    }

    @Test
    fun oldCompletedWithoutActiveOwnerIsRejectedWithoutEditorIo() {
        var observed: ScopeState? = null
        instrumentation.runOnMainSync {
            val service = newService()
            // 没有 active owner（例如新请求尚未建立或旧请求已释放）。
            setField(service, "sessionEpoch", 7L)
            setField(service, "imeRequestId", 70L)
            setField(service, "imeSessionId", "sess-X")
            setField(service, "activeImeOwner", null)
            setField(service, "lastDictationText", null)

            call(
                service,
                "onImeSessionEvent",
                String::class.java,
                """{"kind":"completed","requestId":70,"sessionId":"sess-X","text":"SHOULD_NOT_WRITE"}""",
            )
            observed = snapshot(service)
        }
        val state = requireNotNull(observed)
        assertNull("拒绝路径不得写入文本", state.lastDictationText)
        assertEquals("idle", state.state)
        assertFalse(state.recording)
        assertFalse(state.processing)
    }

    @Test
    fun unidentifiedFrameDoesNotResetRmsWhileRecording() {
        var observed: ScopeState? = null
        instrumentation.runOnMainSync {
            val service = newService()
            arm(service, epoch = 7L, requestId = 70L, boundSessionId = "sess-A")
            service.onCapsuleStateChangedWithSession("recording", "live", 0.3f, "sess-A")

            service.onCapsuleStateChangedWithSession("recording", "noid", 0.95f, null)
            service.onCapsuleStateChangedWithSession("recording", "other", 0.95f, "sess-B")
            observed = snapshot(service)
        }
        val state = requireNotNull(observed)
        assertTrue(state.recording)
        assertEquals("UNIDENTIFIED/FOREIGN 不得改自有 RMS", 0.3f, state.latestVoiceLevel, 0.0001f)
        assertEquals(ImeVoicePhase.RECORDING, state.snapshot.phase)
    }
}
