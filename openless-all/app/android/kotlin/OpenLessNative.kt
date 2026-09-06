package com.openless.app

/**
 * JNI bridge from Kotlin overlay / lifecycle code into Rust Coordinator.
 */
object OpenLessNative {
    private const val BACKEND_CONTRACT_VERSION = "2.0.0"

    init {
        try {
            System.loadLibrary("openless_lib")
        } catch (error: UnsatisfiedLinkError) {
            android.util.Log.e("OpenLessNative", "failed to load openless_lib", error)
        }
    }

    @JvmStatic external fun nativeStartDictation()

    @JvmStatic external fun nativeStartDictationWithTranslation(translation: Boolean)

    @JvmStatic external fun nativeStopDictation()

    @JvmStatic external fun nativeStopDictationWithTranslation(translation: Boolean)

    @JvmStatic external fun nativeCancelDictation()

    @JvmStatic external fun nativeBackendSnapshot(): String

    @JvmStatic fun requireBackendContract() {
        val response = org.json.JSONObject(nativeBackendSnapshot())
        val version = response.optString("contractVersion")
        check(version == BACKEND_CONTRACT_VERSION) {
            "unsupported backend contract version: $version"
        }
        check(response.optBoolean("ok")) {
            response.optString("error", "backend unavailable")
        }
    }

    @JvmStatic external fun nativeSwitchStylePack()

    @JvmStatic external fun nativeOpenQaFromOverlay()

    @JvmStatic external fun nativeFinalizeQaFromOverlay()

    @JvmStatic external fun nativeGetOverlayTriggerMode(): String

    @JvmStatic external fun nativeCanDrawOverlays(context: android.content.Context): Boolean

    @JvmStatic external fun nativeShowOverlay(context: android.content.Context)

    @JvmStatic external fun nativeHideOverlay(context: android.content.Context)

    @JvmStatic external fun nativeIsOverlayVisible(): Boolean

    @JvmStatic external fun nativeNotifyOverlayPermissionChanged(context: android.content.Context)

    @JvmStatic external fun nativeNotifyOverlayDestroyed()
}
