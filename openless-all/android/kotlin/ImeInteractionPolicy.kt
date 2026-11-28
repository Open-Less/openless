package com.openless.app

/**
 * 统一互动策略（按键连发节奏）。
 *
 * 只承载"多久连发一次"这一展示/交互参数，不读偏好、不做 I/O、不依赖 Android；
 * 存储侧（settings/adapter）负责把用户偏好值转换成策略，再通过各 View 的
 * `applyInteractionPolicy()` 下发。
 */
@ConsistentCopyVisibility
internal data class ImeInteractionPolicy private constructor(
    val repeatIntervalMs: Long,
) {
    companion object {
        private const val MIN_REPEAT_MS = 30L
        private const val MAX_REPEAT_MS = 200L
        private const val DEFAULT_REPEAT_MS = 60L

        /** 默认连发间隔（60ms），与旧硬编码 65/66ms 的体感保持一致。 */
        val DEFAULT = ImeInteractionPolicy(DEFAULT_REPEAT_MS)

        /** 把持久化值收敛到受支持区间 [30, 200]ms；越界按边界值处理。 */
        fun fromStoredRepeat(value: Long): ImeInteractionPolicy =
            ImeInteractionPolicy(value.coerceIn(MIN_REPEAT_MS, MAX_REPEAT_MS))
    }
}
