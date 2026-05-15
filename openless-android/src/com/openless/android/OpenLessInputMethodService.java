package com.openless.android;

import android.inputmethodservice.InputMethodService;
import android.view.View;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.EditorInfo;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.lang.ref.WeakReference;

public final class OpenLessInputMethodService extends InputMethodService {
    private static WeakReference<OpenLessInputMethodService> active = new WeakReference<>(null);
    private static volatile String currentEditorPackage;

    static boolean commitToCurrentInput(String text) {
        OpenLessInputMethodService service = active.get();
        if (service == null) {
            return false;
        }
        InputConnection connection = service.getCurrentInputConnection();
        return connection != null && connection.commitText(text, 1);
    }

    static boolean isActive() {
        OpenLessInputMethodService service = active.get();
        return service != null && service.getCurrentInputConnection() != null;
    }

    static String currentTargetPackage() {
        String value = currentEditorPackage;
        return value == null || value.trim().isEmpty() ? null : value.trim();
    }

    @Override
    public void onCreate() {
        super.onCreate();
        active = new WeakReference<>(this);
    }

    @Override
    public void onDestroy() {
        if (active.get() == this) {
            active = new WeakReference<>(null);
        }
        super.onDestroy();
    }

    @Override
    public void onStartInput(EditorInfo attribute, boolean restarting) {
        super.onStartInput(attribute, restarting);
        currentEditorPackage = attribute == null ? null : attribute.packageName;
    }

    @Override
    public void onFinishInput() {
        currentEditorPackage = null;
        super.onFinishInput();
    }

    @Override
    public View onCreateInputView() {
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setPadding(24, 18, 24, 18);
        TextView label = new TextView(this);
        label.setText("OpenLess 键盘已激活，请使用悬浮触发器开始听写。");
        label.setTextSize(14);
        layout.addView(label);
        return layout;
    }
}
