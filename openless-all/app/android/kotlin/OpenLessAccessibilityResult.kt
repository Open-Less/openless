package com.openless.app

/**
 * Structured result for accessibility command IPC.
 * [code] values are sent via [android.os.ResultReceiver].
 */
enum class AccessibilityPasteResult(val code: Int) {
    SUCCESS(1),
    SERVICE_NOT_CONNECTED(2),
    NO_FOCUSED_EDITOR(3),
    PASTE_REJECTED(4),
    TIMEOUT(5),
    IPC_PROTOCOL_ERROR(6),
    ;

    val reason: String
        get() = name

    companion object {
        fun fromCode(code: Int): AccessibilityPasteResult {
            return entries.firstOrNull { it.code == code } ?: IPC_PROTOCOL_ERROR
        }
    }
}
