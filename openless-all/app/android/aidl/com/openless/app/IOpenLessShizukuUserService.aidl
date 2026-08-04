package com.openless.app;

/**
 * Privileged UserService for accessibility recovery and paste-key injection.
 * Single typed entry points — no generic shell or arbitrary secure-settings API.
 */
interface IOpenLessShizukuUserService {
    void destroy() = 16777114;

    /**
     * Best-effort read, merge, write, and verify enabled_accessibility_services.
     * Returns JSON: { "outcome": "...", "messageKey": "..." }.
     */
    String recoverAccessibilityService(String serviceComponent) = 1;

    /** Inject KEYCODE_PASTE (279) via shell input. */
    boolean injectPasteKey() = 2;
}
