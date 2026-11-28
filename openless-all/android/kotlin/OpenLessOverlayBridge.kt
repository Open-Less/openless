package com.openless.app

import android.os.Handler
import android.os.Looper
import androidx.annotation.Keep

/** Rust calls back into this object to refresh overlay UI state. */
@Keep
object OpenLessOverlayBridge {
    private val mainHandler = Handler(Looper.getMainLooper())

    @Volatile var listener: OverlayStateListener? = null
    @Volatile var imeListener: OverlayStateListener? = null
    @Volatile var imeSessionListener: ((String) -> Unit)? = null

    interface OverlayStateListener {
        fun onCapsuleStateChanged(state: String, message: String?, level: Float)

        /** Legacy observers can keep implementing only the original callback. */
        fun onCapsuleStateChangedWithSession(state: String, message: String?, level: Float, sessionId: String?) {
            onCapsuleStateChanged(state, message, level)
        }
    }

    @Keep
    @JvmStatic
    fun onCapsuleStateChanged(state: String, message: String?, level: Float) {
        onCapsuleStateChangedWithSession(state, message, level, null)
    }

    @Keep
    @JvmStatic
    fun onCapsuleStateChangedWithSession(state: String, message: String?, level: Float, sessionId: String?) {
        val identity = sessionId?.takeUnless { it.isBlank() }
        mainHandler.post {
            listener?.onCapsuleStateChangedWithSession(state, message, level, identity)
            imeListener?.onCapsuleStateChangedWithSession(state, message, level, identity)
        }
    }

    @Keep
    @JvmStatic
    fun onImeSessionEvent(json: String) {
        mainHandler.post { imeSessionListener?.invoke(json) }
    }

    @Keep
    @JvmStatic
    fun showToast(message: String) {
        mainHandler.post {
            val service = OpenLessOverlayService.instance ?: return@post
            OpenLessNativeNotice.makeText(
                    service.applicationContext,
                    message,
                    android.widget.Toast.LENGTH_SHORT,
                )
                .show()
        }
    }
}
