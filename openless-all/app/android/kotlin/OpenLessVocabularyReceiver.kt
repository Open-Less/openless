package com.openless.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ResultReceiver

/** Runs in the main process, where Core owns consent, generations and vocabulary. */
class OpenLessVocabularyReceiver : BroadcastReceiver() {
    @Suppress("DEPRECATION")
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent == null) return
        val reply = intent.getParcelableExtra("reply") as? ResultReceiver ?: return
        val response = Bundle()
        val ok = try {
            when (intent.getStringExtra("operation")) {
                "current" -> {
                    response.putLong("generation", OpenLessNative.nativeCurrentVocabularyObservation())
                    true
                }
                "stop" -> {
                    OpenLessNative.nativeStopVocabularyObservation(intent.getLongExtra("generation", 0))
                    true
                }
                "active" -> {
                    // elapsedRealtime is shared across processes. Timestamp before the
                    // native query so IPC latency can only shorten the observation.
                    val now = android.os.SystemClock.elapsedRealtime()
                    val remaining = OpenLessNative.nativeVocabularyObservationRemainingMs(intent.getLongExtra("generation", 0))
                    response.putLong("deadline", now + remaining)
                    remaining > 0L
                }
                "observe" -> {
                    val text = intent.getStringExtra("text")
                    text != null && text.length <= 20_000 && OpenLessNative.nativeObserveVocabularyText(intent.getLongExtra("generation", 0), text)
                }
                "pending" -> {
                    response.putString("json", OpenLessNative.nativePendingVocabularySuggestions())
                    true
                }
                "resolve" -> OpenLessNative.nativeResolveVocabularySuggestion(intent.getStringExtra("id").orEmpty(), intent.getBooleanExtra("accept", false))
                else -> false
            }
        } catch (_: Throwable) { false }
        reply.send(if (ok) 1 else 0, response)
    }
}

/** Explicit non-exported IPC. No Rust singleton is accessed from :accessibility. */
internal object OpenLessVocabularyIpc {
    fun request(context: Context, operation: String, data: Bundle = Bundle(), callback: (Boolean, Bundle?) -> Unit) {
        val handler = Handler(Looper.getMainLooper())
        var completed = false // read and written only on this handler's looper
        val timeout = Runnable {
            if (!completed) { completed = true; callback(false, null) }
        }
        val reply = object : ResultReceiver(handler) {
            override fun onReceiveResult(resultCode: Int, resultData: Bundle?) {
                if (completed) return
                completed = true
                handler.removeCallbacks(timeout)
                callback(resultCode == 1, resultData)
            }
        }
        handler.postDelayed(timeout, 500L)
        try {
            context.sendBroadcast(Intent(context, OpenLessVocabularyReceiver::class.java).apply {
                putExtras(data)
                putExtra("operation", operation)
                putExtra("reply", reply)
            })
        } catch (_: Throwable) {
            handler.removeCallbacks(timeout)
            if (!completed) { completed = true; callback(false, null) }
        }
    }
}
