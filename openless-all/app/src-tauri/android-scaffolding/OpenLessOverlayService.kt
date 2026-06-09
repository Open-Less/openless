package com.openless.app

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.graphics.Color
import android.graphics.PixelFormat
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.IBinder
import android.util.Log
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.Toast
import kotlin.math.abs

/**
 * Foreground service + TYPE_APPLICATION_OVERLAY floating dictation control.
 */
class OpenLessOverlayService : Service(), OpenLessOverlayBridge.OverlayStateListener {

    private var windowManager: WindowManager? = null
    private var rootView: FrameLayout? = null
    private var layoutParams: WindowManager.LayoutParams? = null
    private var recording = false
    private var processing = false
    private var keyboardVisible = false
    private var lastKeyboardTop = 0
    private var normalY = 120
    private var dragStartX = 0
    private var dragStartY = 0
    private var paramStartX = 0
    private var paramStartY = 0
    private var dragging = false

    private lateinit var iconButton: ImageView

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        OpenLessOverlayBridge.listener = this
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_SHOW -> showOverlay()
            ACTION_START_RECORDING -> {
                showOverlay()
                startRecordingFromOverlay()
            }
            ACTION_HIDE -> {
                hideOverlay()
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                    stopForeground(STOP_FOREGROUND_REMOVE)
                } else {
                    @Suppress("DEPRECATION")
                    stopForeground(true)
                }
                stopSelf()
            }
            ACTION_TOGGLE_EXPAND -> handleIconClick()
            ACTION_KEYBOARD_CHANGED -> handleKeyboardChanged(intent)
        }
        return START_STICKY
    }

    override fun onDestroy() {
        if (OpenLessOverlayBridge.listener === this) {
            OpenLessOverlayBridge.listener = null
        }
        hideOverlay()
        if (instance === this) {
            instance = null
        }
        super.onDestroy()
    }

    override fun onCapsuleStateChanged(state: String, message: String?) {
        when (state) {
            "recording" -> {
                recording = true
                processing = false
                if (!tryPromoteRecordingForeground()) {
                    OpenLessNative.nativeCancelDictation()
                    return
                }
                applyVisualState(OverlayVisualState.Recording)
            }
            "transcribing", "polishing" -> {
                recording = false
                processing = true
                applyVisualState(OverlayVisualState.Processing)
            }
            "done" -> {
                recording = false
                processing = false
                applyVisualState(OverlayVisualState.Idle)
            }
            "error" -> {
                recording = false
                processing = false
                applyVisualState(OverlayVisualState.Error)
                message?.takeIf { it.isNotBlank() }?.let { showToast(it) }
            }
            "cancelled", "idle" -> {
                recording = false
                processing = false
                applyVisualState(OverlayVisualState.Idle)
            }
        }
    }

    private fun showOverlay() {
        if (rootView != null) {
            if (keyboardVisible) {
                rootView?.post { moveAboveKeyboard(lastKeyboardTop) }
            }
            return
        }
        windowManager = getSystemService(WINDOW_SERVICE) as WindowManager
        val params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
            } else {
                @Suppress("DEPRECATION")
                WindowManager.LayoutParams.TYPE_PHONE
            },
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL or
                WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            x = dp(24)
            y = normalY
        }
        layoutParams = params

        val root = FrameLayout(this)
        iconButton = buildIconButton()
        root.addView(
            iconButton,
            FrameLayout.LayoutParams(dp(ICON_SIZE_DP), dp(ICON_SIZE_DP), Gravity.CENTER),
        )
        attachDragHandler(iconButton, params)
        windowManager?.addView(root, params)
        rootView = root
        applyVisualState(
            when {
                recording -> OverlayVisualState.Recording
                processing -> OverlayVisualState.Processing
                else -> OverlayVisualState.Idle
            },
        )
        if (keyboardVisible) {
            root.post { moveAboveKeyboard(lastKeyboardTop) }
        }
    }

    private fun hideOverlay() {
        val view = rootView ?: return
        windowManager?.removeView(view)
        rootView = null
        layoutParams = null
    }

    private fun buildIconButton(): ImageView {
        return ImageView(this).apply {
            setImageResource(R.mipmap.ic_launcher_foreground)
            scaleType = ImageView.ScaleType.CENTER_INSIDE
            setPadding(dp(10), dp(10), dp(10), dp(10))
            contentDescription = "OpenLess"
            isClickable = true
            isFocusable = false
            setOnClickListener { handleIconClick() }
        }
    }

    private fun handleIconClick() {
        if (processing) return
        if (recording) {
            OpenLessNative.nativeStopDictation()
        } else {
            startRecordingFromOverlay()
        }
    }

    private fun handleKeyboardChanged(intent: Intent) {
        if (!isKeyboardTriggerMode()) return
        val visible = intent.getBooleanExtra(EXTRA_KEYBOARD_VISIBLE, false)
        keyboardVisible = visible
        if (visible) {
            lastKeyboardTop = intent.getIntExtra(EXTRA_KEYBOARD_TOP, 0)
            showOverlay()
            rootView?.post { moveAboveKeyboard(lastKeyboardTop) }
            return
        }
        if (recording || processing) {
            restoreNormalPosition()
            return
        }
        hideOverlay()
    }

    private fun moveAboveKeyboard(keyboardTop: Int) {
        val params = layoutParams ?: return
        val root = rootView ?: return
        if (keyboardTop <= 0) return
        val iconSize = overlaySize()
        val minY = dp(8)
        val maxY = (keyboardTop - iconSize - dp(12)).coerceAtLeast(minY)
        if (params.y > maxY || params.y < minY) {
            params.y = params.y.coerceIn(minY, maxY)
        }
        val maxX = (resources.displayMetrics.widthPixels - iconSize - dp(8)).coerceAtLeast(dp(8))
        params.x = params.x.coerceIn(dp(8), maxX)
        windowManager?.updateViewLayout(root, params)
    }

    private fun restoreNormalPosition() {
        val params = layoutParams ?: return
        val root = rootView ?: return
        params.y = normalY
        windowManager?.updateViewLayout(root, params)
    }

    private fun attachDragHandler(view: View, params: WindowManager.LayoutParams) {
        view.setOnTouchListener { touchedView, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    dragging = false
                    dragStartX = event.rawX.toInt()
                    dragStartY = event.rawY.toInt()
                    paramStartX = params.x
                    paramStartY = params.y
                    true
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = event.rawX.toInt() - dragStartX
                    val dy = event.rawY.toInt() - dragStartY
                    if (abs(dx) > DRAG_SLOP_PX || abs(dy) > DRAG_SLOP_PX) {
                        dragging = true
                        params.x = paramStartX + dx
                        params.y = paramStartY + dy
                        if (keyboardVisible) {
                            moveAboveKeyboard(lastKeyboardTop)
                        } else {
                            normalY = params.y
                            rootView?.let { windowManager?.updateViewLayout(it, params) }
                        }
                    }
                    true
                }
                MotionEvent.ACTION_UP -> {
                    if (!dragging) {
                        touchedView.performClick()
                    }
                    true
                }
                MotionEvent.ACTION_CANCEL -> true
                else -> false
            }
        }
    }

    private fun applyVisualState(state: OverlayVisualState) {
        if (!::iconButton.isInitialized) return
        val (alpha, fill, stroke, strokeWidth, enabled) = when (state) {
            OverlayVisualState.Idle -> VisualStyle(
                alpha = 0.58f,
                fill = Color.parseColor("#66202A36"),
                stroke = Color.parseColor("#66FFFFFF"),
                strokeWidth = 1,
                enabled = true,
            )
            OverlayVisualState.Recording -> VisualStyle(
                alpha = 1f,
                fill = Color.parseColor("#E6111827"),
                stroke = Color.parseColor("#F43F5E"),
                strokeWidth = 3,
                enabled = true,
            )
            OverlayVisualState.Processing -> VisualStyle(
                alpha = 0.86f,
                fill = Color.parseColor("#D1111827"),
                stroke = Color.parseColor("#38BDF8"),
                strokeWidth = 2,
                enabled = true,
            )
            OverlayVisualState.Error -> VisualStyle(
                alpha = 0.95f,
                fill = Color.parseColor("#E67F1D1D"),
                stroke = Color.parseColor("#EF4444"),
                strokeWidth = 2,
                enabled = true,
            )
        }
        iconButton.alpha = alpha
        iconButton.isEnabled = enabled
        iconButton.background = circleDrawable(fill, stroke, dp(strokeWidth))
    }

    private fun startRecordingFromOverlay() {
        showOverlay()
        if (tryPromoteRecordingForeground()) {
            OpenLessNative.nativeStartDictation()
            return
        }
        applyVisualState(OverlayVisualState.Error)
    }

    private fun tryPromoteRecordingForeground(): Boolean {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            showToast("请先授予麦克风权限")
            return false
        }
        val notification = buildNotification("录音中")
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE,
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
            true
        } catch (error: SecurityException) {
            Log.w(TAG, "microphone foreground service not allowed from current state", error)
            showToast("系统限制后台录音，请在 OpenLess 内开始")
            false
        }
    }

    private fun buildNotification(contentText: String): Notification {
        val channelId = "openless_overlay"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val nm = getSystemService(NotificationManager::class.java)
            nm.createNotificationChannel(
                NotificationChannel(channelId, "OpenLess Overlay", NotificationManager.IMPORTANCE_LOW),
            )
        }
        return Notification.Builder(this, channelId)
            .setContentTitle("OpenLess")
            .setContentText(contentText)
            .setSmallIcon(R.mipmap.ic_launcher_foreground)
            .build()
    }

    private fun circleDrawable(color: Int, strokeColor: Int, strokeWidth: Int): GradientDrawable {
        return GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(color)
            setStroke(strokeWidth, strokeColor)
        }
    }

    private fun overlaySize(): Int {
        val root = rootView
        val measured = maxOf(root?.width ?: 0, root?.height ?: 0)
        return measured.takeIf { it > 0 } ?: dp(ICON_SIZE_DP)
    }

    private fun isKeyboardTriggerMode(): Boolean {
        return try {
            OpenLessNative.nativeGetOverlayTriggerMode() == "keyboard"
        } catch (error: Throwable) {
            Log.w(TAG, "overlay trigger mode unavailable", error)
            false
        }
    }

    private fun showToast(message: String) {
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
    }

    private fun dp(value: Int): Int {
        return (value * resources.displayMetrics.density).toInt()
    }

    private data class VisualStyle(
        val alpha: Float,
        val fill: Int,
        val stroke: Int,
        val strokeWidth: Int,
        val enabled: Boolean,
    )

    private enum class OverlayVisualState {
        Idle,
        Recording,
        Processing,
        Error,
    }

    companion object {
        const val ACTION_SHOW = "com.openless.app.overlay.SHOW"
        const val ACTION_HIDE = "com.openless.app.overlay.HIDE"
        const val ACTION_TOGGLE_EXPAND = "com.openless.app.overlay.TOGGLE_EXPAND"
        const val ACTION_START_RECORDING = "com.openless.app.overlay.START_RECORDING"
        const val ACTION_KEYBOARD_CHANGED = "com.openless.app.overlay.KEYBOARD_CHANGED"
        const val EXTRA_KEYBOARD_VISIBLE = "keyboard_visible"
        const val EXTRA_KEYBOARD_TOP = "keyboard_top"
        const val EXTRA_KEYBOARD_BOTTOM = "keyboard_bottom"
        private const val ICON_SIZE_DP = 72
        private const val DRAG_SLOP_PX = 8
        private const val NOTIFICATION_ID = 42001
        private const val TAG = "OpenLessOverlayService"

        @Volatile
        var instance: OpenLessOverlayService? = null
            private set
    }
}
