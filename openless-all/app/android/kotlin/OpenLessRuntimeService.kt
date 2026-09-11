package com.openless.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log

/** Keeps the OpenLess process alive while the system IME is active, without showing an overlay. */
class OpenLessRuntimeService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        try {
            val notification = buildNotification()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
                    } else {
                        0
                    },
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (error: Throwable) {
            Log.e(TAG, "failed to promote IME runtime service", error)
            stopSelf(startId)
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    private fun buildNotification(): Notification {
        val channelId = "openless_ime_runtime"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(
                    channelId,
                    "OpenLess 输入法服务",
                    NotificationManager.IMPORTANCE_LOW,
                )
            )
        }
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, channelId)
                .setContentTitle("OpenLess 输入法")
                .setContentText("输入法面板已就绪")
                .setSmallIcon(R.mipmap.ic_launcher)
                .setOngoing(true)
                .build()
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
                .setContentTitle("OpenLess 输入法")
                .setContentText("输入法面板已就绪")
                .setSmallIcon(R.mipmap.ic_launcher)
                .setOngoing(true)
                .build()
        }
    }

    companion object {
        private const val NOTIFICATION_ID = 42002
        private const val TAG = "OpenLessRuntimeService"
    }
}
