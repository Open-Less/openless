package com.openless.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** The common Rime engine owns composition; the host owns target-editor writes. */
internal class RimePinyinController(context: Context) {
    private val context = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val cloudWorker = Executors.newSingleThreadExecutor()
    @Volatile private var generation = 0L
    @Volatile private var requestId = 0L
    @Volatile private var learning = false
    @Volatile private var encoding = ""
    @Volatile private var candidates: List<String> = emptyList()
    @Volatile private var cloudChoices: Set<String> = emptySet()
    @Volatile private var candidateRevision = 0L
    private var session = ""
    private var pending = ""
    private var pendingLiteral = ""
    private var commit: (String) -> Boolean = { false }
    private val confirmedText = StringBuilder()
    private var associationEpoch = 0L
    fun bindCommit(handler: (String) -> Boolean) { commit = handler }
    fun currentEncoding(): String = encoding
    fun isEmpty(): Boolean = encoding.isEmpty()
    fun setLearningAllowed(allowed: Boolean) { learning = allowed; clear(); resetAssociationContext() }
    fun refresh(update: (List<String>) -> Unit) { submit("state", "", update) }
    private fun deliver(text: String, epoch: Long): Boolean = onEditor(epoch) { commit(text) }
    private fun onEditor(epoch: Long, action: () -> Boolean): Boolean {
        val ready = CountDownLatch(1)
        val delivery = AtomicInteger(0) // waiting, executing, complete, cancelled
        var accepted = false
        if (!main.post {
            if (!delivery.compareAndSet(0, 1)) { ready.countDown(); return@post }
            try { if (epoch == generation) accepted = runCatching(action).getOrDefault(false) }
            finally { delivery.set(2); ready.countDown() }
        }) return false
        if (!ready.await(5, TimeUnit.SECONDS)) {
            if (delivery.compareAndSet(0, 3)) return false
            // A write already executing must finish before allowing a retry.
            ready.await()
        }
        return accepted && epoch == generation
    }
    private fun submit(operation: String, argument: String, update: (List<String>) -> Unit, emptyAction: (() -> Unit)? = null) {
        val epoch = generation
        val revision = ++requestId
        worker.execute {
            try {
                if (epoch != generation) return@execute
                OpenLessRime.initialize(context)
                if (session.isEmpty()) session = OpenLessRime.request("create", "", if (learning) "learning" else "private").getString("session")
                if (pending.isNotEmpty()) {
                    check(deliver(pending, epoch)) { "Editor write failed" }
                    OpenLessRime.request("ack", session); pending = ""
                }
                if (pendingLiteral.isNotEmpty()) {
                    check(deliver(pendingLiteral, epoch)) { "Editor write failed" }
                    pendingLiteral = ""
                }
                // Inspect the native composition on the worker, after all earlier keys.
                // UI snapshots may lag behind fast typing or an in-flight commit.
                var state = OpenLessRime.request("state", session)
                val composing = state.optString("input").isNotEmpty()
                val nativeOperation = when (operation) {
                    "backspace", "space", "confirmFirst" -> if (composing) "key" else "state"
                    "literal" -> if (argument == "'" && composing) "key" else "confirm"
                    else -> operation
                }
                val nativeArgument = when (operation) {
                    "backspace" -> "65288"
                    "space", "confirmFirst" -> "32"
                    "literal" -> if (argument == "'" && composing) "39" else argument
                    else -> argument
                }
                if (!composing && operation in listOf("backspace", "space", "confirmFirst")) {
                    if (operation == "space") check(deliver(" ", epoch)) { "Editor write failed" }
                    else check(onEditor(epoch) { emptyAction?.invoke(); true }) { "Editor unavailable" }
                } else {
                    state = OpenLessRime.request(nativeOperation, session, nativeArgument)
                }
                if (operation == "literal" && nativeOperation == "confirm") pendingLiteral = argument
                pending = state.optString("commit")
                if (pending.isNotEmpty()) {
                    check(deliver(pending, epoch)) { "Editor write failed" }
                    state = OpenLessRime.request("ack", session); pending = ""
                }
                if (pendingLiteral.isNotEmpty()) {
                    check(deliver(pendingLiteral, epoch)) { "Editor write failed" }
                    pendingLiteral = ""
                }
                if (epoch != generation || revision != requestId) return@execute
                state = OpenLessRime.request("candidates", session)
                val input = state.optString("input")
                val rows = state.optJSONArray("candidates")
                val result = if (rows == null) emptyList() else (0 until rows.length()).map { rows.getString(it) }
                main.post {
                    if (epoch != generation || revision != requestId) return@post
                    encoding = input; candidates = result; cloudChoices = emptySet()
                    candidateRevision = state.optLong("revision"); update(result)
                    scheduleCloud(input, result, epoch, revision, update)
                }
            } catch (error: Throwable) {
                Log.e("OpenLessRime", "engine request failed: ${error.javaClass.simpleName}")
                // Retain the native composition on a rejected stale selection or
                // failed editor write, rather than presenting an empty candidate bar.
                runCatching {
                    val state = OpenLessRime.request("candidates", session)
                    val rows = state.getJSONArray("candidates")
                    val result = (0 until rows.length()).map { rows.getString(it) }
                    main.post {
                        if (epoch == generation && revision == requestId) {
                            encoding = state.optString("input"); candidates = result
                            candidateRevision = state.optLong("revision"); update(result)
                        }
                    }
                }
            }
        }
    }
    fun appendLetter(letter: Char, update: (List<String>) -> Unit) {
        submit("key", letter.lowercaseChar().code.toString(), update)
    }
    fun backspace(update: (List<String>) -> Unit, deleteFromEditor: () -> Unit) {
        submit("backspace", "", update, deleteFromEditor)
    }
    fun selectCandidate(text: String, update: (List<String>) -> Unit) {
        if (cloudChoices.contains(text)) {
            submit("external", "$candidateRevision@$text", update)
            return
        }
        val index = candidates.indexOf(text)
        if (index >= 0) submit("select_absolute", "$index@$candidateRevision", update)
    }
    fun space(update: (List<String>) -> Unit) { submit("space", "", update) }
    fun confirmFirst(update: (List<String>) -> Unit, emptyAction: () -> Unit) {
        submit("confirmFirst", "", update, emptyAction)
    }
    fun insertLiteral(text: String, update: (List<String>) -> Unit) { submit("literal", text, update) }
    fun clear() {
        generation++; requestId++; encoding = ""; candidates = emptyList(); cloudChoices = emptySet()
        worker.execute {
            try { if (session.isNotEmpty()) OpenLessRime.request("destroy", session) }
            catch (_: Throwable) {} finally { session = ""; pending = ""; pendingLiteral = "" }
        }
    }
    fun shutdown() { clear(); worker.shutdown(); cloudWorker.shutdownNow() }
    private fun cloudEnabled(): Boolean = learning && context.getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE)
        .getBoolean("cloud_pinyin_enabled", false)

    private fun scheduleCloud(input: String, local: List<String>, epoch: Long, revision: Long, update: (List<String>) -> Unit) {
        if (!cloudEnabled() || !input.matches(Regex("[a-z']{2,64}"))) return
        main.postDelayed({
            if (!cloudEnabled() || epoch != generation || revision != requestId) return@postDelayed
            cloudWorker.execute {
                if (!cloudEnabled() || epoch != generation || revision != requestId) return@execute
                val rows = runCatching {
                    val json = org.json.JSONArray(OpenLessNative.nativeCloudPinyin(input))
                    (0 until json.length()).map { json.getString(it) }.filterNot(local::contains)
                }.getOrDefault(emptyList())
                main.post {
                    if (!cloudEnabled() || epoch != generation || revision != requestId || encoding != input) return@post
                    cloudChoices = rows.toSet()
                    update(local.take(1) + rows + local.drop(1))
                }
            }
        }, 180)
    }
    fun recordCommittedText(text: String) {
        if (!learning) return
        confirmedText.append(text)
        if (confirmedText.length > StrokeInputController.MAX_ASSOCIATION_CONTEXT)
            confirmedText.delete(0, confirmedText.length - StrokeInputController.MAX_ASSOCIATION_CONTEXT)
    }
    fun resetAssociationContext() { confirmedText.clear(); associationEpoch++ }
    fun queryAssociations(repository: StrokePhraseRepository, update: (List<StrokePhraseRepository.Candidate>) -> Unit) {
        val text = confirmedText.toString()
        if (text.isEmpty()) { update(emptyList()); return }
        val epoch = ++associationEpoch
        repository.searchAsync(text) { result -> if (epoch == associationEpoch) update(result) }
    }
}
