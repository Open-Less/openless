package com.openless.android;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;

final class TextInserter {
    static final class Result {
        final InsertStatus status;
        final String appBundleId;
        final String appName;

        Result(InsertStatus status, String appBundleId, String appName) {
            this.status = status;
            this.appBundleId = appBundleId;
            this.appName = appName;
        }
    }

    private final Context context;

    TextInserter(Context context) {
        this.context = context.getApplicationContext();
    }

    Result insertOrCopy(String text, boolean allowClipboardFallback) {
        String appBundleId = OpenLessInputMethodService.currentTargetPackage();
        String appName = resolveAppName(appBundleId);
        if (OpenLessInputMethodService.commitToCurrentInput(text)) {
            return new Result(InsertStatus.INSERTED, appBundleId, appName);
        }
        if (!allowClipboardFallback) {
            return new Result(InsertStatus.FAILED, appBundleId, appName);
        }
        ClipboardManager clipboard = (ClipboardManager) context.getSystemService(Context.CLIPBOARD_SERVICE);
        if (clipboard == null) {
            return new Result(InsertStatus.FAILED, appBundleId, appName);
        }
        clipboard.setPrimaryClip(ClipData.newPlainText("OpenLess", text));
        return new Result(InsertStatus.COPIED_FALLBACK, appBundleId, appName);
    }

    boolean isImeActive() {
        return OpenLessInputMethodService.isActive();
    }

    private String resolveAppName(String appBundleId) {
        if (appBundleId == null || appBundleId.isEmpty()) {
            return null;
        }
        try {
            PackageManager pm = context.getPackageManager();
            ApplicationInfo info = pm.getApplicationInfo(appBundleId, 0);
            CharSequence label = pm.getApplicationLabel(info);
            String value = label == null ? null : label.toString().trim();
            return value == null || value.isEmpty() ? null : value;
        } catch (Exception ignored) {
            return null;
        }
    }
}
