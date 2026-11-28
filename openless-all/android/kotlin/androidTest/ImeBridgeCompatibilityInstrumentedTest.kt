package com.openless.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith

/** Listener compatibility independent of microphone, backend and app-private data. */
@RunWith(AndroidJUnit4::class)
class ImeBridgeCompatibilityInstrumentedTest {
    @Test fun legacyListenerReceivesOneNotificationFromMetadataCallback() {
        val states = mutableListOf<String>()
        val listener = object : OpenLessOverlayBridge.OverlayStateListener {
            override fun onCapsuleStateChanged(state: String, message: String?, level: Float) {
                states += "$state:$message:$level"
            }
        }
        listener.onCapsuleStateChangedWithSession("recording", "ready", 0.5f, "session-a")
        assertEquals(listOf("recording:ready:0.5"), states)
    }

    @Test fun scopedListenerReceivesOpaqueIdentityWithoutLegacyDoubleDispatch() {
        var legacyCalls = 0
        var scopedCalls = 0
        var identity: String? = null
        val listener = object : OpenLessOverlayBridge.OverlayStateListener {
            override fun onCapsuleStateChanged(state: String, message: String?, level: Float) { legacyCalls++ }
            override fun onCapsuleStateChangedWithSession(state: String, message: String?, level: Float, sessionId: String?) {
                scopedCalls++
                identity = sessionId
            }
        }
        listener.onCapsuleStateChangedWithSession("done", null, 0f, "Opaque-ID 7")
        assertEquals(0, legacyCalls)
        assertEquals(1, scopedCalls)
        assertEquals("Opaque-ID 7", identity)
    }

    @Test fun unscopedLegacyNotificationsRemainExplicitlyUnknown() {
        var identity: String? = "unexpected"
        val listener = object : OpenLessOverlayBridge.OverlayStateListener {
            override fun onCapsuleStateChanged(state: String, message: String?, level: Float) = Unit
            override fun onCapsuleStateChangedWithSession(state: String, message: String?, level: Float, sessionId: String?) {
                identity = sessionId
            }
        }
        listener.onCapsuleStateChangedWithSession("error", "unscoped", 0f, null)
        assertNull(identity)
    }
}
