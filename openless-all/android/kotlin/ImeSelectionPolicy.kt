package com.openless.app

/**
 * 输入法编辑区间（绝对 UTF-16 偏移）。
 *
 * `anchor` 是扩展起点（保持不动），`active` 是随方向移动的一端；两者顺序不保证
 * `anchor <= active`（反向选择是合法的）。
 */
internal data class ImeSelectionRange(val anchor: Int, val active: Int)

/**
 * Range 选区的纯策略：只做 UTF-16/codepoint 级别的单步扩展。
 *
 * 关键约束：
 * - `text` 为 `CharSequence?`，返回**绝对**偏移；`null`、负 `startOffset`、
 *   未知/越界 `selectionStart/End`、Int 溢出、非法 `direction` 一律返回 `null`，
 *   绝不猜测 0；
 * - `selectionStart/End` 是相对 extracted text 的局部偏移，必须加上 `startOffset`
 *   才是绝对位置；
 * - `previous` 只有其 `min/max` 与当前快照的绝对 selection **完全一致**才复用，
 *   否则用当前 live selection 重新建立（外部移动 caret 不沿用旧 anchor）；
 * - `active` 按 [Character.offsetByCodePoints] 跨一个完整 codepoint，不切 surrogate
 *   pair；到达 0 / text.length 边界时保持原 range；
 * - 校验 `start/end/active` 不落在合法 surrogate pair 中间，否则返回 `null`。
 *
 * 注意：这里只承诺 **codepoint** 粒度，不承诺完整 grapheme cluster；ZWJ 序列
 * （如 👨‍👩‍👧）会按 codepoint 单步推进，不保证一次跳过整个可视字形。
 * 纯函数：无 Native/JNI/IO/偏好，可直接 JVM 测试。
 */
internal object ImeSelectionPolicy {

    fun extend(
        text: CharSequence?,
        startOffset: Int,
        selectionStart: Int,
        selectionEnd: Int,
        previous: ImeSelectionRange?,
        direction: Int,
    ): ImeSelectionRange? {
        if (text == null) return null
        if (direction != -1 && direction != 1) return null
        if (startOffset < 0) return null
        if (selectionStart < 0 || selectionEnd < 0) return null

        val spanLength = text.length
        // All absolute offsets are Int-based in the public API; a window whose end
        // would not fit cannot be represented without overflow.
        val maxAbsolute = startOffset.toLong() + spanLength.toLong()
        if (maxAbsolute < 0L || maxAbsolute > Int.MAX_VALUE.toLong()) return null

        val liveStartAbsolute = startOffset.toLong() + selectionStart.toLong()
        val liveEndAbsolute = startOffset.toLong() + selectionEnd.toLong()
        if (liveStartAbsolute < 0L || liveStartAbsolute > maxAbsolute) return null
        if (liveEndAbsolute < 0L || liveEndAbsolute > maxAbsolute) return null

        // Local offsets index into `text`; only these may be used with Character/text APIs.
        val liveStartLocal = (liveStartAbsolute - startOffset).toInt()
        val liveEndLocal = (liveEndAbsolute - startOffset).toInt()
        if (isInsideSurrogatePair(text, liveStartLocal) || isInsideSurrogatePair(text, liveEndLocal)) {
            return null
        }

        val liveMin = minOf(liveStartAbsolute, liveEndAbsolute)
        val liveMax = maxOf(liveStartAbsolute, liveEndAbsolute)
        val reusable = previous != null &&
            minOf(previous.anchor, previous.active).toLong() == liveMin &&
            maxOf(previous.anchor, previous.active).toLong() == liveMax

        val anchor: Int
        val baseActiveLocal: Int
        if (reusable) {
            val kept = previous
            // The stored range is what gets handed back, so validate it as well.
            if (kept.anchor < startOffset || kept.anchor > maxAbsolute) return null
            if (kept.active < startOffset || kept.active > maxAbsolute) return null
            val keptAnchorLocal = kept.anchor - startOffset
            val keptActiveLocal = kept.active - startOffset
            if (isInsideSurrogatePair(text, keptAnchorLocal) ||
                isInsideSurrogatePair(text, keptActiveLocal)
            ) {
                return null
            }
            anchor = kept.anchor
            baseActiveLocal = keptActiveLocal
        } else {
            // External caret movement (or first use) re-bases on the live selection.
            anchor = liveStartAbsolute.toInt()
            baseActiveLocal = liveEndLocal
        }

        val nextLocal = when (direction) {
            1 -> if (baseActiveLocal >= spanLength) baseActiveLocal
            else Character.offsetByCodePoints(text, baseActiveLocal, 1)
            else -> if (baseActiveLocal <= 0) baseActiveLocal
            else Character.offsetByCodePoints(text, baseActiveLocal, -1)
        }
        val nextAbsolute = startOffset.toLong() + nextLocal.toLong()
        if (nextAbsolute < 0L || nextAbsolute > maxAbsolute) return null
        return ImeSelectionRange(anchor, nextAbsolute.toInt())
    }

    /** `index` 是否恰好落在合法 high/low surrogate pair 的中间。 */
    private fun isInsideSurrogatePair(text: CharSequence, index: Int): Boolean {
        if (index <= 0 || index >= text.length) return false
        return Character.isHighSurrogate(text[index - 1]) && Character.isLowSurrogate(text[index])
    }
}
