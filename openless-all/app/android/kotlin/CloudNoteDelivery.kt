package com.openless.app

/**
 * What to do with a cloud-note transcript the webhook did not accept.
 *
 * The backend deletes the recording and history before the IME learns whether
 * the webhook succeeded. A failed submit therefore has to keep the text the
 * completion event already delivered, or the dictation is gone.
 */
internal enum class CloudNoteFailure {
    MissingDestination,
    Http,
    Network,
}

internal fun cloudNoteShouldRetain(text: String): Boolean = text.isNotBlank()

internal fun cloudNoteFailureDetail(failure: CloudNoteFailure, httpCode: Int = 0): Pair<String, String> {
    return when (failure) {
        CloudNoteFailure.MissingDestination ->
            "请先在设置中填写云笔记的地址和 Token" to "Add the cloud note URL and token in Settings"
        CloudNoteFailure.Http ->
            "云笔记提交失败（$httpCode）" to "Cloud note submit failed ($httpCode)"
        CloudNoteFailure.Network ->
            "云笔记提交失败，请检查网络" to "Cloud note submit failed. Check the network"
    }
}

internal fun cloudNoteFailureMessage(
    detail: Pair<String, String>,
    retained: Boolean,
): Pair<String, String> {
    val (zh, en) = detail
    return if (retained) {
        "$zh，内容已保存在键盘剪贴板" to "$en. The text was saved in the keyboard clipboard."
    } else {
        zh to en
    }
}
