package com.openless.app

import android.content.ComponentName
import android.content.Context
import android.content.ServiceConnection
import android.os.IBinder
import android.util.Log
import rikka.shizuku.Shizuku
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import java.util.concurrent.locks.ReentrantLock

/**
 * Binds the Shizuku UserService for synchronous privileged operations.
 * Recovery calls are serialized; unbind removes the UserService after each operation.
 */
internal object OpenLessShizukuUserServiceClient {
    private const val TAG = "OpenLessShizukuClient"
    private const val BIND_TIMEOUT_MS = 8_000L
    private const val SERVICE_VERSION = 3

    private val recoveryLock = ReentrantLock()

    @Volatile
    private var recoveryInProgress = false

    fun <T> withService(context: Context, block: (IOpenLessShizukuUserService) -> T): T? {
        if (!Shizuku.pingBinder()) {
            return null
        }
        val component = ComponentName(context.packageName, OpenLessShizukuUserService::class.java.name)
        val args = Shizuku.UserServiceArgs(component)
            .daemon(false)
            .version(SERVICE_VERSION)
            .tag("openless_shizuku_recovery")

        val latch = CountDownLatch(1)
        val binderRef = AtomicReference<IOpenLessShizukuUserService?>(null)
        val connection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                binderRef.set(IOpenLessShizukuUserService.Stub.asInterface(binder))
                latch.countDown()
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                binderRef.set(null)
            }
        }

        return try {
            Shizuku.bindUserService(args, connection)
            if (!latch.await(BIND_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
                Log.w(TAG, "UserService bind timed out")
                return null
            }
            val service = binderRef.get() ?: return null
            block(service)
        } catch (error: Throwable) {
            Log.w(TAG, "UserService bind failed", error)
            null
        } finally {
            try {
                Shizuku.unbindUserService(args, connection, true)
            } catch (error: Throwable) {
                Log.w(TAG, "UserService unbind failed", error)
            }
        }
    }

    fun <T> withRecoveryLock(block: () -> T): T? {
        if (!recoveryLock.tryLock()) {
            return null
        }
        return try {
            if (recoveryInProgress) {
                null
            } else {
                recoveryInProgress = true
                try {
                    block()
                } finally {
                    recoveryInProgress = false
                }
            }
        } finally {
            recoveryLock.unlock()
        }
    }

    fun isRecoveryInProgress(): Boolean = recoveryInProgress
}
