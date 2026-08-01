package com.openless.app

import android.content.Context
import android.net.Uri
import android.util.Log
import androidx.annotation.Keep

/**
 * Writes bytes to a SAF content:// URI via ContentResolver.
 *
 * Prefer this over tauri-plugin-fs for exports: fs detaches the FD early and
 * some providers finalize a 0-byte file before Rust finishes writing.
 */
@Keep
object OpenLessContentWriter {
    private const val TAG = "OpenLessContentWriter"

    @Keep
    @JvmStatic
    fun writeBytes(context: Context, uriString: String, bytes: ByteArray): Boolean {
        return try {
            val uri = Uri.parse(uriString)
            context.contentResolver.openOutputStream(uri)?.use { output ->
                output.write(bytes)
                output.flush()
            } ?: run {
                Log.w(TAG, "openOutputStream returned null for $uriString")
                return false
            }
            Log.i(TAG, "wrote ${bytes.size} bytes to $uriString")
            true
        } catch (error: Throwable) {
            Log.e(TAG, "failed to write $uriString", error)
            false
        }
    }
}
