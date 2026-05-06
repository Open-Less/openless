package com.openless.android;

final class PermissionStatus {
    static final String ACTION_APP_PERMISSIONS = "app_permissions";
    static final String ACTION_OVERLAY = "overlay";
    static final String ACTION_NOTIFICATIONS = "notifications";
    static final String ACTION_IME = "ime";
    static final String ACTION_NONE = "none";

    final String title;
    final String detail;
    final boolean ok;
    final String action;
    final String actionLabel;

    PermissionStatus(String title, String detail, boolean ok, String action, String actionLabel) {
        this.title = title;
        this.detail = detail;
        this.ok = ok;
        this.action = action;
        this.actionLabel = actionLabel;
    }
}
