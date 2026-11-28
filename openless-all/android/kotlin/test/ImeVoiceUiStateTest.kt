package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * SDK-free JUnit4：只验证纯投影语义，不涉及 Android/Service/JNI/Gradle。
 */
class ImeVoiceUiStateTest {

    private fun project(
        state: String = "idle",
        recording: Boolean = false,
        processing: Boolean = false,
        cloudSubmitting: Boolean = false,
        raw: Boolean = false,
        quickNote: Boolean = false,
        cloud: Boolean = false,
        message: String = "msg",
        level: Float = 0f,
        awaitingStart: Boolean = false,
    ): ImeVoiceUiSnapshot = ImeVoiceUiProjection.fromLegacy(
        state = state,
        recording = recording,
        processing = processing,
        cloudSubmitting = cloudSubmitting,
        raw = raw,
        quickNote = quickNote,
        cloud = cloud,
        message = message,
        level = level,
        awaitingStart = awaitingStart,
    )

    // --- 状态优先级 ---

    @Test
    fun recordingWinsOverProcessingAndCloudSubmitting() {
        val snapshot = project(recording = true, processing = true, cloudSubmitting = true)
        assertEquals(ImeVoicePhase.RECORDING, snapshot.phase)
    }

    @Test
    fun awaitingStartOnlyTurnsRecordingIntoStarting() {
        assertEquals(ImeVoicePhase.STARTING, project(recording = true, awaitingStart = true).phase)
        assertEquals(
            ImeVoicePhase.PROCESSING,
            project(processing = true, awaitingStart = true).phase,
        )
        assertEquals(ImeVoicePhase.IDLE, project(awaitingStart = true).phase)
    }

    @Test
    fun processingWinsOverCloudSubmitting() {
        val snapshot = project(processing = true, cloudSubmitting = true)
        assertEquals(ImeVoicePhase.PROCESSING, snapshot.phase)
        assertTrue(snapshot.processing)
    }

    @Test
    fun cloudSubmittingIsSubmittingAndNotCancellable() {
        val snapshot = project(cloudSubmitting = true)
        assertEquals(ImeVoicePhase.SUBMITTING, snapshot.phase)
        assertTrue(snapshot.editorLocked)
        assertFalse("webhook 提交不承诺可取消", snapshot.canCancel)
        assertTrue("旧 cloudPost 未完成时允许新录音", snapshot.canStart)
        assertFalse(snapshot.recording)
        assertFalse(snapshot.processing)
    }

    // --- legacy 终态映射 ---

    @Test
    fun legacyTerminalStatesMapToTheirPhases() {
        assertEquals(ImeVoicePhase.DONE, project(state = "done").phase)
        assertEquals(ImeVoicePhase.CANCELLED, project(state = "cancelled").phase)
        assertEquals(ImeVoicePhase.CANCELLED, project(state = "canceled").phase)
        assertEquals(ImeVoicePhase.ERROR, project(state = "error").phase)
        assertEquals(ImeVoicePhase.IDLE, project(state = "idle").phase)
        assertEquals(ImeVoicePhase.IDLE, project(state = "").phase)
        assertEquals(ImeVoicePhase.IDLE, project(state = "unknown-state").phase)
        assertEquals(ImeVoicePhase.DONE, project(state = "  DONE ").phase)
    }

    @Test
    fun legacyBusyStringsWithoutFlagsDoNotFabricateWork() {
        for (state in listOf("speaking", "thinking", "recording", "processing")) {
            val snapshot = project(state = state)
            assertEquals("无真实 flag 不得虚构录音/处理（$state）", ImeVoicePhase.IDLE, snapshot.phase)
            assertFalse(snapshot.recording)
            assertFalse(snapshot.processing)
            assertFalse(snapshot.editorLocked)
            assertFalse(snapshot.canCancel)
            assertTrue(snapshot.canStart)
        }
    }

