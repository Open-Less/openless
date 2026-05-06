package com.openless.android;

import android.Manifest;
import android.app.NotificationManager;
import android.content.Context;
import android.content.pm.PackageManager;
import android.provider.Settings;
import android.view.inputmethod.InputMethodInfo;
import android.view.inputmethod.InputMethodManager;

import java.util.ArrayList;
import java.util.List;

final class AndroidPermissionDiagnostics {
    private AndroidPermissionDiagnostics() {
    }

    static List<PermissionStatus> collect(Context context) {
        ArrayList<PermissionStatus> out = new ArrayList<>();
        out.add(microphoneStatus(context));
        out.add(overlayStatus(context));
        out.add(notificationStatus(context));
        out.add(foregroundServiceStatus(context));
        out.add(imeEnabledStatus(context));
        out.add(imeActiveStatus());
        return out;
    }

    private static PermissionStatus microphoneStatus(Context context) {
        boolean granted = context.checkSelfPermission(Manifest.permission.RECORD_AUDIO)
                == PackageManager.PERMISSION_GRANTED;
        return new PermissionStatus(
                "麦克风",
                granted ? "已授权。" : "所有听写都需要麦克风权限。",
                granted,
                PermissionStatus.ACTION_APP_PERMISSIONS,
                granted ? "查看" : "去设置");
    }

    private static PermissionStatus overlayStatus(Context context) {
        boolean granted = Settings.canDrawOverlays(context);
        return new PermissionStatus(
                "悬浮窗",
                granted ? "可以显示在其他应用上方。" : "悬浮触发气泡需要此权限。",
                granted,
                PermissionStatus.ACTION_OVERLAY,
                granted ? "管理" : "授权");
    }

    private static PermissionStatus notificationStatus(Context context) {
        boolean enabled = notificationsEnabled(context);
        String detail;
        if (android.os.Build.VERSION.SDK_INT < 33) {
            detail = enabled ? "前台服务通知已启用。" : "前台服务通知被阻止。";
        } else {
            detail = enabled ? "Android 13+ 通知权限已授权。" : "Android 13+ 需要通知权限才能显示前台服务。";
        }
        return new PermissionStatus(
                "通知",
                detail,
                enabled,
                PermissionStatus.ACTION_NOTIFICATIONS,
                enabled ? "管理" : "启用");
    }

    private static PermissionStatus foregroundServiceStatus(Context context) {
        boolean microphone = context.checkSelfPermission(Manifest.permission.RECORD_AUDIO)
                == PackageManager.PERMISSION_GRANTED;
        boolean notifications = notificationsEnabled(context);
        boolean overlay = Settings.canDrawOverlays(context);
        boolean ready = microphone && notifications && overlay;
        String detail = ready
                ? "前台麦克风服务所需条件已就绪。"
                : "需要麦克风、通知和悬浮窗权限，才能接近桌面版的使用方式。";
        return new PermissionStatus(
                "前台服务",
                detail,
                ready,
                PermissionStatus.ACTION_NONE,
                "就绪");
    }

    private static PermissionStatus imeEnabledStatus(Context context) {
        boolean enabled = isImeEnabled(context);
        return new PermissionStatus(
                "OpenLess 键盘已启用",
                enabled ? "已出现在系统输入法列表中。" : "请先在系统输入法设置中启用 OpenLess 键盘。",
                enabled,
                PermissionStatus.ACTION_IME,
                enabled ? "管理" : "启用");
    }

    private static PermissionStatus imeActiveStatus() {
        boolean active = OpenLessInputMethodService.isActive();
        return new PermissionStatus(
                "OpenLess 键盘当前激活",
                active ? "当前输入框可直接插入文字。" : "请在输入框中切换到 OpenLess 键盘以启用直接插入。",
                active,
                active ? PermissionStatus.ACTION_NONE : PermissionStatus.ACTION_IME,
                active ? "已激活" : "打开设置");
    }

    private static boolean notificationsEnabled(Context context) {
        if (android.os.Build.VERSION.SDK_INT >= 33
                && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED) {
            return false;
        }
        NotificationManager manager =
                (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
        if (manager == null) {
            return true;
        }
        if (android.os.Build.VERSION.SDK_INT >= 24) {
            return manager.areNotificationsEnabled();
        }
        return true;
    }

    private static boolean isImeEnabled(Context context) {
        InputMethodManager imm =
                (InputMethodManager) context.getSystemService(Context.INPUT_METHOD_SERVICE);
        if (imm == null) {
            return false;
        }
        String expectedId = context.getPackageName() + "/" + OpenLessInputMethodService.class.getName();
        List<InputMethodInfo> enabled = imm.getEnabledInputMethodList();
        for (InputMethodInfo info : enabled) {
            if (expectedId.equals(info.getId())) {
                return true;
            }
        }
        return false;
    }
}
