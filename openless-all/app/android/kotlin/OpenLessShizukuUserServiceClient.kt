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
    private const val USER_SERVICE_PROCESS_SUFFIX = "shizuku"
    private const val PASTE_SERVICE_PROCESS_SUFFIX = "paste"

    private val recoveryLock = ReentrantLock()

    @Volatile
    private var recoveryInProgress = false

    fun <T> withService(context: Context, block: (IOpenLessShizukuUserService) -> T): T? {
        return bindUserService(
            context = context,
            daemon = false,
            processNameSuffix = USER_SERVICE_PROCESS_SUFFIX,
            tag = "openless_shizuku",
            block = block,
        )
    }

    /**
     * Paste injection uses a daemon UserService so MTK/Xiaomi ROMs do not spawn
     * `com.openless.app:shizuku`, where LoadedApk.makeApplicationInner NPEs.
     */
    fun <T> withPasteService(context: Context, block: (IOpenLessShizukuUserService) -> T): T? {
        return bindUserService(
            context = context,
            daemon = true,
            processNameSuffix = PASTE_SERVICE_PROCESS_SUFFIX,
            tag = "openless_paste",
            block = block,
        )
    }

    private fun <T> bindUserService(
        context: Context,
        daemon: Boolean,
        processNameSuffix: String,
        tag: String,
        block: (IOpenLessShizukuUserService) -> T,
    ): T? {
        if (!Shizuku.pingBinder()) {
            return null
        }
        val component = ComponentName(context.packageName, OpenLessShizukuUserService::class.java.name)
        val args = Shizuku.UserServiceArgs(component)
            .daemon(daemon)
            .processNameSuffix(processNameSuffix)
            .version(SERVICE_VERSION)
            .tag(tag)

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
                Log.w(TAG, "UserService bind timed out tag=$tag daemon=$daemon")
                return null
            }
            val service = binderRef.get() ?: return null
            block(service)
        } catch (error: Throwable) {
            Log.w(TAG, "UserService bind failed tag=$tag daemon=$daemon", error)
            null
        } finally {
            try {
                Shizuku.unbindUserService(args, connection, true)
            } catch (error: Throwable) {
                Log.w(TAG, "UserService unbind failed tag=$tag", error)
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
