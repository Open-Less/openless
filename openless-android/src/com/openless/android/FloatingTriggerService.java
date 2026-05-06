package com.openless.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.ServiceInfo;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.PixelFormat;
import android.graphics.RectF;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.provider.Settings;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.WindowManager;
import android.widget.Toast;

// UI-only: refined bubble drawing (shadow hint, smoother mic icon)
// All actions preserved: toggle, translate, QA panel, QA clipboard, cancel, stop, refresh settings
// All Chinese text preserved
public final class FloatingTriggerService extends Service implements AndroidDictationCoordinator.Listener {
    private static final int NOTIFICATION_ID = 1001;
    private static final String CHANNEL_ID = "openless_floating_trigger";
    private static final String ACTION_STOP = "com.openless.android.STOP_FLOATING";
    private static final String ACTION_CANCEL = "com.openless.android.CANCEL_DICTATION";
    private static final String ACTION_TOGGLE = "com.openless.android.TOGGLE_DICTATION";
    private static final String ACTION_TRANSLATE = "com.openless.android.START_TRANSLATION";
    private static final String ACTION_OPEN_QA = "com.openless.android.OPEN_QA";
    private static final String ACTION_QA_CLIPBOARD = "com.openless.android.OPEN_QA_CLIPBOARD";
    static final String ACTION_REFRESH_SETTINGS = "com.openless.android.REFRESH_FLOATING_SETTINGS";
    private static final long LONG_PRESS_CANCEL_MS = 800;
    private static final long IDLE_DELAY_MS = 1500;

    private WindowManager windowManager;
    private WindowManager.LayoutParams params;
    private SharedPreferences prefs;
    private SettingsStore settingsStore;
    private MicBubbleView bubble;
    private AndroidDictationCoordinator coordinator;
    private final Handler main = new Handler(Looper.getMainLooper());
    private int stateGeneration;
    private boolean dragging;
    private float downX;
    private float downY;
    private int startX;
    private int startY;
    private CapsuleState currentState = CapsuleState.IDLE;
    private String currentMessage;

    @Override
    public void onCreate() {
        super.onCreate();
        settingsStore = new SettingsStore(this);
        HistoryStore historyStore = new HistoryStore(this);
        prefs = getSharedPreferences("openless_capsule", MODE_PRIVATE);
        coordinator = new AndroidDictationCoordinator(this, settingsStore, historyStore, this);
        windowManager = (WindowManager) getSystemService(WINDOW_SERVICE);
        if (!Settings.canDrawOverlays(this)) {
            toast("请先授予悬浮窗权限。");
            stopSelf();
            return;
        }
        startAsForegroundService();
        applyBubbleVisibility();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        String action = intent == null ? null : intent.getAction();
        if (ACTION_STOP.equals(action)) { stopSelf(); return START_NOT_STICKY; }
        if (ACTION_CANCEL.equals(action)) { coordinator.cancel(); return START_STICKY; }
        if (ACTION_TOGGLE.equals(action)) { coordinator.toggle(); return START_STICKY; }
        if (ACTION_TRANSLATE.equals(action)) { coordinator.startTranslation(); return START_STICKY; }
        if (ACTION_OPEN_QA.equals(action)) {
            Intent qaIntent = new Intent(this, QaPanelActivity.class);
            qaIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            startActivity(qaIntent);
            return START_STICKY;
        }
        if (ACTION_QA_CLIPBOARD.equals(action)) { openQaWithClipboardContext(); return START_STICKY; }
        if (ACTION_REFRESH_SETTINGS.equals(action)) { applyBubbleVisibility(); return START_STICKY; }
        if (bubble == null && Settings.canDrawOverlays(this)) { applyBubbleVisibility(); }
        return START_STICKY;
    }

    @Override
    public void onDestroy() {
        if (coordinator != null) coordinator.shutdown();
        if (bubble != null) { windowManager.removeView(bubble); bubble = null; }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) { return null; }

    private void showBubble() {
        int type = android.os.Build.VERSION.SDK_INT >= 26
                ? WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
                : WindowManager.LayoutParams.TYPE_PHONE;
        params = new WindowManager.LayoutParams(dp(64), dp(64), type,
                WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                        | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
                PixelFormat.TRANSLUCENT);
        params.gravity = Gravity.TOP | Gravity.START;
        params.x = prefs.getInt("x", dp(18));
        params.y = prefs.getInt("y", dp(180));
        bubble = new MicBubbleView(this);
        bubble.setOnTouchListener(this::onBubbleTouch);
        windowManager.addView(bubble, params);
    }

