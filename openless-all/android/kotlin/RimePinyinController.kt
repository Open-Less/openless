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
    private val worker = RimeTaskQueue.executor
    private val cloudWorker = Executors.newSingleThreadExecutor()
    @Volatile private var generation = 0L
    @Volatile private var requestId = 0L
    @Volatile private var learning = false
    @Volatile private var encoding = ""
    @Volatile private var preeditText = ""
    @Volatile private var statusHint: String? = null
    @Volatile private var candidates: List<String> = emptyList()
    @Volatile private var cloudChoices: Set<String> = emptySet()
    @Volatile private var candidateRevision = 0L
    private var session = ""
    private var sessionGeneration = 0L
    private var pending = ""
    private var pendingLiteral = ""
    private var commit: (String) -> Boolean = { false }
    private var onCommitted: ((String) -> Unit)? = null
    private val confirmedText = StringBuilder()
    val associationLearning = TypedContinuationHistory()
    private var associationEpoch = 0L
    fun bindCommit(handler: (String) -> Boolean) { commit = handler }
    fun bindAfterCommit(handler: (String) -> Unit) { onCommitted = handler }
    fun currentEncoding(): String = encoding
    fun currentPreedit(): String = preeditText.ifEmpty { encoding }
    fun statusHint(): String? = statusHint
    fun isEmpty(): Boolean = encoding.isEmpty()
    fun preload() { OpenLessRime.preloadAsync(context) }
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
        if (!OpenLessRime.isReady && operation != "state") {
            statusHint = "词库初始化中…"
            main.post {
                if (epoch == generation && revision == requestId && !OpenLessRime.isReady) update(candidates)
            }
        }
        worker.execute {
            try {
                if (epoch != generation) return@execute
                OpenLessRime.initialize(context)
                // A dictionary import/export or a fuzzy-pinyin redeploy closed every native session.
                if (session.isNotEmpty() && sessionGeneration != OpenLessRime.sessionGeneration) {
                    // A failed editor write still belongs to this editor even if
                    // maintenance closed the native session before its retry.
                    session = ""
                }
                if (session.isEmpty()) {
                    sessionGeneration = OpenLessRime.sessionGeneration
                    session = OpenLessRime.request("create", "", if (learning) "learning" else "private").getString("session")
                }
                var committedNow: String? = null
                if (pending.isNotEmpty()) {
                    check(deliver(pending, epoch)) { "Editor write failed" }
                    committedNow = pending
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
                    "commitRaw" -> if (composing) "clear" else "state"
                    else -> operation
                }
                val rawBeforeClear = if (operation == "commitRaw" && composing) state.optString("input") else ""
                val nativeArgument = when (operation) {
                    "backspace" -> "65288"
                    "space", "confirmFirst" -> "32"
                    "literal" -> if (argument == "'" && composing) "39" else argument
                    "commitRaw" -> ""
                    else -> argument
                }
                if (!composing && operation in listOf("backspace", "space", "confirmFirst", "commitRaw")) {
                    if (operation == "space") check(deliver(" ", epoch)) { "Editor write failed" }
                    else check(onEditor(epoch) { emptyAction?.invoke(); true }) { "Editor unavailable" }
                } else {
                    state = OpenLessRime.request(nativeOperation, session, nativeArgument)
                }
                if (operation == "literal" && nativeOperation == "confirm") pendingLiteral = argument
                if (rawBeforeClear.isNotEmpty()) pendingLiteral = rawBeforeClear
                pending = state.optString("commit")
                if (pending.isNotEmpty()) {
                    check(deliver(pending, epoch)) { "Editor write failed" }
                    committedNow = pending
                    state = OpenLessRime.request("ack", session); pending = ""
                }
                if (pendingLiteral.isNotEmpty()) {
                    check(deliver(pendingLiteral, epoch)) { "Editor write failed" }
                    pendingLiteral = ""
                }
                if (epoch != generation || revision != requestId) return@execute
                state = OpenLessRime.request("candidates", session)
                val input = state.optString("input")
                val preedit = state.optString("preedit").ifEmpty { input }
                val rows = state.optJSONArray("candidates")
                val result = if (rows == null) emptyList() else (0 until rows.length()).map { rows.getString(it) }
                val committedForHook = committedNow
                main.post {
                    if (epoch != generation || revision != requestId) return@post
                    statusHint = null
                    encoding = input; preeditText = preedit; candidates = result; cloudChoices = emptySet()
                    candidateRevision = state.optLong("revision"); update(result)
                    if (committedForHook != null && input.isEmpty()) onCommitted?.invoke(committedForHook)
                    scheduleCloud(input, result, epoch, revision, update)
                }
            } catch (error: Throwable) {
                Log.e("OpenLessRime", "engine request failed: ${error.javaClass.simpleName}: ${error.message}", error)
                // Retain the native composition on a rejected stale selection or
                // failed editor write, rather than presenting an empty candidate bar.
                val recovered = runCatching {
                    val state = OpenLessRime.request("candidates", session)
                    val rows = state.getJSONArray("candidates")
                    val result = (0 until rows.length()).map { rows.getString(it) }
                    val input = state.optString("input")
                    val preedit = state.optString("preedit").ifEmpty { input }
                    main.post {
                        if (epoch == generation && revision == requestId) {
                            statusHint = null
                            encoding = input; preeditText = preedit; candidates = result
                            candidateRevision = state.optLong("revision"); update(result)
                        }
                    }
                }
                if (recovered.isFailure) {
                    val reason = error.message ?: error.javaClass.simpleName
                    main.post {
                        if (epoch == generation && revision == requestId) {
                            statusHint = "拼音引擎异常：$reason"
                            update(emptyList())
                        }
                    }
                }
            }
        }
    }
    fun appendLetter(letter: Char, update: (List<String>) -> Unit) {
        submit("key", letter.code.toString(), update)
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
    fun deleteCandidate(text: String, update: (List<String>) -> Unit, onResult: (Boolean) -> Unit = {}) {
        if (cloudChoices.contains(text)) { onResult(false); return }
        val index = candidates.indexOf(text)
        if (index < 0) { onResult(false); return }
        val epoch = generation
        val revision = ++requestId
        val targetRev = candidateRevision
        worker.execute {
            val deleted = runCatching {
                if (epoch != generation || session.isEmpty()) return@runCatching false
                OpenLessRime.request("delete_candidate", session, "$index@$targetRev")
                true
            }.getOrDefault(false)
            if (epoch != generation || revision != requestId) return@execute
            val refreshed = runCatching { OpenLessRime.request("candidates", session) }.getOrNull()
            main.post {
                if (epoch != generation || revision != requestId) return@post
                if (refreshed != null) {
                    val input = refreshed.optString("input")
                    val preedit = refreshed.optString("preedit").ifEmpty { input }
                    val rows = refreshed.optJSONArray("candidates")
                    val result = if (rows == null) emptyList() else (0 until rows.length()).map { rows.getString(it) }
                    encoding = input; preeditText = preedit; candidates = result
                    candidateRevision = refreshed.optLong("revision"); update(result)
                }
                onResult(deleted)
            }
        }
    }
    fun space(update: (List<String>) -> Unit) { submit("space", "", update) }
    fun confirmFirst(update: (List<String>) -> Unit, emptyAction: () -> Unit) {
        submit("confirmFirst", "", update, emptyAction)
    }
    fun commitRawInput(update: (List<String>) -> Unit, emptyAction: () -> Unit) {
        submit("commitRaw", "", update, emptyAction)
    }
    fun insertLiteral(text: String, update: (List<String>) -> Unit) { submit("literal", text, update) }
    fun clear() {
        generation++; requestId++; encoding = ""; preeditText = ""; candidates = emptyList(); cloudChoices = emptySet()
        worker.execute {
            try { if (session.isNotEmpty()) OpenLessRime.request("destroy", session) }
            catch (_: Throwable) {} finally { session = ""; pending = ""; pendingLiteral = "" }
        }
    }
    // The input queue also belongs to other controllers and dictionary maintenance.
    fun shutdown() { clear(); cloudWorker.shutdownNow() }
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
    /** What was committed just before the next commit, for StrokePhraseRepository.recordTypedContinuation(). */
    fun associationContext(): String = confirmedText.toString()
    fun resetAssociationContext() { confirmedText.clear(); associationLearning.reset(); associationEpoch++ }
    fun queryAssociations(repository: StrokePhraseRepository, update: (List<StrokePhraseRepository.Candidate>) -> Unit) {
        val text = confirmedText.toString()
        if (text.isEmpty()) { update(emptyList()); return }
        val epoch = ++associationEpoch
        repository.searchAsync(text) { result -> if (epoch == associationEpoch) update(result) }
    }
}
