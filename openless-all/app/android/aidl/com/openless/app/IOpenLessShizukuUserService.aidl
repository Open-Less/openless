package com.openless.app;



/**

 * Privileged UserService for accessibility recovery only.

 * Single typed entry point — no generic shell or arbitrary secure-settings API.

 */

interface IOpenLessShizukuUserService {

    void destroy() = 16777114;



    /**

     * Best-effort read, merge, write, and verify enabled_accessibility_services.

     * Returns JSON: { "outcome": "...", "messageKey": "..." }.

     */

    String recoverAccessibilityService(String serviceComponent) = 1;

}