    @Test
    fun allPhasesAreReachable() {
        val phases = setOf(
            project().phase,
            project(recording = true, awaitingStart = true).phase,
            project(recording = true).phase,
            project(processing = true).phase,
            project(cloudSubmitting = true).phase,
            project(state = "done").phase,
            project(state = "cancelled").phase,
            project(state = "error").phase,
        )
        assertEquals(ImeVoicePhase.entries.toSet(), phases)
    }

    // --- 模式优先级 ---

    @Test
    fun modePriorityRawQuickCloudDictation() {
        assertEquals(ImeVoiceMode.RAW, project(raw = true, quickNote = true, cloud = true).mode)
        assertEquals(ImeVoiceMode.QUICK_NOTE, project(quickNote = true, cloud = true).mode)
        assertEquals(ImeVoiceMode.CLOUD_NOTE, project(cloud = true).mode)
        assertEquals(ImeVoiceMode.CLOUD_NOTE, project(cloudSubmitting = true).mode)
        assertEquals(ImeVoiceMode.DICTATION, project().mode)
        assertEquals(
            ImeVoiceMode.RAW,
            project(raw = true, cloudSubmitting = true).mode,
        )
    }

    // --- 电平 ---

    @Test
    fun audioLevelIsOnlyExposedWhileRecording() {
        assertEquals(0f, project(level = 0.8f).audioLevel, 0f)
        assertEquals(0f, project(processing = true, level = 0.8f).audioLevel, 0f)
        assertEquals(0f, project(cloudSubmitting = true, level = 0.8f).audioLevel, 0f)
        assertEquals(0.8f, project(recording = true, level = 0.8f).audioLevel, 0f)
    }

    @Test
    fun audioLevelClampsAndRejectsNonFiniteValues() {
        assertEquals(0f, project(recording = true, level = -1f).audioLevel, 0f)
        assertEquals(1f, project(recording = true, level = 5f).audioLevel, 0f)
        assertEquals(0f, project(recording = true, level = Float.NaN).audioLevel, 0f)
        assertEquals(0f, project(recording = true, level = Float.POSITIVE_INFINITY).audioLevel, 0f)
        assertEquals(0f, project(recording = true, level = Float.NEGATIVE_INFINITY).audioLevel, 0f)
        assertEquals(0.5f, project(recording = true, level = 0.5f).audioLevel, 0f)
    }

    // --- 文案与 flags ---

    @Test
    fun messageIsPreservedVerbatim() {
        val samples = listOf("", "点击开始说话", "Already done ✓", "  padded  ")
        for (text in samples) {
            assertEquals(text, project(message = text).message)
        }
    }

    @Test
    fun derivedFlagsFollowRealInputsOnly() {
        val idle = project()
        assertFalse(idle.recording)
        assertFalse(idle.processing)
        assertFalse(idle.editorLocked)
        assertFalse(idle.canCancel)
        assertTrue(idle.canStart)

        val recording = project(recording = true)
        assertTrue(recording.recording)
        assertFalse(recording.processing)
        assertTrue(recording.editorLocked)
        assertTrue(recording.canCancel)
        assertFalse(recording.canStart)

        val processing = project(processing = true)
        assertTrue(processing.processing)
        assertTrue(processing.editorLocked)
        assertTrue(processing.canCancel)
        assertFalse(processing.canStart)
    }

    // --- 128 种 flags 组合不变式 ---

