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
import android.provider.Settings
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
    private var armed = false
    private var dragStartX = 0
    private var dragStartY = 0
    private var paramStartX = 0
    private var paramStartY = 0
    private var dragging = false
    private var longPressRecording = false
    private var pendingSwipe: SwipeDirection? = null
    private var swipeConsumed = false

    private lateinit var iconContainer: FrameLayout
    private lateinit var iconButton: ImageView

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        OpenLessOverlayBridge.listener = this
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i(
            TAG,
            "onStartCommand action=${intent?.action} startId=$startId rootAttached=${rootView?.isAttachedToWindow}",
        )
        when (intent?.action) {
            ACTION_SHOW -> showOverlay()
            ACTION_START_RECORDING -> {
                showOverlay()
                if (!tryPromoteRecordingForeground()) {
                    abortRecordingStart(startId)
                    return START_NOT_STICKY
                }
                beginDictationFromOverlay()
            }
            ACTION_HIDE -> {
                hideOverlay()
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                    stopForeground(STOP_FOREGROUND_REMOVE)
                } else {
                    @Suppress("DEPRECATION")
                    stopForeground(true)
                }
                stopSelf(startId)
            }
            ACTION_REPLACE_OVERLAY -> replaceOverlay()
            ACTION_TOGGLE_EXPAND -> handleIconClick()
            ACTION_KEYBOARD_CHANGED -> handleKeyboardChanged(intent)
            ACTION_REFRESH_LAYOUT -> refreshOverlayLayout()
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
        // 系统杀死前台服务时也会走到这里：同步原生 OVERLAY_VISIBLE=false，避免状态永久残留为 true。
        runCatching { OpenLessNative.nativeNotifyOverlayDestroyed() }
        super.onDestroy()
    }

    override fun onCapsuleStateChanged(state: String, message: String?) {
        when (state) {
            "recording" -> {
                recording = true
                processing = false
                if (!tryPromoteRecordingForeground()) {
                    try {
                        OpenLessNative.nativeCancelDictation()
                    } catch (error: Throwable) {
                        Log.w(TAG, "cancel dictation bridge unavailable", error)
                    }
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
                setArmed(false)
            }
            "error" -> {
                recording = false
                processing = false
                setArmed(false)
                applyVisualState(OverlayVisualState.Error)
                message?.takeIf { it.isNotBlank() }?.let { showToast(it) }
            }
            "cancelled", "idle" -> {
                recording = false
                processing = false
                setArmed(false)
            }
        }
    }

    private fun showOverlay() = withOverlayLock {
        windowManager = getSystemService(WINDOW_SERVICE) as WindowManager
        reconcileOverlayRoots()
        overlayRoots.lastOrNull()?.let { existing ->
            val params = existing.layoutParams as? WindowManager.LayoutParams
            if (params != null) {
                refreshExistingOverlayLayout(existing, params)
                Log.i(TAG, "overlay already shown roots=${overlayRoots.size}")
                return@withOverlayLock
            }
            removeOverlayRoot(existing)
            synchronized(overlayRoots) {
                overlayRoots.remove(existing)
            }
            if (rootView === existing) {
                rootView = null
                layoutParams = null
            }
        }
        attachNewOverlayRoot()
    }

    private fun replaceOverlay() = withOverlayLock {
        windowManager = windowManager ?: getSystemService(WINDOW_SERVICE) as WindowManager
        reconcileOverlayRoots()
        val removed = clearAllOverlayRoots()
        if (removed > 0) {
            Log.i(TAG, "overlay replace cleared removed=$removed")
        }
        if (!canDrawOverlays()) {
            Log.i(TAG, "overlay replace skipped no overlay permission")
            return@withOverlayLock
        }
        if (!attachNewOverlayRoot()) {
            return@withOverlayLock
        }
        if (recording) {
            tryPromoteRecordingForeground()
        }
        Log.i(TAG, "overlay replaced roots=1")
    }

    private fun hideOverlay() = withOverlayLock {
        val removed = clearAllOverlayRoots()
        if (removed > 0) {
            Log.i(TAG, "overlay hidden removed=$removed")
        }
    }

    private fun clearAllOverlayRoots(): Int {
        windowManager = windowManager ?: getSystemService(WINDOW_SERVICE) as WindowManager
        val views = synchronized(overlayRoots) {
            (overlayRoots + listOfNotNull(rootView)).distinct().also {
                overlayRoots.clear()
            }
        }
        views.forEach { view ->
            removeOverlayRoot(view)
        }
        rootView = null
        layoutParams = null
        return views.size
    }

    private fun attachNewOverlayRoot(): Boolean {
        val savedPosition = loadSavedPosition()
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
            x = savedPosition.first
            y = savedPosition.second
        }
        layoutParams = params

        val root = FrameLayout(this).apply {
            contentDescription = "OpenLess"
            isClickable = true
            isFocusable = false
            setOnClickListener { handleIconClick() }
        }
        iconContainer = root
        iconButton = buildIconButton()
        root.addView(
            iconButton,
            FrameLayout.LayoutParams(1, 1, Gravity.CENTER),
        )
        applyOverlaySize(root)
        attachDragHandler(root, params)
        return try {
            windowManager?.addView(root, params)
            rootView = root
            synchronized(overlayRoots) {
                overlayRoots.add(root)
            }
            Log.i(TAG, "overlay shown x=${params.x} y=${params.y} roots=${overlayRoots.size}")
            applyVisualState(
                when {
                    recording -> OverlayVisualState.Recording
                    processing -> OverlayVisualState.Processing
                    armed -> OverlayVisualState.Armed
                    else -> OverlayVisualState.Idle
                },
            )
            true
        } catch (error: Throwable) {
            Log.w(TAG, "show overlay failed", error)
            layoutParams = null
            false
        }
    }

    private fun canDrawOverlays(): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            Settings.canDrawOverlays(this)
        } else {
            true
        }
    }

    private fun refreshExistingOverlayLayout(
        root: FrameLayout,
        params: WindowManager.LayoutParams,
    ) {
        rootView = root
        layoutParams = params
        iconContainer = root
        (root.getChildAt(0) as? ImageView)?.let { iconButton = it }
        applyOverlaySize(root)
        attachDragHandler(root, params)
        clampToScreen(params)
        windowManager?.updateViewLayout(root, params)
    }

    private fun refreshOverlayLayout() = withOverlayLock {
        windowManager = windowManager ?: getSystemService(WINDOW_SERVICE) as WindowManager
        reconcileOverlayRoots()
        val root = overlayRoots.lastOrNull() ?: rootView
        if (root == null || !root.isAttachedToWindow) {
            Log.i(TAG, "overlay refresh skipped roots=0")
            return@withOverlayLock
        }
        val params = root.layoutParams as? WindowManager.LayoutParams
        if (params == null) {
            Log.i(TAG, "overlay refresh skipped roots=0")
            return@withOverlayLock
        }
        refreshExistingOverlayLayout(root, params)
        val sizeDp = OpenLessAndroidPreferences.overlaySizeDp(this)
        Log.i(TAG, "overlay layout refreshed sizeDp=$sizeDp roots=1")
    }

    private fun reconcileOverlayRoots() {
        val roots = synchronized(overlayRoots) {
            (overlayRoots + listOfNotNull(rootView)).distinct().filter { it.isAttachedToWindow }.also {
                overlayRoots.clear()
                overlayRoots.addAll(it)
            }
        }
        if (roots.isEmpty()) {
            rootView = null
            layoutParams = null
            return
        }
        roots.dropLast(1).forEach { staleRoot ->
            removeOverlayRoot(staleRoot)
            synchronized(overlayRoots) {
                overlayRoots.remove(staleRoot)
            }
        }
        val activeRoot = roots.last()
        synchronized(overlayRoots) {
            overlayRoots.clear()
            overlayRoots.add(activeRoot)
        }
        rootView = activeRoot
        layoutParams = activeRoot.layoutParams as? WindowManager.LayoutParams
        Log.i(TAG, "reconciled overlay roots kept=1 removed=${roots.size - 1}")
    }

    private fun removeOverlayRoot(view: FrameLayout) {
        try {
            if (view.isAttachedToWindow) {
                windowManager?.removeViewImmediate(view)
            }
        } catch (error: Throwable) {
            Log.w(TAG, "remove overlay root failed", error)
        }
    }

    private fun buildIconButton(): ImageView {
        return ImageView(this).apply {
            setImageResource(R.drawable.ic_overlay_logo)
            scaleType = ImageView.ScaleType.CENTER_INSIDE
            setPadding(0, 0, 0, 0)
            contentDescription = "OpenLess"
            isClickable = false
            isFocusable = false
        }
    }

    private fun applyOverlaySize(container: FrameLayout) {
        if (!::iconButton.isInitialized) return
        val sizeDp = OpenLessAndroidPreferences.overlaySizeDp(this)
        val paddingDp = overlayPaddingDp(sizeDp)
        val imageSizePx = dp((sizeDp - paddingDp * 2).coerceAtLeast(MIN_ICON_IMAGE_SIZE_DP))
        val paddingPx = dp(paddingDp)
        container.setPadding(paddingPx, paddingPx, paddingPx, paddingPx)
        (iconButton.layoutParams as? FrameLayout.LayoutParams)?.let { childParams ->
            childParams.width = imageSizePx
            childParams.height = imageSizePx
            childParams.gravity = Gravity.CENTER
            iconButton.layoutParams = childParams
        }
        container.requestLayout()
        Log.i(TAG, "overlay size applied sizeDp=$sizeDp imagePx=$imageSizePx")
    }

    private fun overlayPaddingDp(sizeDp: Int): Int {
        return (sizeDp * ICON_PADDING_DP / DEFAULT_ICON_SIZE_DP).coerceIn(8, 20)
    }

    private fun handleIconClick() {
        if (processing) return
        if (recording) {
            stopRecordingFromOverlay()
            return
        }
        if (!isTapActivationMode()) {
            return
        }
        startRecordingFromOverlay()
    }

    private fun handleKeyboardChanged(intent: Intent) {
        val visible = intent.getBooleanExtra(EXTRA_KEYBOARD_VISIBLE, false)
        keyboardVisible = visible
        Log.i(TAG, "keyboard changed visible=$visible")
        if (visible) {
            showOverlay()
            return
        }
        if (!recording && !processing) {
            hideOverlay()
        }
    }

    private fun attachDragHandler(view: View, params: WindowManager.LayoutParams) {
        view.setOnTouchListener { touchedView, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    dragging = false
                    swipeConsumed = false
                    longPressRecording = false
                    pendingSwipe = null
                    if (processing) {
                        return@setOnTouchListener true
                    }
                    dragStartX = event.rawX.toInt()
                    dragStartY = event.rawY.toInt()
                    paramStartX = params.x
                    paramStartY = params.y
                    if (!isTapActivationMode() && !recording && !processing) {
                        longPressRecording = true
                        startRecordingFromOverlay()
                    }
                    true
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = event.rawX.toInt() - dragStartX
                    val dy = event.rawY.toInt() - dragStartY
                    val verticalSwipe = detectVerticalSwipe(dx, dy)
                    if (recording && verticalSwipe != null && matchesConfiguredCancelSwipe(verticalSwipe) && !swipeConsumed) {
                        pendingSwipe = verticalSwipe
                        swipeConsumed = true
                        applySwipePreview(verticalSwipe)
                        return@setOnTouchListener true
                    }
                    val swipe = detectHorizontalSwipe(dx, dy)
                    if ((recording || armed || longPressRecording) && swipe != null && !swipeConsumed) {
                        pendingSwipe = swipe
                        swipeConsumed = true
                        applySwipePreview(swipe)
                        return@setOnTouchListener true
                    }
                    if (!processing && !armed && !recording && !longPressRecording && (abs(dx) > DRAG_SLOP_PX || abs(dy) > DRAG_SLOP_PX)) {
                        dragging = true
                        params.x = paramStartX + dx
                        params.y = paramStartY + dy
                        clampToScreen(params)
                        rootView?.let { windowManager?.updateViewLayout(it, params) }
                    }
                    true
                }
                MotionEvent.ACTION_UP -> {
                    if (!dragging) {
                        val swipe = pendingSwipe
                        if (swipe != null) {
                            commitSwipe(swipe)
                        } else if (longPressRecording || (!isTapActivationMode() && recording)) {
                            stopRecordingFromOverlay()
                        } else if (!isTapActivationMode()) {
                            setArmed(false)
                        } else if (!swipeConsumed) {
                            touchedView.performClick()
                        }
                    } else {
                        savePosition(params.x, params.y)
                    }
                    longPressRecording = false
                    pendingSwipe = null
                    swipeConsumed = false
                    true
                }
                MotionEvent.ACTION_CANCEL -> {
                    if (longPressRecording || (!isTapActivationMode() && recording)) {
                        stopRecordingFromOverlay()
                    }
                    longPressRecording = false
                    pendingSwipe = null
                    swipeConsumed = false
                    true
                }
                else -> false
            }
        }
    }

    private fun applyVisualState(state: OverlayVisualState) {
        if (!::iconContainer.isInitialized || !::iconButton.isInitialized) return
        val (alpha, fill, stroke, strokeWidth, enabled) = when (state) {
            OverlayVisualState.Idle -> VisualStyle(
                alpha = 0.58f,
                fill = Color.parseColor("#66202A36"),
                stroke = Color.parseColor("#66FFFFFF"),
                strokeWidth = 1,
                enabled = true,
            )
            OverlayVisualState.Armed -> VisualStyle(
                alpha = 1f,
                fill = Color.parseColor("#E6111827"),
                stroke = Color.parseColor("#38BDF8"),
                strokeWidth = 3,
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
        iconContainer.alpha = alpha
        iconContainer.isEnabled = enabled
        iconContainer.background = circleDrawable(fill, stroke, dp(strokeWidth))
        iconButton.isEnabled = enabled
    }

    private fun setArmed(value: Boolean) {
        armed = value
        if (!recording && !processing) {
            applyVisualState(if (value) OverlayVisualState.Armed else OverlayVisualState.Idle)
        }
    }

    private fun detectHorizontalSwipe(dx: Int, dy: Int): SwipeDirection? {
        if (abs(dx) < dp(SWIPE_THRESHOLD_DP)) return null
        if (abs(dy) > abs(dx) * SWIPE_VERTICAL_RATIO) return null
        return if (dx < 0) SwipeDirection.Left else SwipeDirection.Right
    }

    private fun detectVerticalSwipe(dx: Int, dy: Int): SwipeDirection? {
        if (abs(dy) < dp(SWIPE_THRESHOLD_DP)) return null
        if (abs(dx) > abs(dy) * SWIPE_VERTICAL_RATIO) return null
        return if (dy < 0) SwipeDirection.Up else SwipeDirection.Down
    }

    private fun matchesConfiguredCancelSwipe(direction: SwipeDirection): Boolean {
        val configured = OpenLessAndroidPreferences.overlayCancelSwipeDirection(this)
        return (direction == SwipeDirection.Up && configured == "up") ||
            (direction == SwipeDirection.Down && configured == "down")
    }

    private fun applySwipePreview(direction: SwipeDirection) {
        when (direction) {
            SwipeDirection.Left -> applyVisualState(OverlayVisualState.Armed)
            SwipeDirection.Right -> applyVisualState(OverlayVisualState.Processing)
            SwipeDirection.Up,
            SwipeDirection.Down -> applyVisualState(OverlayVisualState.Error)
        }
    }

    private fun commitSwipe(direction: SwipeDirection) {
        Log.i(TAG, "commit swipe direction=$direction recording=$recording processing=$processing")
        when (direction) {
            SwipeDirection.Left -> handleLeftSwipe()
            SwipeDirection.Right -> finalizeQaFromOverlay()
            SwipeDirection.Up,
            SwipeDirection.Down -> cancelRecordingFromOverlay(direction)
        }
    }

    private fun cancelRecordingFromOverlay(direction: SwipeDirection) {
        if (!recording || !matchesConfiguredCancelSwipe(direction)) {
            return
        }
        try {
            OpenLessNative.nativeCancelDictation()
            recording = false
            processing = false
            longPressRecording = false
            setArmed(false)
            applyVisualState(OverlayVisualState.Idle)
            Log.i(TAG, "recording cancelled from overlay direction=$direction")
        } catch (error: Throwable) {
            Log.w(TAG, "cancel dictation bridge unavailable", error)
            applyVisualState(OverlayVisualState.Error)
            showToast("语音服务未就绪，请打开 OpenLess 后重试")
        }
    }

    private fun handleLeftSwipe() {
        when (OpenLessAndroidPreferences.overlayLeftSwipeAction(this)) {
            "style_pack" -> {
                switchStylePackFromOverlay()
                if (recording) {
                    stopRecordingFromOverlay()
                }
            }
            else -> stopRecordingFromOverlay(translation = true)
        }
    }

    private fun switchStylePackFromOverlay() {
        try {
            OpenLessNative.nativeSwitchStylePack()
            setArmed(false)
        } catch (error: Throwable) {
            Log.w(TAG, "switch style pack bridge unavailable", error)
            applyVisualState(OverlayVisualState.Error)
            showToast("语音服务未就绪，请打开 OpenLess 后重试")
        }
    }

    private fun openQaFromOverlay() {
        try {
            Log.i(TAG, "open QA from overlay")
            OpenLessNative.nativeOpenQaFromOverlay()
            setArmed(false)
        } catch (error: Throwable) {
            Log.w(TAG, "open QA bridge unavailable", error)
            applyVisualState(OverlayVisualState.Error)
            showToast("问答服务未就绪，请打开 OpenLess 后重试")
        }
    }

    private fun finalizeQaFromOverlay() {
        try {
            Log.i(TAG, "finalize QA from overlay")
            OpenLessNative.nativeFinalizeQaFromOverlay()
            recording = false
            processing = true
            setArmed(false)
            applyVisualState(OverlayVisualState.Processing)
        } catch (error: Throwable) {
            Log.w(TAG, "finalize QA bridge unavailable", error)
            processing = false
            applyVisualState(OverlayVisualState.Error)
            showToast("问答服务未就绪，请打开 OpenLess 后重试")
        }
    }

    private fun abortRecordingStart(startId: Int) {
        recording = false
        processing = false
        try {
            OpenLessNative.nativeCancelDictation()
        } catch (error: Throwable) {
            Log.w(TAG, "cancel dictation after foreground failure", error)
        }
        stopSelf(startId)
    }

    private fun beginDictationFromOverlay(translation: Boolean = false) {
        try {
            if (translation) {
                OpenLessNative.nativeStartDictationWithTranslation(true)
            } else {
                OpenLessNative.nativeStartDictation()
            }
            recording = true
            processing = false
            setArmed(false)
            applyVisualState(OverlayVisualState.Recording)
        } catch (error: Throwable) {
            Log.w(TAG, "start dictation bridge unavailable", error)
            recording = false
            processing = false
            applyVisualState(OverlayVisualState.Error)
            showToast("语音服务未就绪，请打开 OpenLess 后重试")
        }
    }

    private fun startRecordingFromOverlay(translation: Boolean = false) {
        showOverlay()
        if (tryPromoteRecordingForeground()) {
            beginDictationFromOverlay(translation)
            return
        }
        applyVisualState(OverlayVisualState.Error)
    }

    private fun stopRecordingFromOverlay(translation: Boolean = false) {
        try {
            recording = false
            processing = true
            applyVisualState(OverlayVisualState.Processing)
            if (translation) {
                OpenLessNative.nativeStopDictationWithTranslation(true)
            } else {
                OpenLessNative.nativeStopDictation()
            }
        } catch (error: Throwable) {
            Log.w(TAG, "stop dictation bridge unavailable", error)
            recording = false
            processing = false
            applyVisualState(OverlayVisualState.Error)
            showToast("语音服务未就绪，请打开 OpenLess 后重试")
        }
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
            .setSmallIcon(R.mipmap.ic_launcher)
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
        return measured.takeIf { it > 0 } ?: dp(OpenLessAndroidPreferences.overlaySizeDp(this))
    }

    private fun clampToScreen(params: WindowManager.LayoutParams) {
        val iconSize = overlaySize()
        val margin = dp(8)
        val maxX = (resources.displayMetrics.widthPixels - iconSize - margin).coerceAtLeast(margin)
        val maxY = (resources.displayMetrics.heightPixels - iconSize - margin).coerceAtLeast(margin)
        params.x = params.x.coerceIn(margin, maxX)
        params.y = params.y.coerceIn(margin, maxY)
    }

    private fun loadSavedPosition(): Pair<Int, Int> {
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        val defaultX = dp(24)
        val defaultY = dp(120)
        val x = prefs.getInt(PREF_KEY_X, defaultX)
        val y = prefs.getInt(PREF_KEY_Y, defaultY)
        return x to y
    }

    private fun savePosition(x: Int, y: Int) {
        getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
            .edit()
            .putInt(PREF_KEY_X, x)
            .putInt(PREF_KEY_Y, y)
            .apply()
    }

    private fun isTapActivationMode(): Boolean {
        return OpenLessAndroidPreferences.overlayActivationMode(this) == "tap"
    }

    private fun showToast(message: String) {
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
    }

    private fun dp(value: Int): Int {
        return (value * resources.displayMetrics.density).toInt()
    }

    private inline fun <T> withOverlayLock(block: () -> T): T {
        return synchronized(overlayLock) {
            block()
        }
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
        Armed,
        Recording,
        Processing,
        Error,
    }

    private enum class SwipeDirection {
        Left,
        Right,
        Up,
        Down,
    }

    companion object {
        const val ACTION_SHOW = "com.openless.app.overlay.SHOW"
        const val ACTION_HIDE = "com.openless.app.overlay.HIDE"
        const val ACTION_REPLACE_OVERLAY = "com.openless.app.overlay.REPLACE_OVERLAY"
        const val ACTION_REFRESH_LAYOUT = "com.openless.app.overlay.REFRESH_LAYOUT"
        const val ACTION_TOGGLE_EXPAND = "com.openless.app.overlay.TOGGLE_EXPAND"
        const val ACTION_START_RECORDING = "com.openless.app.overlay.START_RECORDING"
        const val ACTION_KEYBOARD_CHANGED = "com.openless.app.overlay.KEYBOARD_CHANGED"
        const val EXTRA_KEYBOARD_VISIBLE = "keyboard_visible"
        const val EXTRA_KEYBOARD_TOP = "keyboard_top"
        const val EXTRA_KEYBOARD_BOTTOM = "keyboard_bottom"
        private const val DEFAULT_ICON_SIZE_DP = 72
        private const val MIN_ICON_IMAGE_SIZE_DP = 32
        private const val ICON_PADDING_DP = 12
        private const val DRAG_SLOP_PX = 8
        private const val SWIPE_THRESHOLD_DP = 56
        private const val SWIPE_VERTICAL_RATIO = 0.6f
        private const val PREFS_NAME = "openless_overlay"
        private const val PREF_KEY_X = "overlay_x"
        private const val PREF_KEY_Y = "overlay_y"
        private const val NOTIFICATION_ID = 42001
        private const val TAG = "OpenLessOverlayService"

        private val overlayLock = Any()
        private val overlayRoots = mutableListOf<FrameLayout>()

        @Volatile
        var instance: OpenLessOverlayService? = null
            private set
    }
}
