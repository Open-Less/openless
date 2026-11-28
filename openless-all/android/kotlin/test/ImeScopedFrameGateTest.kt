package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * SDK-free JUnit4：只验证帧门禁的归属/缓冲/退休语义，不依赖 Android/Service/JNI。
 */
class ImeScopedFrameGateTest {

    private fun owner(epoch: Long, requestId: Long) = ImeRequestOwner(epoch, requestId)

    private fun frame(
        sessionId: String?,
        state: String = "recording",
        message: String? = null,
        level: Float = 0f,
    ) = ImeCapsuleFrame(sessionId = sessionId, state = state, message = message, level = level)

    // --- 缓冲与 flush ---

    @Test
    fun framesBeforeBindAreBufferedAndFlushedOnBind() {
        val gate = ImeScopedFrameGate()
        val o = owner(1, 10)
        gate.begin(o)

        assertEquals(ImeFrameDisposition.BUFFERED, gate.accept(frame("A", "recording")))
        assertEquals(ImeFrameDisposition.BUFFERED, gate.accept(frame("A", "done", "ok", 0.7f)))
        assertEquals(1, gate.bufferedCount)

        val flushed = gate.bind(o, "A")
        assertEquals("done", flushed?.state)
        assertEquals("ok", flushed?.message)
        assertEquals(0.7f, flushed?.level ?: -1f, 0f)
        assertEquals(0, gate.bufferedCount)

        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A", "transcribing")))
    }

