package com.openless.app

import android.content.Context
import android.net.Uri
import android.util.Log
import androidx.annotation.Keep
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream

@Keep
sealed class OpenLessReadResult {
    data class Success(val bytes: ByteArray) : OpenLessReadResult()
    data class Failure(val reason: Reason) : OpenLessReadResult()

    enum class Reason {
        INVALID_URI,
        ACCESS_DENIED,
        TOO_LARGE,
        READ_FAILED,
        CANCELLED,
    }
}

/** Reads a bounded document and preserves failure classes for migration UI. */
@Keep
object OpenLessContentReader {
    private const val TAG = "OpenLessContentReader"
    private const val BUFFER_BYTES = 8 * 1024

    @Keep
    @JvmStatic
    fun readStrict(context: Context, uriString: String, maxBytes: Int): OpenLessReadResult {
        if (maxBytes < 0) return OpenLessReadResult.Failure(OpenLessReadResult.Reason.TOO_LARGE)
        val uri = runCatching { Uri.parse(uriString) }.getOrNull()
            ?: return OpenLessReadResult.Failure(OpenLessReadResult.Reason.INVALID_URI)
        if (uri.scheme != "content") {
            return OpenLessReadResult.Failure(OpenLessReadResult.Reason.INVALID_URI)
        }
        return try {
            val bytes = context.contentResolver.openInputStream(uri)?.use { input -> readBounded(input, maxBytes) }
                ?: return OpenLessReadResult.Failure(OpenLessReadResult.Reason.READ_FAILED)
            OpenLessReadResult.Success(bytes)
        } catch (_: SecurityException) {
            OpenLessReadResult.Failure(OpenLessReadResult.Reason.ACCESS_DENIED)
        } catch (_: java.util.concurrent.CancellationException) {
            OpenLessReadResult.Failure(OpenLessReadResult.Reason.CANCELLED)
        } catch (error: IOException) {
            if (error.message?.contains("exceeds", ignoreCase = true) == true) {
                OpenLessReadResult.Failure(OpenLessReadResult.Reason.TOO_LARGE)
            } else {
                Log.e(TAG, "failed to read selected document", error)
                OpenLessReadResult.Failure(OpenLessReadResult.Reason.READ_FAILED)
            }
        } catch (error: Throwable) {
            Log.e(TAG, "failed to read selected document", error)
            OpenLessReadResult.Failure(OpenLessReadResult.Reason.READ_FAILED)
        }
    }

    @Keep
    @JvmStatic
    fun readBytes(context: Context, uriString: String, maxBytes: Int): ByteArray? =
        when (val result = readStrict(context, uriString, maxBytes)) {
            is OpenLessReadResult.Success -> result.bytes
            is OpenLessReadResult.Failure -> null
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
