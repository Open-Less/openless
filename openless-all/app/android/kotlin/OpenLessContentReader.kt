package com.openless.app

import android.content.Context
import android.net.Uri
import android.util.Log
import androidx.annotation.Keep
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream

/** Reads a bounded document selected through Android's Storage Access Framework. */
@Keep
object OpenLessContentReader {
    private const val TAG = "OpenLessContentReader"
    private const val BUFFER_BYTES = 8 * 1024

    @Keep
    @JvmStatic
    fun readBytes(context: Context, uriString: String, maxBytes: Int): ByteArray? {
        return try {
            val uri = Uri.parse(uriString)
            context.contentResolver.openInputStream(uri)?.use { input ->
                readBounded(input, maxBytes)
            }
                ?: run {
                    Log.w(TAG, "openInputStream returned null for selected document")
                    null
                }
        } catch (error: Throwable) {
            Log.e(TAG, "failed to read selected document", error)
            null
        }
    }

    internal fun readBounded(input: InputStream, maxBytes: Int): ByteArray {
        require(maxBytes >= 0) { "maxBytes must not be negative" }
        val output = ByteArrayOutputStream(minOf(maxBytes, BUFFER_BYTES))
        val buffer = ByteArray(BUFFER_BYTES)
        var total = 0
        while (true) {
            val count = input.read(buffer)
            if (count < 0) break
            if (count == 0) continue
            if (total > maxBytes - count) {
                throw IOException("selected document exceeds $maxBytes bytes")
            }
            output.write(buffer, 0, count)
            total += count
        }
        return output.toByteArray()
    }
}
