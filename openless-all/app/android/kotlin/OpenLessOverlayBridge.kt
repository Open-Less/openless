package com.openless.app

import android.os.Handler
import android.os.Looper
import androidx.annotation.Keep

/**
 * Rust calls back into this object to refresh overlay UI state.
 */
@Keep
object OpenLessOverlayBridge {
    private val mainHandler = Handler(Looper.getMainLooper())

    @Volatile
    var listener: OverlayStateListener? = null

    interface OverlayStateListener {
        fun onCapsuleStateChanged(state: String, message: String?)
    }

    @Keep
    @JvmStatic
    fun onCapsuleStateChanged(state: String, message: String?) {
        mainHandler.post {
            listener?.onCapsuleStateChanged(state, message)
        }
    }

    @Keep
    @JvmStatic
    fun showToast(message: String) {
        mainHandler.post {
            val service = OpenLessOverlayService.instance ?: return@post
            android.widget.Toast.makeText(service.applicationContext, message, android.widget.Toast.LENGTH_SHORT).show()
        }
    }
}
