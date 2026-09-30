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

/**
 * Writes bytes to a SAF content:// URI via ContentResolver.
 *
 * Prefer this over tauri-plugin-fs for exports: fs detaches the FD early and some providers
 * finalize a 0-byte file before Rust finishes writing.
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
            }
                ?: run {
                    Log.w(TAG, "openOutputStream returned null for selected document")
                    return false
                }
            Log.i(TAG, "wrote ${bytes.size} bytes to selected document")
            true
        } catch (error: Throwable) {
            Log.e(TAG, "failed to write selected document", error)
            false
        }
    }

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