package com.openless.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.graphics.Color
import android.graphics.PixelFormat
import android.graphics.drawable.GradientDrawable
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.Manifest
import android.os.Build
import android.os.IBinder
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import kotlin.math.abs

/**
 * Foreground service + TYPE_APPLICATION_OVERLAY floating dictation control.
 */
class OpenLessOverlayService : Service(), OpenLessOverlayBridge.OverlayStateListener {

    private var windowManager: WindowManager? = null
    private var rootView: FrameLayout? = null
    private var layoutParams: WindowManager.LayoutParams? = null
    private var expanded = false
    private var recording = false
    private var dragStartX = 0
    private var dragStartY = 0
    private var paramStartX = 0
    private var paramStartY = 0
    private var dragging = false

    private lateinit var pillView: TextView
    private lateinit var panelView: LinearLayout
    private lateinit var statusView: TextView
    private lateinit var recordButton: TextView

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        OpenLessOverlayBridge.listener = this
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_SHOW -> {
                showOverlay()
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
            ACTION_TOGGLE_EXPAND -> toggleExpanded()
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
                promoteRecordingForeground()
                statusView.text = "录音中…"
                recordButton.text = "■"
            }
            "transcribing", "polishing" -> {
                recording = false
                statusView.text = if (state == "transcribing") "识别中…" else "润色中…"
                recordButton.text = "…"
                recordButton.isEnabled = false
            }
            "done" -> {
                recording = false
                statusView.text = message ?: "完成"
                recordButton.text = "●"
                recordButton.isEnabled = true
            }
            "error" -> {
                recording = false
                statusView.text = message ?: "出错"
                recordButton.text = "●"
                recordButton.isEnabled = true
            }
            "cancelled", "idle" -> {
                recording = false
                statusView.text = "就绪"
                recordButton.text = "●"
                recordButton.isEnabled = true
            }
        }
    }

    private fun showOverlay() {
        if (rootView != null) return
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
            x = 24
            y = 120
        }
        layoutParams = params

        val root = FrameLayout(this)
        pillView = buildPillView()
        panelView = buildPanelView()
        panelView.visibility = View.GONE
        root.addView(pillView)
        root.addView(panelView)
        attachDragHandler(root, params)
        windowManager?.addView(root, params)
        rootView = root
    }

    private fun hideOverlay() {
        val view = rootView ?: return
        windowManager?.removeView(view)
        rootView = null
        layoutParams = null
        expanded = false
    }

    private fun toggleExpanded() {
        expanded = !expanded
        pillView.visibility = if (expanded) View.GONE else View.VISIBLE
        panelView.visibility = if (expanded) View.VISIBLE else View.GONE
    }

    private fun buildPillView(): TextView {
        return TextView(this).apply {
            text = "OL"
            setTextColor(Color.WHITE)
            textSize = 14f
            gravity = Gravity.CENTER
            background = circleDrawable(Color.parseColor("#2563EB"))
            setPadding(24, 16, 24, 16)
            setOnClickListener {
                toggleExpanded()
            }
        }
    }

    private fun buildPanelView(): LinearLayout {
        statusView = TextView(this).apply {
            text = "就绪"
            setTextColor(Color.WHITE)
            textSize = 12f
        }
        recordButton = TextView(this).apply {
            text = "●"
            textSize = 28f
            setTextColor(Color.parseColor("#EF4444"))
            gravity = Gravity.CENTER
            setOnClickListener {
                if (recording) {
                    OpenLessNative.nativeStopDictation()
                } else {
                    promoteRecordingForeground()
                    expanded = true
                    panelView.visibility = View.VISIBLE
                    pillView.visibility = View.GONE
                    OpenLessNative.nativeStartDictation()
                }
            }
        }
        val collapse = TextView(this).apply {
            text = "—"
            setTextColor(Color.WHITE)
            textSize = 16f
            setOnClickListener {
                expanded = false
                panelView.visibility = View.GONE
                pillView.visibility = View.VISIBLE
            }
        }
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = roundedDrawable(Color.parseColor("#CC111827"))
            setPadding(24, 20, 24, 20)
            addView(collapse)
            addView(statusView)
            addView(recordButton)
        }
    }

    private fun attachDragHandler(root: View, params: WindowManager.LayoutParams) {
        root.setOnTouchListener { _, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    dragging = false
                    dragStartX = event.rawX.toInt()
                    dragStartY = event.rawY.toInt()
                    paramStartX = params.x
                    paramStartY = params.y
                    false
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = event.rawX.toInt() - dragStartX
                    val dy = event.rawY.toInt() - dragStartY
                    if (abs(dx) > 8 || abs(dy) > 8) {
                        dragging = true
                        params.x = paramStartX + dx
                        params.y = paramStartY + dy
                        windowManager?.updateViewLayout(root, params)
                    }
                    true
                }
                MotionEvent.ACTION_UP -> dragging
                else -> false
            }
        }
    }

    private fun circleDrawable(color: Int): GradientDrawable {
        return GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(color)
        }
    }

    private fun roundedDrawable(color: Int): GradientDrawable {
        return GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = 24f
            setColor(color)
        }
    }

    private fun promoteRecordingForeground() {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            return
        }
        val notification = buildNotification("录音中")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
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
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .build()
    }

    companion object {
        const val ACTION_SHOW = "com.openless.app.overlay.SHOW"
        const val ACTION_HIDE = "com.openless.app.overlay.HIDE"
        const val ACTION_TOGGLE_EXPAND = "com.openless.app.overlay.TOGGLE_EXPAND"
        private const val NOTIFICATION_ID = 42001

        @Volatile
        var instance: OpenLessOverlayService? = null
            private set
    }
}
