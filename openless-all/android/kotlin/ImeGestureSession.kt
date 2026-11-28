package com.openless.app

import kotlin.math.abs

/**
 * One pointer sequence, one owner, at most one committed action.
 *
 * Android adapters own event delivery and hit testing; this class deliberately
 * has no View/Service dependency so cancellation and axis arbitration can be
 * replayed in JVM tests. A cancelled or multi-pointer sequence must call
 * [cancel], never [release]. Child scroll regions can disable either axis at
 * [begin], before an ancestor gets its first interception opportunity.
 */
internal class ImeGestureSession(
    private val touchSlop: Float,
    private val axisDominance: Float = 1.5f,
) {
    enum class Axis { NONE, HORIZONTAL, VERTICAL }
    enum class Direction { LEFT, RIGHT, UP, DOWN }

    var axis: Axis = Axis.NONE
        private set
    private var active = false
    private var startX = 0f
    private var startY = 0f
    private var horizontalAllowed = false
    private var upAllowed = false
    private var downAllowed = false

    init {
        require(touchSlop.isFinite() && touchSlop >= 0f)
        require(axisDominance.isFinite() && axisDominance >= 1f)
    }

    fun begin(
        x: Float,
        y: Float,
        horizontal: Boolean = false,
        up: Boolean = false,
        down: Boolean = false,
    ) {
        cancel()
        if (!x.isFinite() || !y.isFinite()) return
        active = true
        startX = x
        startY = y
        horizontalAllowed = horizontal
        upAllowed = up
        downAllowed = down
    }

    /** Once an axis wins, crossing a diagonal cannot transfer ownership. */
    fun move(x: Float, y: Float, allowDown: Boolean = true): Axis {
        if (!active) return Axis.NONE
        if (!x.isFinite() || !y.isFinite()) {
            cancel()
            return Axis.NONE
        }
        if (axis != Axis.NONE) return axis
        val dx = x - startX
        val dy = y - startY
        axis = when {
            horizontalAllowed && abs(dx) > touchSlop && abs(dx) > abs(dy) * axisDominance -> Axis.HORIZONTAL
            ((dy < 0f && upAllowed) || (dy > 0f && downAllowed && allowDown)) &&
                abs(dy) > touchSlop && abs(dy) > abs(dx) * axisDominance -> Axis.VERTICAL
            else -> Axis.NONE
        }
        return axis
    }

    /** Only a completed UP event may commit; release also ends ownership. */
    fun release(
        x: Float,
        y: Float,
        horizontalThreshold: Float,
        verticalThreshold: Float,
    ): Direction? {
        val result = if (!active || !x.isFinite() || !y.isFinite() ||
            !horizontalThreshold.isFinite() || horizontalThreshold < 0f ||
            !verticalThreshold.isFinite() || verticalThreshold < 0f
        ) null else {
            val dx = x - startX
            val dy = y - startY
            when {
                axis == Axis.HORIZONTAL && abs(dx) > horizontalThreshold ->
                    if (dx < 0f) Direction.LEFT else Direction.RIGHT
                axis == Axis.VERTICAL && dy < -verticalThreshold && upAllowed -> Direction.UP
                axis == Axis.VERTICAL && dy > verticalThreshold && downAllowed -> Direction.DOWN
                else -> null
            }
        }
        cancel()
        return result
    }

    fun cancel() {
        active = false
        axis = Axis.NONE
        horizontalAllowed = false
        upAllowed = false
        downAllowed = false
    }
}
