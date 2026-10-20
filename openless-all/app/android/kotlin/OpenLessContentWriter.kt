package com.openless.app

import android.content.Context
import android.content.ContentValues
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.Log
import androidx.annotation.Keep
import java.io.File
import java.io.IOException

@Keep
sealed class OpenLessWriteResult {
    data object Success : OpenLessWriteResult()
    data class Failure(val reason: Reason) : OpenLessWriteResult()

    enum class Reason {
        INVALID_URI,
        ACCESS_DENIED,
        SHORT_WRITE,
        FLUSH_FAILED,
        WRITE_FAILED,
        CANCELLED,
    }
}

/** Writes bounded bytes to a user-selected SAF document. */
@Keep
object OpenLessContentWriter {
    private const val TAG = "OpenLessContentWriter"

    @Keep
    @JvmStatic
    fun writeStrict(context: Context, uriString: String, bytes: ByteArray): OpenLessWriteResult {
        val uri = runCatching { Uri.parse(uriString) }.getOrNull()
            ?: return OpenLessWriteResult.Failure(OpenLessWriteResult.Reason.INVALID_URI)
        if (uri.scheme != "content") {
            return OpenLessWriteResult.Failure(OpenLessWriteResult.Reason.INVALID_URI)
        }
        return try {
            val output = context.contentResolver.openOutputStream(uri)
                ?: return OpenLessWriteResult.Failure(OpenLessWriteResult.Reason.WRITE_FAILED)
            var written = 0
            output.use { stream ->
                stream.write(bytes)
                written = bytes.size
                try {
                    stream.flush()
                } catch (_: Throwable) {
                    return OpenLessWriteResult.Failure(OpenLessWriteResult.Reason.FLUSH_FAILED)
                }
            }
            if (written != bytes.size) {
                OpenLessWriteResult.Failure(OpenLessWriteResult.Reason.SHORT_WRITE)
            } else {
                OpenLessWriteResult.Success
            }
        } catch (_: SecurityException) {
            OpenLessWriteResult.Failure(OpenLessWriteResult.Reason.ACCESS_DENIED)
        } catch (_: java.util.concurrent.CancellationException) {
            OpenLessWriteResult.Failure(OpenLessWriteResult.Reason.CANCELLED)
        } catch (error: Throwable) {
            Log.e(TAG, "failed to write selected document", error)
            OpenLessWriteResult.Failure(OpenLessWriteResult.Reason.WRITE_FAILED)
        }
    }

    @Keep
    @JvmStatic
    fun writeBytes(context: Context, uriString: String, bytes: ByteArray): Boolean =
        writeStrict(context, uriString, bytes) is OpenLessWriteResult.Success

    /**
     * Writes an app-generated export into Downloads without a SAF picker.
     *
     * Android 10+ uses MediaStore public Downloads (no storage permission).
     * Older Android writes to the app-specific external Downloads directory
     * instead — public Downloads would need WRITE_EXTERNAL_STORAGE which this
     * app does not declare.
     */
    @Keep
    @JvmStatic
    fun writePublicDownload(context: Context, fileName: String, bytes: ByteArray): String {
        val safeName = fileName
            .substringAfterLast('/')
            .substringAfterLast('\\')
            .trim()
            .takeIf { it.isNotEmpty() && it != "." && it != ".." }
            ?: "openless-error.log"
        @Suppress("DEPRECATION")
        val displayFile = File(
            Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS),
            safeName,
        )

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val values = ContentValues().apply {
                put(MediaStore.MediaColumns.DISPLAY_NAME, safeName)
                put(MediaStore.MediaColumns.MIME_TYPE, "text/plain")
                put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
            }
            val resolver = context.contentResolver
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: throw IOException("MediaStore failed to create Downloads item")
            try {
                resolver.openOutputStream(uri)?.use { output ->
                    output.write(bytes)
                    output.flush()
                } ?: throw IOException("MediaStore returned no output stream")
            } catch (error: Throwable) {
                resolver.delete(uri, null, null)
                throw error
            }
            Log.i(TAG, "wrote ${bytes.size} bytes to public Downloads: $displayFile")
            return displayFile.absolutePath
        }

        // Pre-Android 10: public Downloads needs WRITE_EXTERNAL_STORAGE which
        // this app does not declare. Write to app-specific external Downloads
        // instead (no runtime permission; still a real on-device .log path).
        val appDownloads = context.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS)
            ?: throw IOException("app-specific Downloads directory unavailable")
        if (!appDownloads.exists() && !appDownloads.mkdirs()) {
            throw IOException("failed to create app-specific Downloads directory")
        }
        val appFile = File(appDownloads, safeName)
        appFile.outputStream().use { output ->
            output.write(bytes)
            output.flush()
        }
        Log.i(TAG, "wrote ${bytes.size} bytes to app Downloads (pre-Q): $appFile")
        return appFile.absolutePath
    }
}
