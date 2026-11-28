package com.openless.app

/**
 * 胶囊状态帧（由桥/Service 原样传入）。`sessionId` 可为 null 或空/空白，
 * 表示"该帧未携带真实会话标识"；非空 id 一律按 **opaque 原样**使用（不 trim、不改写、不 casefold）。
 */
internal data class ImeCapsuleFrame(
    val sessionId: String?,
    val state: String,
    val message: String?,
    val level: Float,
)

/** 一帧的归属判定结果。 */
internal enum class ImeFrameDisposition {
    /** 与当前 owner 已绑定 id 精确一致：只有这种帧可以改自己的业务 flags/RMS。 */
    OWNED,

    /** 无 owner：帧来自外部（其他入口/悬浮球/热键），不得授权本入口动作。 */
    EXTERNAL,

    /** 有 owner 但尚未 bind：按 id 保留最新帧（有界），等 started 后 bind 再 flush。 */
    BUFFERED,

    /** 有 owner 且已绑定另一个 id：别的会话的帧，不得影响本入口。 */
    FOREIGN,

    /** 有 owner 但帧不带 id：无法归属，不得据此改 flags/RMS。 */
    UNIDENTIFIED,

    /** 该 id 已退休（本入口结束/取消过，或外部已知结束）：迟到帧，丢弃。 */
    STALE,
}

/**
 * IME 会话帧门禁：把"胶囊全局广播帧"收敛为"只属于本入口当前请求"的帧。
 *
 * 生命周期（由 Service 适配层驱动）：
 * 1. 发 start 前 [begin]：绑定当前 `epoch/requestId`，但**此时还不知道真实 sessionId**；
 * 2. started 前到达的真实帧按 id 有界缓存（只保留每个 id 的最新帧），不会授权任何动作；
 * 3. 收到 ack/started 后调用 [bind]：只接受同 owner、非空、非退休的 id；
 *    首次 bind 返回该 id 在缓存里的最后一帧（随后清空全部缓存），供适配层补投；
 * 4. 之后的帧按 [accept] 判定，只有 [ImeFrameDisposition.OWNED] 才允许驱动本入口状态；
 * 5. 结束/取消时 [finish]：只对匹配 owner 生效，并退休已知绑定 id，拦住晚到帧；
 * 6. 外部已知结束/取消的 id 可用 [retire] 提前退休。
 *
 * 保护性约定：
 * - 无 owner 的帧一律 [ImeFrameDisposition.EXTERNAL]；空白 id 视为"无 id"；
 * - [retire] 对**当前 active 绑定 id** 直接 no-op（误调用不误伤活跃会话）：
 *   活跃会话只由 [finish] 结束；[begin] 也会在切换时退休上一位 owner 的旧绑定 id；
 * - [finish] 传入 null 不做任何事（避免无主清理误伤新 owner）；过期 owner 的
 *   [bind]/[finish] 都不能影响新 owner；
 * - 有界：缓存按"最久未更新"逐出，退休表按插入顺序逐出；上限至少为 1；
 * - 纯内存结构，不做日志/I/O。
 */