    private void hideBubble() {
        if (bubble != null) { windowManager.removeView(bubble); bubble = null; }
    }

    private void applyBubbleVisibility() {
        boolean shouldShow = settingsStore.get().showCapsule;
        if (!Settings.canDrawOverlays(this)) { hideBubble(); return; }
        if (shouldShow) { if (bubble == null) showBubble(); }
        else { hideBubble(); }
    }

    private void startAsForegroundService() {
        NotificationManager manager = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        if (android.os.Build.VERSION.SDK_INT >= 26 && manager != null) {
            NotificationChannel channel = new NotificationChannel(
                    CHANNEL_ID,
                    getString(R.string.floating_channel_name),
                    NotificationManager.IMPORTANCE_LOW);
            channel.setDescription(getString(R.string.floating_channel_description));
            manager.createNotificationChannel(channel);
        }
        Notification notification = buildNotification(currentState, currentMessage);
        if (android.os.Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE);
        } else { startForeground(NOTIFICATION_ID, notification); }
    }

    private Notification buildNotification(CapsuleState state, String message) {
        Intent openIntent = new Intent(this, MainActivity.class);
        PendingIntent pendingIntent = PendingIntent.getActivity(this, 0, openIntent,
                android.os.Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0);
        PendingIntent cancelIntent = PendingIntent.getService(this, 1,
                new Intent(this, FloatingTriggerService.class).setAction(ACTION_CANCEL),
                android.os.Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0);
        PendingIntent stopIntent = PendingIntent.getService(this, 2,
                new Intent(this, FloatingTriggerService.class).setAction(ACTION_STOP),
                android.os.Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0);
        PendingIntent translateIntent = PendingIntent.getService(this, 3,
                new Intent(this, FloatingTriggerService.class).setAction(ACTION_TRANSLATE),
                android.os.Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0);
        PendingIntent toggleIntent = PendingIntent.getService(this, 6,
                new Intent(this, FloatingTriggerService.class).setAction(ACTION_TOGGLE),
                android.os.Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0);
        PendingIntent qaIntent = PendingIntent.getService(this, 4,
                new Intent(this, FloatingTriggerService.class).setAction(ACTION_OPEN_QA),
                android.os.Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0);
        PendingIntent qaClipboardIntent = PendingIntent.getService(this, 5,
                new Intent(this, FloatingTriggerService.class).setAction(ACTION_QA_CLIPBOARD),
                android.os.Build.VERSION.SDK_INT >= 23 ? PendingIntent.FLAG_IMMUTABLE : 0);
        Notification.Builder builder = android.os.Build.VERSION.SDK_INT >= 26
                ? new Notification.Builder(this, CHANNEL_ID)
                : new Notification.Builder(this);
        builder.setSmallIcon(android.R.drawable.ic_btn_speak_now)
                .setContentTitle(getString(R.string.floating_notification_title))
                .setContentText(notificationContent(state, message))
                .setContentIntent(pendingIntent)
                .setOngoing(true);
        if (state == CapsuleState.STARTING
                || state == CapsuleState.RECORDING
                || state == CapsuleState.TRANSCRIBING
                || state == CapsuleState.POLISHING
                || state == CapsuleState.TRANSLATING) {
            builder.addAction(android.R.drawable.ic_menu_close_clear_cancel, getString(R.string.floating_action_cancel), cancelIntent)
                    .addAction(android.R.drawable.ic_menu_compass, getString(R.string.floating_action_qa), qaIntent)
                    .addAction(android.R.drawable.ic_menu_close_clear_cancel, getString(R.string.floating_action_stop), stopIntent);
        } else {
            builder.addAction(android.R.drawable.ic_btn_speak_now, getString(R.string.floating_action_toggle), toggleIntent)
                    .addAction(android.R.drawable.ic_menu_edit, getString(R.string.floating_action_translate), translateIntent)
                    .addAction(android.R.drawable.ic_menu_search, getString(R.string.floating_action_qa_clipboard), qaClipboardIntent);
        }
        return builder.build();
    }

    private String notificationContent(CapsuleState state, String message) {
        if (state == null || state == CapsuleState.IDLE) return getString(R.string.floating_notification_text);
        if (message != null && !message.trim().isEmpty()
                && (state == CapsuleState.DONE || state == CapsuleState.ERROR)) {
            return message;
        }
        return state.label;
    }

    private void updateForegroundNotification() {
        NotificationManager manager = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        if (manager == null) return;
        manager.notify(NOTIFICATION_ID, buildNotification(currentState, currentMessage));
    }

    private boolean onBubbleTouch(View view, MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                downX = event.getRawX(); downY = event.getRawY();
                startX = params.x; startY = params.y; dragging = false;
                return true;
            case MotionEvent.ACTION_MOVE:
                float dx = event.getRawX() - downX;
                float dy = event.getRawY() - downY;
                if (Math.abs(dx) > dp(8) || Math.abs(dy) > dp(8)) {
                    dragging = true;
                    params.x = startX + (int) dx; params.y = startY + (int) dy;
                    windowManager.updateViewLayout(bubble, params);
                    prefs.edit().putInt("x", params.x).putInt("y", params.y).apply();
                }
                return true;
            case MotionEvent.ACTION_UP:
                if (!dragging) {
                    if (System.currentTimeMillis() - bubble.downAt >= LONG_PRESS_CANCEL_MS) {
                        coordinator.cancel();
                    } else { coordinator.toggle(); }
                }
                return true;
            default: return true;
        }
    }

    private void setState(CapsuleState state, String message) {
        int generation = ++stateGeneration;
        currentState = state;
        currentMessage = message;
        if (bubble != null) bubble.setState(state, message);
        updateForegroundNotification();
        if (state == CapsuleState.DONE || state == CapsuleState.ERROR || state == CapsuleState.CANCELLED) {
            main.postDelayed(() -> { if (generation == stateGeneration) setState(CapsuleState.IDLE, null); }, IDLE_DELAY_MS);
        }
    }

    @Override public void onCapsuleState(CapsuleState state, String message) { setState(state, message); }
    @Override public void onRecordingLevel(float level) { if (bubble != null) bubble.setLevel(level); }
    @Override public void onToast(String message) { toast(message); }

    private void toast(String message) {
        Toast.makeText(this, message == null ? "OpenLess 操作失败。" : message, Toast.LENGTH_SHORT).show();
    }

    private void openQaWithClipboardContext() {
        ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard == null || !clipboard.hasPrimaryClip()) { toast("剪贴板为空。"); return; }
        ClipData clip = clipboard.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) { toast("剪贴板为空。"); return; }
        CharSequence text = clip.getItemAt(0).coerceToText(this);
        String context = text == null ? "" : text.toString().trim();
        Intent qaIntent = new Intent(this, QaPanelActivity.class);
        qaIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        if (!context.isEmpty()) qaIntent.putExtra(QaPanelActivity.EXTRA_CONTEXT, context);
        startActivity(qaIntent);
    }

    private int dp(int value) { return (int) (value * getResources().getDisplayMetrics().density + 0.5f); }

    // ─── Mic bubble view (visual refined) ────────────────────────────

    private static class MicBubbleView extends View {
        private static final int BG_READY = Color.rgb(37, 99, 235);
        private static final int BG_RECORDING = Color.rgb(220, 38, 38);
        private static final int BG_PROCESSING = Color.rgb(217, 119, 6);
        private static final int BG_DONE = Color.rgb(22, 163, 74);

        private final Paint circlePaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint micPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint statusPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint dotPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final RectF micRect = new RectF();
        private float level;
        private int bgColor = BG_READY;
        private String statusText;
        private boolean showDot = true;
        private boolean dotOn = true;
        long downAt;

        MicBubbleView(android.content.Context context) {
            super(context);
            circlePaint.setStyle(Paint.Style.FILL);
            micPaint.setStyle(Paint.Style.STROKE);
            micPaint.setStrokeCap(Paint.Cap.ROUND);
            micPaint.setStrokeJoin(Paint.Join.ROUND);
            statusPaint.setTextAlign(Paint.Align.CENTER);
            statusPaint.setAntiAlias(true);
            dotPaint.setStyle(Paint.Style.FILL);
        }

        void setState(CapsuleState state, String message) {
            switch (state.label) {
                case "就绪": bgColor = BG_READY; statusText = null; showDot = true; break;
                case "启动中": case "听写中": bgColor = BG_RECORDING; statusText = null; showDot = true; break;
                case "转写中": case "润色中": case "翻译中": bgColor = BG_PROCESSING; statusText = ".."; showDot = false; break;
                case "完成": case "已复制": bgColor = BG_DONE; statusText = message; showDot = false; break;
                case "错误": bgColor = BG_RECORDING; statusText = "!"; showDot = false; break;
                case "已取消": bgColor = Color.rgb(160, 160, 163); statusText = null; showDot = true; break;
                default: bgColor = BG_READY; statusText = null; showDot = true; break;
            }
            dotOn = true;
            invalidate();
        }

        void setLevel(float nextLevel) { level = Math.max(0f, Math.min(1f, nextLevel)); invalidate(); }

        @Override
        protected void onDraw(Canvas canvas) {
            float cx = getWidth() / 2f;
            float cy = getHeight() / 2f;
            float r = Math.min(cx, cy) - dp(2);

            // Shadow hint: subtle darker ring
            Paint shadowPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
            shadowPaint.setStyle(Paint.Style.FILL);
            shadowPaint.setColor(Color.argb(18, 0, 0, 0));
            canvas.drawCircle(cx + dp(1), cy + dp(1), r, shadowPaint);

            circlePaint.setColor(bgColor);
            canvas.drawCircle(cx, cy, r, circlePaint);

            if (showDot && dotOn) {
                float dotR = dp(4);
                dotPaint.setColor(Color.WHITE);
                canvas.drawCircle(cx, cy - dp(6), dotR, dotPaint);
            } else if (statusText != null) {
                statusPaint.setColor(Color.WHITE);
                statusPaint.setTextSize(dp(9));
                canvas.drawText(statusText, cx, cy + dp(3), statusPaint);
            } else {
                drawMicIcon(canvas, cx, cy, r);
            }
        }

        private void drawMicIcon(Canvas canvas, float cx, float cy, float r) {
            float micSize = r * 0.42f;
            float strokeW = 2.2f * getResources().getDisplayMetrics().density;
            micPaint.setColor(Color.WHITE);
            micPaint.setStrokeWidth(strokeW);

            float left = cx - micSize * 0.35f;
            float top = cy - micSize * 0.7f;
            float right = cx + micSize * 0.35f;
            float bottom = cy + micSize * 0.25f;
            micRect.set(left, top, right, bottom);
            canvas.drawRoundRect(micRect, dp(3), dp(3), micPaint);
            canvas.drawLine(cx, top + micSize * 0.15f, cx, cy + micSize * 0.15f, micPaint);

            float armY = cy + micSize * 0.25f;
            canvas.drawLine(cx - micSize * 0.3f, armY, cx + micSize * 0.3f, armY, micPaint);

            float arcY = cy + micSize * 0.3f;
            canvas.drawArc(new RectF(cx - micSize * 0.25f, arcY - micSize * 0.1f,
                    cx + micSize * 0.25f, arcY + micSize * 0.3f), 0, 180, false, micPaint);

            if (level > 0.05f) {
                float waveHeight = level * micSize * 0.45f;
                float waveAlpha = 0.3f + level * 0.4f;
                micPaint.setAlpha((int) (255 * waveAlpha));
                canvas.drawArc(new RectF(cx - micSize * 0.55f - waveHeight * 0.3f,
                        cy - waveHeight * 0.4f, cx - micSize * 0.15f, cy + waveHeight * 0.4f),
                        -90, 180, false, micPaint);
                canvas.drawArc(new RectF(cx + micSize * 0.15f, cy - waveHeight * 0.4f,
                        cx + micSize * 0.55f + waveHeight * 0.3f, cy + waveHeight * 0.4f),
                        90, 180, false, micPaint);
                micPaint.setAlpha(255);
            }
        }

        @Override
        public boolean onTouchEvent(MotionEvent event) {
            if (event.getAction() == MotionEvent.ACTION_DOWN) downAt = System.currentTimeMillis();
            return super.onTouchEvent(event);
        }

        private int dp(int value) { return (int) (value * getResources().getDisplayMetrics().density + 0.5f); }
    }
}