    @Test
    fun allFlagCombinationsRespectInvariants() {
        val level = 0.4f
        val message = "flags-matrix"
        for (mask in 0..127) {
            val recording = mask and 1 != 0
            val processing = mask and 2 != 0
            val cloudSubmitting = mask and 4 != 0
            val raw = mask and 8 != 0
            val quickNote = mask and 16 != 0
            val cloud = mask and 32 != 0
            val awaitingStart = mask and 64 != 0
            val snapshot = project(
                recording = recording,
                processing = processing,
                cloudSubmitting = cloudSubmitting,
                raw = raw,
                quickNote = quickNote,
                cloud = cloud,
                message = message,
                level = level,
                awaitingStart = awaitingStart,
            )
            val label = "mask=$mask rec=$recording proc=$processing cloudSub=$cloudSubmitting " +
                "raw=$raw quick=$quickNote cloud=$cloud awaiting=$awaitingStart"

            val expectedPhase = when {
                recording -> if (awaitingStart) ImeVoicePhase.STARTING else ImeVoicePhase.RECORDING
                processing -> ImeVoicePhase.PROCESSING
                cloudSubmitting -> ImeVoicePhase.SUBMITTING
                else -> ImeVoicePhase.IDLE
            }
            assertEquals("phase ($label)", expectedPhase, snapshot.phase)

            val expectedMode = when {
                raw -> ImeVoiceMode.RAW
                quickNote -> ImeVoiceMode.QUICK_NOTE
                cloud || cloudSubmitting -> ImeVoiceMode.CLOUD_NOTE
                else -> ImeVoiceMode.DICTATION
            }
            assertEquals("mode ($label)", expectedMode, snapshot.mode)

            assertEquals("recording echo ($label)", recording, snapshot.recording)
            assertEquals("processing echo ($label)", processing, snapshot.processing)
            assertEquals(
                "editorLocked ($label)",
                recording || processing || cloudSubmitting,
                snapshot.editorLocked,
            )
            assertEquals("canCancel ($label)", recording || processing, snapshot.canCancel)
            assertEquals("canStart ($label)", !recording && !processing, snapshot.canStart)
            assertEquals("message ($label)", message, snapshot.message)
            assertEquals(
                "audioLevel ($label)",
                if (recording) level.coerceIn(0f, 1f) else 0f,
                snapshot.audioLevel,
                0f,
            )

            // 结构不变式：锁定 == 处于忙态；未锁定 == 终态/空闲；可取消只出现在采集/处理阶段。
            if (snapshot.editorLocked) {
                assertTrue(
                    "locked implies busy phase ($label)",
                    snapshot.phase in setOf(
                        ImeVoicePhase.STARTING,
                        ImeVoicePhase.RECORDING,
                        ImeVoicePhase.PROCESSING,
                        ImeVoicePhase.SUBMITTING,
                    ),
                )
            } else {
                assertTrue(
                    "unlocked implies terminal/idle phase ($label)",
                    snapshot.phase in setOf(
                        ImeVoicePhase.IDLE,
                        ImeVoicePhase.DONE,
                        ImeVoicePhase.CANCELLED,
                        ImeVoicePhase.ERROR,
                    ),
                )
            }
            if (snapshot.canCancel) {
                assertTrue(
                    "cancellable implies capture/processing phase ($label)",
                    snapshot.phase in setOf(
                        ImeVoicePhase.STARTING,
                        ImeVoicePhase.RECORDING,
                        ImeVoicePhase.PROCESSING,
                    ),
                )
            }
            assertTrue("audio level range ($label)", snapshot.audioLevel in 0f..1f)
        }
    }

    // --- 请求所有权 ---

    @Test
    fun ownerMatchesOnlyExactEpochAndRequestId() {
        val owner = ImeRequestOwner(epoch = 7L, requestId = 42L)
        assertTrue(owner.matches(7L, 42L))
        assertFalse(owner.matches(8L, 42L))
        assertFalse(owner.matches(7L, 43L))
        assertFalse(owner.matches(8L, 43L))
    }

    @Test
    fun ownerMatchesIsExactForZeroAndNegativeValues() {
        val zero = ImeRequestOwner(epoch = 0L, requestId = 0L)
        assertTrue(zero.matches(0L, 0L))
        assertFalse(zero.matches(1L, 0L))
        assertFalse(zero.matches(0L, 1L))

        val negative = ImeRequestOwner(epoch = -3L, requestId = -9L)
        assertTrue(negative.matches(-3L, -9L))
        assertFalse(negative.matches(-3L, 9L))
    }

    @Test
    fun ownerValueEqualityAndCopy() {
        val owner = ImeRequestOwner(1L, 2L)
        assertEquals(owner, ImeRequestOwner(1L, 2L))
        assertTrue(owner.matches(owner.epoch, owner.requestId))
        val newer = owner.copy(epoch = 3L)
        assertFalse(newer.matches(1L, 2L))
        assertTrue(newer.matches(3L, 2L))
    }
}