internal class ImeScopedFrameGate(
    bufferLimit: Int = 4,
    retiredLimit: Int = 8,
) {
    private val bufferLimit: Int = bufferLimit.coerceAtLeast(1)
    private val retiredLimit: Int = retiredLimit.coerceAtLeast(1)

    private val buffer = LinkedHashMap<String, ImeCapsuleFrame>()
    private val retired = LinkedHashSet<String>()

    private var owner: ImeRequestOwner? = null
    private var boundSessionId: String? = null

    /** 当前缓存的不同 id 帧数（每个 id 只保留最新一帧）。 */
    val bufferedCount: Int get() = buffer.size

    /** 当前退休 id 数（有界）。 */
    val retiredCount: Int get() = retired.size

    /** 开始一次新请求：清理旧 owner/缓存，并退休旧绑定 id 防旧帧夺权。 */
    fun begin(owner: ImeRequestOwner) {
        retireInternal(boundSessionId)
        boundSessionId = null
        buffer.clear()
        this.owner = owner
    }

    /**
     * 收到真实 sessionId 后绑定当前 owner。
     *
     * 只接受：同 owner、非空、非退休 id，且不得从已绑定 id 改绑到另一个 id。
     * 重复 bind 同一 owner/id 是幂等的（保留绑定，不会丢目标）。
     *
     * @return 缓存中该 id 的最后一帧（若有），调用后完整清空缓存；拒绝时返回 null 且不改状态。
     */
    fun bind(owner: ImeRequestOwner, sessionId: String): ImeCapsuleFrame? {
        val current = this.owner ?: return null
        if (current != owner) return null
        if (!isKnownId(sessionId)) return null
        if (isRetired(sessionId)) return null
        val existing = boundSessionId
        if (existing != null && existing != sessionId) return null
        boundSessionId = sessionId
        val frame = buffer[sessionId]
        buffer.clear()
        return frame
    }

    /**
     * 只读确认：当前是否已把 [owner] 精确绑定到该非空、未退休 id。
     *
     * 用于区分 [bind] 返回 null 的两种情况：**拒绝绑定**（false）与**绑定成功但缓存无帧**（true）。
     * Service 可只在 `bind` 后 `isBound` 为 true 时才接收 started/进入录音态，不必伪造帧探测。
     */
    fun isBound(owner: ImeRequestOwner, sessionId: String): Boolean {
        if (owner != this.owner) return false
        if (!isKnownId(sessionId)) return false
        if (isRetired(sessionId)) return false
        return boundSessionId == sessionId
    }

    /** 判定一帧归属；只有 [ImeFrameDisposition.OWNED] 才允许改本入口业务状态。 */
    fun accept(frame: ImeCapsuleFrame): ImeFrameDisposition {
        val sessionId = frame.sessionId
        if (isKnownId(sessionId) && isRetired(sessionId!!)) return ImeFrameDisposition.STALE

        val current = owner ?: return ImeFrameDisposition.EXTERNAL
        if (!isKnownId(sessionId)) return ImeFrameDisposition.UNIDENTIFIED

        val id = sessionId!!
        val bound = boundSessionId
        return when {
            bound == null -> {
                bufferFrame(id, frame)
                ImeFrameDisposition.BUFFERED
            }
            bound == id -> ImeFrameDisposition.OWNED
            else -> ImeFrameDisposition.FOREIGN
        }
    }

    /**
     * 结束会话：仅当 [owner] 与当前 owner 完全一致时生效；退休已知绑定 id 并清缓存。
     * 传入 null 或过期 owner 都是无操作（过期 owner 不能结束新 owner）。
     */
    fun finish(owner: ImeRequestOwner?) {
        if (owner == null) return
        if (owner != this.owner) return
        retireInternal(boundSessionId)
        boundSessionId = null
        buffer.clear()
        this.owner = null
    }

    /**
     * 外部已知结束/取消的 id：提前退休以拦截晚到帧，并移除其缓存帧。
     *
     * 若传入的正是**当前 active 绑定 id**，直接 no-op：误调用不得把活跃会话的帧
     * 变成 STALE（等价屏蔽活跃 UI）。结束活跃会话只能走 [finish]（内部退休旧绑定）；
     * 切换会话由 [begin] 负责退休上一位 owner 的绑定 id。
     * 空白 id 忽略；不做 trim/casefold。
     */
    fun retire(sessionId: String) {
        if (!isKnownId(sessionId)) return
        if (sessionId == boundSessionId) return
        retireInternal(sessionId)
        buffer.remove(sessionId)
    }

    private fun bufferFrame(id: String, frame: ImeCapsuleFrame) {
        // 更新视为"最近使用"：先移除再插入，逐出最旧。
        buffer.remove(id)
        buffer[id] = frame
        while (buffer.size > bufferLimit) {
            val iterator = buffer.keys.iterator()
            if (!iterator.hasNext()) break
            iterator.next()
            iterator.remove()
        }
    }

    private fun retireInternal(sessionId: String?) {
        if (!isKnownId(sessionId)) return
        val id = sessionId!!
        retired.remove(id)
        retired.add(id)
        while (retired.size > retiredLimit) {
            val iterator = retired.iterator()
            if (!iterator.hasNext()) break
            iterator.next()
            iterator.remove()
        }
    }

    private fun isRetired(sessionId: String): Boolean = retired.contains(sessionId)

    /** 空白/空串一律视为"无 id"；非空 id 原样保留（不 trim、不改写）。 */
    private fun isKnownId(sessionId: String?): Boolean = sessionId != null && sessionId.isNotBlank()
}
