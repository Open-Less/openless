package com.openless.app

/** Capsule frames are global; only the IME result channel may complete an IME request. */
internal enum class ImeVoiceFrameAction {
    EXTERNAL, IGNORE, RECORDING, PROCESSING, WAIT_RESULT, EXTERNALLY_COMPLETED, CANCELLED, ERROR,
}

internal object ImeVoiceFramePolicy {
    /** No second stop/mode mutation once a buffered frame shows finalization. */
    fun canApplyQueuedStartActions(bufferedState: String?): Boolean = bufferedState !in setOf(
        "transcribing", "polishing", "done", "cancelled", "error",
    )

    fun resolve(
        ownsRequest: Boolean,
        captureEnded: Boolean,
        state: String,
        stopSubmitted: Boolean = captureEnded,
    ): ImeVoiceFrameAction {
        if (!ownsRequest) return ImeVoiceFrameAction.EXTERNAL
        return when (state) {
            "recording" -> if (captureEnded) ImeVoiceFrameAction.IGNORE else ImeVoiceFrameAction.RECORDING
            "transcribing", "polishing" -> ImeVoiceFrameAction.PROCESSING
            // A global done cue may precede the request-scoped text delivery.
            // Do not open another start window and then drop the prior result.
            "done" -> if (stopSubmitted) ImeVoiceFrameAction.WAIT_RESULT else ImeVoiceFrameAction.EXTERNALLY_COMPLETED
            "cancelled" -> ImeVoiceFrameAction.CANCELLED
            "error" -> ImeVoiceFrameAction.ERROR
            else -> ImeVoiceFrameAction.IGNORE
        }
    }
}