    @Test
    fun bufferedFramesForOtherIdsDoNotBecomeOwned() {
        val gate = ImeScopedFrameGate()
        val o = owner(1, 10)
        gate.begin(o)
        gate.accept(frame("B"))
        gate.accept(frame("C"))

        assertNull("绑定 A 时不能 flush B/C 的帧", gate.bind(o, "A"))
        assertEquals(0, gate.bufferedCount)
        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("B")))
        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("C")))
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
    }

    @Test
    fun freshIdBeforeBindIsBufferedButNeverOwned() {
        val gate = ImeScopedFrameGate()
        gate.begin(owner(2, 20))
        assertEquals(ImeFrameDisposition.BUFFERED, gate.accept(frame("real-id", "level", null, 0.3f)))
        assertEquals(1, gate.bufferedCount)
    }

    // --- 无 id / 无 owner ---

    @Test
    fun framesWithoutIdAreUnidentifiedAndCannotChangeOwnState() {
        val gate = ImeScopedFrameGate()
        val o = owner(3, 30)
        gate.begin(o)
        gate.bind(o, "A")

        assertEquals(ImeFrameDisposition.UNIDENTIFIED, gate.accept(frame(null)))
        assertEquals(ImeFrameDisposition.UNIDENTIFIED, gate.accept(frame("")))
        assertEquals(ImeFrameDisposition.UNIDENTIFIED, gate.accept(frame("   ")))
        // 无 id 帧不进缓存，也不影响绑定。
        assertEquals(0, gate.bufferedCount)
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
    }

    @Test
    fun framesWithoutOwnerAreExternal() {
        val gate = ImeScopedFrameGate()
        assertEquals(ImeFrameDisposition.EXTERNAL, gate.accept(frame("A")))
        assertEquals(ImeFrameDisposition.EXTERNAL, gate.accept(frame(null)))
        assertEquals(ImeFrameDisposition.EXTERNAL, gate.accept(frame("   ")))
        assertEquals(0, gate.bufferedCount)
    }

    @Test
    fun blankIdIsNotRetirable() {
        val gate = ImeScopedFrameGate()
        gate.retire("")
        gate.retire("   ")
        assertEquals(0, gate.retiredCount)
    }

    // --- 归属与绑定 ---

    @Test
    fun otherSessionIdIsForeignOnceBound() {
        val gate = ImeScopedFrameGate()
        val o = owner(4, 40)
        gate.begin(o)
        gate.bind(o, "A")

        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("B")))
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
    }

    @Test
    fun bindRejectsWrongOwnerBlankAndRetiredIds() {
        val gate = ImeScopedFrameGate()
        val o = owner(5, 50)
        gate.begin(o)

        assertNull(gate.bind(owner(9, 90), "A"))
        assertNull(gate.bind(o, ""))
        assertNull(gate.bind(o, "   "))

        gate.retire("R")
        assertNull(gate.bind(o, "R"))
        // 拒绝的 bind 不应改变可用状态：仍可绑定真实的非退休 id。
        assertNull(gate.bind(o, "A"))
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
    }

    @Test
    fun bindCannotRebindToAnotherId() {
        val gate = ImeScopedFrameGate()
        val o = owner(6, 60)
        gate.begin(o)
        gate.bind(o, "A")

        assertNull(gate.bind(o, "B"))
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("B")))
    }

    @Test
    fun repeatBindIsIdempotentAndKeepsTarget() {
        val gate = ImeScopedFrameGate()
        val o = owner(7, 70)
        gate.begin(o)
        assertNull(gate.bind(o, "A"))

        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
        assertNull("重复 bind 不应清掉目标", gate.bind(o, "A"))
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
    }

    @Test
    fun bindClearsAllBufferedFrames() {
        val gate = ImeScopedFrameGate()
        val o = owner(8, 80)
        gate.begin(o)
        gate.accept(frame("A", "recording"))
        gate.accept(frame("B", "recording"))
        assertEquals(2, gate.bufferedCount)

        val flushed = gate.bind(o, "A")
        assertEquals("A", flushed?.sessionId)
        assertEquals(0, gate.bufferedCount)
        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("B")))
    }

    // --- isBound（只读确认） ---

    @Test
    fun isBoundIsFalseBeforeBindAndTrueAfterwards() {
        val gate = ImeScopedFrameGate()
        val o = owner(30, 300)
        gate.begin(o)
        assertTrue(!gate.isBound(o, "A"))

        assertNull(gate.bind(o, "A"))
        assertTrue(gate.isBound(o, "A"))
        // 重复 bind（无缓存时返回 null）后仍处于已绑定状态。
        assertNull(gate.bind(o, "A"))
        assertTrue(gate.isBound(o, "A"))
    }

    @Test
    fun isBoundDistinguishesRejectedBindFromEmptyBuffer() {
        val gate = ImeScopedFrameGate()
        val o = owner(31, 310)
        gate.begin(o)
        gate.accept(frame("A", "done"))

        val flushed = gate.bind(o, "A")
        assertEquals("done", flushed?.state)
        assertTrue("bind 有缓存时返回帧且 isBound 为真", gate.isBound(o, "A"))

        val otherGate = ImeScopedFrameGate()
        otherGate.begin(o)
        assertNull("无缓存时 bind 返回 null 但不代表拒绝", otherGate.bind(o, "B"))
        assertTrue("无缓存但绑定成功 → isBound 为真", otherGate.isBound(o, "B"))
        assertTrue("被拒绝的 bind（错误 owner）→ isBound 为假", !otherGate.isBound(owner(9, 90), "B"))
    }

    @Test
    fun isBoundRejectsOldOwnerOtherIdsAndBlankIds() {
        val gate = ImeScopedFrameGate()
        val o = owner(32, 320)
        gate.begin(o)
        gate.bind(o, "A")

        assertTrue(!gate.isBound(o, "B"))
        assertTrue(!gate.isBound(o, ""))
        assertTrue(!gate.isBound(o, "   "))
        assertTrue("不 trim，opaque 精确匹配", !gate.isBound(o, " A"))
        assertTrue(!gate.isBound(owner(32, 321), "A"))
        assertTrue(!gate.isBound(owner(99, 320), "A"))
    }

    @Test
    fun isBoundIsFalseAfterFinishAndRetire() {
        val gate = ImeScopedFrameGate()
        val o = owner(33, 330)
        gate.begin(o)
        gate.bind(o, "A")
        assertTrue(gate.isBound(o, "A"))

        gate.finish(o)
        assertTrue(!gate.isBound(o, "A"))
        assertTrue(!gate.isBound(owner(33, 331), "A"))

        val other = ImeScopedFrameGate()
        other.retire("X")
        other.begin(o)
        assertTrue("退休 id 不能被认为已绑定", !other.isBound(o, "X"))
    }

    @Test
    fun isBoundStaysTrueAfterMisplacedRetireOfActiveId() {
        val gate = ImeScopedFrameGate()
        val o = owner(34, 340)
        gate.begin(o)
        gate.bind(o, "A")

        gate.retire("A") // active retire no-op
        assertTrue("误 retire 活跃 id 后绑定必须保持", gate.isBound(o, "A"))
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))

        gate.finish(o)
        assertTrue(!gate.isBound(o, "A"))
    }

    // --- begin / finish 与退休 ---

    @Test
    fun beginRetiresPreviousBoundId() {
        val gate = ImeScopedFrameGate()
        val first = owner(9, 90)
        val second = owner(9, 91)
        gate.begin(first)
        gate.bind(first, "old-id")

        gate.begin(second)
        assertEquals("旧绑定 id 的迟到帧必须 STALE", ImeFrameDisposition.STALE, gate.accept(frame("old-id")))
        assertEquals(ImeFrameDisposition.BUFFERED, gate.accept(frame("new-id")))
        assertEquals("new-id", gate.bind(second, "new-id")?.sessionId)
    }

    @Test
    fun finishRetiresBoundIdAndLateFramesAreStale() {
        val gate = ImeScopedFrameGate()
        val o = owner(10, 100)
        gate.begin(o)
        gate.bind(o, "A")
        gate.finish(o)

        assertEquals(1, gate.retiredCount)
        assertEquals(ImeFrameDisposition.STALE, gate.accept(frame("A")))
        // 结束后回到无 owner：其他 id 一律 EXTERNAL。
        assertEquals(ImeFrameDisposition.EXTERNAL, gate.accept(frame("B")))
    }

    @Test
    fun finishWithNullOwnerIsNoOp() {
        val gate = ImeScopedFrameGate()
        val o = owner(11, 110)
        gate.begin(o)
        gate.bind(o, "A")
        gate.finish(null)

        assertEquals("无主清理不能结束当前 owner", ImeFrameDisposition.OWNED, gate.accept(frame("A")))
    }

    @Test
    fun staleOwnerCannotBindOrFinishNewOwner() {
        val gate = ImeScopedFrameGate()
        val oldOwner = owner(12, 120)
        val newOwner = owner(12, 121)
        gate.begin(oldOwner)
        gate.bind(oldOwner, "old-id")
        gate.begin(newOwner)

        assertNull("过期 owner 不能 bind", gate.bind(oldOwner, "new-id"))
        gate.finish(oldOwner)
        assertEquals("过期 owner 不能 finish 新 owner", ImeFrameDisposition.BUFFERED, gate.accept(frame("new-id")))
        assertEquals("new-id", gate.bind(newOwner, "new-id")?.sessionId)
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("new-id")))
    }

    @Test
    fun finishClearsBuffer() {
        val gate = ImeScopedFrameGate()
        val o = owner(13, 130)
        gate.begin(o)
        gate.accept(frame("A"))
        assertEquals(1, gate.bufferedCount)
        gate.finish(o)
        assertEquals(0, gate.bufferedCount)
    }

    @Test
    fun beginClearsBuffer() {
        val gate = ImeScopedFrameGate()
        val first = owner(14, 140)
        gate.begin(first)
        gate.accept(frame("A"))
        gate.begin(owner(14, 141))
        assertEquals(0, gate.bufferedCount)
    }

    // --- retire ---

    @Test
    fun retiredBufferedIdIsRemovedAndCannotBeBound() {
        val gate = ImeScopedFrameGate()
        val o = owner(15, 150)
        gate.begin(o)
        gate.accept(frame("A"))
        assertEquals(1, gate.bufferedCount)

        gate.retire("A")
        assertEquals(0, gate.bufferedCount)
        assertNull(gate.bind(o, "A"))
        assertEquals(ImeFrameDisposition.STALE, gate.accept(frame("A")))
    }

    @Test
    fun retireOfActiveBoundIdIsNoOpAndDoesNotSilenceLiveSession() {
        val gate = ImeScopedFrameGate()
        val o = owner(16, 160)
        gate.begin(o)
        gate.bind(o, "A")

        // 误把当前 active id 传给 retire：必须 no-op，活跃帧仍然 OWNED。
        gate.retire("A")
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
        assertEquals(0, gate.retiredCount)
        // 绑定仍是 A：其他 id 依旧 FOREIGN。
        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("B")))

        // 只有 finish 才真正退休旧绑定 id，之后迟到帧才 STALE。
        gate.finish(o)
        assertEquals(1, gate.retiredCount)
        assertEquals(ImeFrameDisposition.STALE, gate.accept(frame("A")))
    }

    @Test
    fun retiredIdWinsOverOwnerBuffering() {
        val gate = ImeScopedFrameGate()
        gate.retire("dead-id")
        gate.begin(owner(17, 170))
        assertEquals(ImeFrameDisposition.STALE, gate.accept(frame("dead-id")))
        assertEquals(0, gate.bufferedCount)
    }

    // --- 边界 ---

    @Test
    fun bufferIsBoundedAndEvictsOldest() {
        val gate = ImeScopedFrameGate(bufferLimit = 2)
        val o = owner(18, 180)
        gate.begin(o)
        gate.accept(frame("A", "recording"))
        gate.accept(frame("B", "recording"))
        gate.accept(frame("C", "recording"))
        assertEquals(2, gate.bufferedCount)

        // 最旧的 A 已被逐出，bind A 得不到帧；C 仍可 flush。
        assertNull(gate.bind(o, "A"))
        assertEquals(0, gate.bufferedCount)
    }

    @Test
    fun bufferKeepsLatestFramePerIdAndRefreshesRecency() {
        val gate = ImeScopedFrameGate(bufferLimit = 2)
        val o = owner(19, 190)
        gate.begin(o)
        gate.accept(frame("A", "recording"))
        gate.accept(frame("B", "recording"))
        gate.accept(frame("A", "done")) // A 刷新为最新 + 最新内容
        gate.accept(frame("C", "recording")) // 逐出最久未更新的 B
        assertEquals(2, gate.bufferedCount)

        // A 被刷新过所以仍在缓存里，且返回的是最新内容 "done"。
        val flushed = gate.bind(o, "A")
        assertEquals("done", flushed?.state)
    }

    @Test
    fun retiredIsBoundedAndEvictsOldest() {
        val gate = ImeScopedFrameGate(retiredLimit = 2)
        gate.retire("A")
        gate.retire("B")
        gate.retire("C")
        assertEquals(2, gate.retiredCount)

        // A 已被逐出 → 不再 STALE；C 仍在。
        assertEquals(ImeFrameDisposition.EXTERNAL, gate.accept(frame("A")))
        assertEquals(ImeFrameDisposition.STALE, gate.accept(frame("C")))
    }

    @Test
    fun limitsAreCoercedToAtLeastOne() {
        val gate = ImeScopedFrameGate(bufferLimit = 0, retiredLimit = -5)
        val o = owner(20, 200)
        gate.begin(o)
        gate.accept(frame("A"))
        gate.accept(frame("B"))
        assertEquals(1, gate.bufferedCount)

        gate.retire("X")
        gate.retire("Y")
        assertEquals(1, gate.retiredCount)
    }

    // --- opaque id 语义 ---

    @Test
    fun idsAreExactOpaqueMatchWithoutTrimOrCaseFold() {
        val gate = ImeScopedFrameGate()
        val o = owner(21, 210)
        gate.begin(o)
        gate.bind(o, "A")

        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("A ")))
        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame(" A")))
        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("a")))
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(frame("A")))
    }

    @Test
    fun whitespaceAroundBufferedIdIsNotTrimmed() {
        val gate = ImeScopedFrameGate()
        val o = owner(22, 220)
        gate.begin(o)
        gate.accept(frame("A ", "recording"))
        gate.accept(frame("A", "done"))

        val flushed = gate.bind(o, "A")
        assertEquals("done", flushed?.state)
        assertEquals(ImeFrameDisposition.FOREIGN, gate.accept(frame("A ")))
    }

    // --- 帧内容不被解释 ---

    @Test
    fun frameStateMessageAndLevelAreNotInterpretedByGate() {
        val gate = ImeScopedFrameGate()
        val o = owner(23, 230)
        gate.begin(o)
        val weird = ImeCapsuleFrame("A", "totally-unknown-state", "long message ✓", 0.25f)
        assertEquals(ImeFrameDisposition.BUFFERED, gate.accept(weird))
        val flushed = gate.bind(o, "A")
        assertEquals(weird, flushed)
        assertEquals(ImeFrameDisposition.OWNED, gate.accept(weird))
    }
}
