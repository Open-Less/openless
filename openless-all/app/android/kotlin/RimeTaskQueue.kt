package com.openless.app

import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors

/** Input (including editor delivery and ack) and session-closing maintenance share one queue. */
internal object RimeTaskQueue {
    @Volatile private var workerThread: Thread? = null
    val executor = Executors.newSingleThreadExecutor { task ->
        Thread(task, "openless-rime").apply {
            isDaemon = true
            workerThread = this
        }
    }

    /** Maintenance callers are off the main thread; nested calls on this queue run directly. */
    fun <T> run(task: () -> T): T {
        if (Thread.currentThread() === workerThread) return task()
        return try {
            executor.submit(Callable { task() }).get()
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw error
        } catch (error: ExecutionException) {
            throw error.cause ?: error
        }
    }
}
