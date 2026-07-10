package com.openless.app

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log

class MicrophonePermissionActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            OpenLessPermissionBridge.resolveRecordAudioPermission(true)
            finish()
            return
        }
        requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), REQUEST_RECORD_AUDIO)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQUEST_RECORD_AUDIO) {
            return
        }
        val granted = grantResults.isNotEmpty() &&
            grantResults[0] == PackageManager.PERMISSION_GRANTED
        Log.i(TAG, "RECORD_AUDIO permission result granted=$granted")
        OpenLessPermissionBridge.resolveRecordAudioPermission(granted)
        finish()
    }

    override fun onDestroy() {
        if (isFinishing) {
            OpenLessPermissionBridge.resolveRecordAudioPermission(
                checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED,
            )
        }
        super.onDestroy()
    }

    companion object {
        private const val TAG = "OpenLessMicPermission"
        private const val REQUEST_RECORD_AUDIO = 9101
    }
}
