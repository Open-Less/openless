package com.openless.app

/**
 * JNI bridge from Kotlin overlay / lifecycle code into Rust Coordinator.
 */
object OpenLessNative {
    init {
        try {
            System.loadLibrary("openless_lib")
        } catch (error: UnsatisfiedLinkError) {
            android.util.Log.e("OpenLessNative", "failed to load openless_lib", error)
        }
    }

    @JvmStatic external fun nativeStartDictation()

    @JvmStatic external fun nativeStopDictation()

    @JvmStatic external fun nativeCancelDictation()

    @JvmStatic external fun nativeGetOverlayTriggerMode(): String

    @JvmStatic external fun nativeCanDrawOverlays(): Boolean

    @JvmStatic external fun nativeShowOverlay()

    @JvmStatic external fun nativeHideOverlay()

    @JvmStatic external fun nativeIsOverlayVisible(): Boolean

    @JvmStatic external fun nativeNotifyOverlayPermissionChanged()
}
