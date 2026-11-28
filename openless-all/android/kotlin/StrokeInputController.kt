package com.openless.app

import android.view.View

/** Stroke engine/session adapter. UI components do not own input or learning. */
internal class StrokeInputController(private val service: OpenLessImeService) {
    private val strokeRepository by lazy { StrokeInputRepository(service) }
    // internal, not private: Pinyin mode's own post-commit association
    // (RimePinyinController) reuses this exact instance rather than loading
    // a second copy of the same ~220k-entry phrase index — association data
    // is keyed by confirmed on-screen Chinese text, not by how it was
    // typed, so it's equally valid for either input mode.
    internal val phraseRepository by lazy { StrokePhraseRepository(service) }

    private var strokeCode = ""
    private var strokeQueryEpoch = 0L
    private val wordSegments = mutableListOf<String>()
    private var lastStrokeCandidates: List<String> = emptyList()
    private var confirmedText = ""
    private val associationLearning = TypedContinuationHistory()
    private var phraseQueryEpoch = 0L
    private var panel: ImeStrokePanel? = null
    private var candidateEntries = emptyList<CandidateSpec>()
    private var inputGeneration = 0L

    // Read by OpenLessImeService's own panel dispatcher (onCreateInputView())
    // to choose between buildStrokeView()/buildStrokeNumberView() — the only
    // piece of stroke state anything outside this class still touches.
    var strokeNumberMode = false
    private var numberSymbolMode = false
    private var symbolPageIndex = 0
    private val numberPanelSymbolPages = listOf(
        listOf("、", "。", "，", "；", "：", "？", "！", "…", "—", "～", "·", "（"),
        listOf("）", "《", "》", "“", "”", "‘", "’", "【", "】", "「", "」", "￥"),
        listOf("%", "#", "&", "*", "=", "/", "\\", "<", ">", "^", "_", "|"),
    )
    private var punctuationGroupIndex = 0
    private val punctuationGroups = listOf(
        listOf(",", "°", "?", "!", "~"),
        listOf(".", "、", ";", ":", "\""),
        listOf("(", ")", "[", "]", "-"),
        listOf("@", "#", "$", "%", "&"),
        listOf("*", "+", "=", "/", "_"),
    )

    /** Warm the offline stroke index and phrase-association trie while the IME is idle — see OpenLessImeService.onCreate(). */
    fun preloadAsync() {
        strokeRepository.preloadAsync()
        phraseRepository.preloadAsync()
    }

    fun shutdown() {
        strokeRepository.shutdown()
        phraseRepository.shutdown()
    }

    /** Explicit mode switches discard pending composition, never carrying segments into a later mode. */
    fun resetForModeSwitch() {
        ++inputGeneration
        panel?.cancelInteractions()
        candidateEntries = emptyList()
        wordSegments.clear()
        lastStrokeCandidates = emptyList()
        associationLearning.reset()
        strokeNumberMode = false
        numberSymbolMode = false
        symbolPageIndex = 0
        punctuationGroupIndex = 0
        strokeCode = ""
        strokeQueryEpoch++
        confirmedText = ""
        phraseQueryEpoch++
    }

    /** Matches onStartInput()'s own old inline reset exactly — a fresh input session also drops any in-progress word segments/candidates, but leaves the punctuation rail's page alone. */
    fun resetForNewInputSession() {
        ++inputGeneration
        panel?.cancelInteractions()
        associationLearning.reset()
        strokeNumberMode = false
        numberSymbolMode = false
        symbolPageIndex = 0
        confirmedText = ""
        phraseQueryEpoch++
        strokeCode = ""
        strokeQueryEpoch++
        wordSegments.clear()
        lastStrokeCandidates = emptyList()
        candidateEntries = emptyList()
        refreshPanel()
    }

    /**
     * The host emptied the field (e.g. a chat app's send button): start over
     * — no association context, no half-typed code or 分词 segments, and an
     * empty code row and candidate row. The candidate overlay entries are
     * shared with the English panel, so they're only touched while this
     * panel is the one showing.
     */
    fun clearAfterFieldEmptied() {
        associationLearning.reset()
        if (service.inputMode == OpenLessImeService.InputMode.STROKE) {
            abandonComposition()
        } else {
            confirmedText = ""
            phraseQueryEpoch++
        }
    }

    fun buildStrokeView(): View = buildPanel()
    fun buildStrokeNumberView(): View = buildPanel()

    private fun buildPanel(): View {
        panel?.cancelInteractions()
        return ImeStrokePanel(
            service, service.keyboardPalette(), service.buildKeyboardHeader(), service.isEnglishKeyboardUi(),
            onCommand = ::handleCommand,
            onText = { text -> if (isCurrentPanel()) commitDirect(text) },
            onPunctuationPage = { step ->
                if (isCurrentPanel()) {
                    punctuationGroupIndex = (punctuationGroupIndex + step).mod(punctuationGroups.size)
                    refreshPanel()
                }
            },
            onFeedback = service::emitKeyFeedback,
            interactionPolicy = service.interactionPolicy(),
        ).also {
            panel = it
            refreshPanel()
            if (strokeCode.isNotEmpty()) refreshStrokeCandidates(strokeCode)
        }
    }

    private fun isCurrentPanel(): Boolean =
        service.inputMode == OpenLessImeService.InputMode.STROKE && service.isInputViewShown && !service.isKeyboardEditingBlocked()

    fun cancelInteractions() { panel?.cancelInteractions() }
    fun applyInteractionPolicy(policy: ImeInteractionPolicy) { panel?.applyInteractionPolicy(policy) }
    fun setInteractionBlocked(blocked: Boolean) { panel?.setBlocked(blocked) }

    fun refreshPanel(): Boolean {
        val current = panel ?: return false
        val preview = android.text.TextUtils.concat(wordSegments.joinToString(""), service.displayStrokeCode(strokeCode))
        current.render(
            preview, service.traditionalOutput, strokeNumberMode, numberSymbolMode,
            symbolPageIndex, numberPanelSymbolPages.size,
            numberPanelSymbolPages[symbolPageIndex.coerceIn(numberPanelSymbolPages.indices)],
            punctuationGroups[punctuationGroupIndex],
        )
        val generation = inputGeneration
        current.submitCandidates(candidateEntries.map { entry ->
            ImeStrokePanel.Candidate(entry.label) {
                if (generation == inputGeneration && isCurrentPanel()) entry.action()
            }
        })
        current.setBlocked(service.isKeyboardEditingBlocked())
        return true
    }

    private fun handleCommand(command: String) {
        if (!isCurrentPanel()) return
        when (command) {
            "h", "s", "p", "n", "z", "*" -> appendStroke(command)
            "segment" -> segmentStroke()
            "script" -> {
                service.toggleScriptPreference()
                if (strokeCode.isNotEmpty() || wordSegments.isNotEmpty()) renderCandidateRow(lastStrokeCandidates)
                else refreshAssociations()
            }
            "space" -> commitDirect(" ")
            "backspace" -> if (strokeNumberMode) service.deleteBackward() else deleteStroke()
            "enter" -> { abandonComposition(); service.sendEnterKey() }
            "clear" -> clearStrokes()
            "symbols" -> { strokeNumberMode = true; numberSymbolMode = true; symbolPageIndex = 0; refreshPanel() }
            "numbers" -> { strokeNumberMode = true; numberSymbolMode = false; symbolPageIndex = 0; refreshPanel() }
            "toggle_symbols" -> { numberSymbolMode = !numberSymbolMode; symbolPageIndex = 0; refreshPanel() }
            "prev" -> { symbolPageIndex = (symbolPageIndex - 1).mod(numberPanelSymbolPages.size); refreshPanel() }
            "next" -> { symbolPageIndex = (symbolPageIndex + 1).mod(numberPanelSymbolPages.size); refreshPanel() }
            "back" -> { strokeNumberMode = false; numberSymbolMode = false; symbolPageIndex = 0; refreshPanel() }
            "voice" -> {
                val startRecording = !strokeNumberMode
                strokeNumberMode = false
                numberSymbolMode = false
                symbolPageIndex = 0
                clearStrokes()
                service.inputMode = OpenLessImeService.InputMode.VOICE
                service.saveInputMode(service.inputMode)
                service.refreshInputView()
                if (startRecording && !service.recording) service.toggleDictation()
            }
        }
    }

    private fun appendStroke(stroke: String) {
        if (strokeCode.length >= 32) return
        if (strokeCode.isEmpty()) {
            phraseQueryEpoch++
            lastStrokeCandidates = emptyList()
            renderCandidateRow(emptyList())
        }
        strokeCode += stroke
        updateStrokePreview()
        refreshStrokeCandidates(strokeCode)
    }

    /** Marks the current character as a word segment without committing it yet. */
    private fun segmentStroke() {
        if (strokeCode.isEmpty()) return
        val candidate = lastStrokeCandidates.firstOrNull() ?: return
        wordSegments.add(candidate)
        strokeCode = ""
        strokeQueryEpoch++
        lastStrokeCandidates = emptyList()
        updateStrokePreview()
        renderCandidateRow(emptyList())
    }

    private fun updateStrokePreview() { refreshPanel() }

    private fun refreshStrokeCandidates(code: String) {
        val query = ++strokeQueryEpoch
        if (service.isSensitiveField(service.currentInputEditorInfo)) {
            renderCandidateRow(emptyList())
            return
        }
        strokeRepository.searchAsync(code) { result ->
            if (query != strokeQueryEpoch || service.inputMode != OpenLessImeService.InputMode.STROKE) return@searchAsync
            lastStrokeCandidates = result
            renderCandidateRow(result)
        }
    }

    /**
     * Renders the stroke candidate row: a leading "commit the whole word"
     * button for any segments marked via 分词 (if present), followed by the
     * single-character candidates for the character currently being typed.
     */
    private fun renderCandidateRow(strokeMatches: List<String>) {
        val entries = mutableListOf<CandidateSpec>()
        if (wordSegments.isNotEmpty()) {
            val word = wordSegments.joinToString("")
            entries += CandidateSpec(service.outputScript(word)) { commitWord(word) }
        }
        strokeMatches.forEach { word ->
            entries += CandidateSpec(service.outputScript(word)) { commitStrokeCandidate(word) }
        }
        populateCandidateRow(entries)
    }

    private data class CandidateSpec(val label: String, val action: () -> Unit)

    private fun populateCandidateRow(specs: List<CandidateSpec>) {
        candidateEntries = specs
        refreshPanel()
    }

    /** Commits the current character together with any segments already marked via 分词. */
    private fun commitStrokeCandidate(candidate: String) {
        val code = strokeCode
        val wasTop = candidate == lastStrokeCandidates.firstOrNull()
        if (commitWord((wordSegments + candidate).joinToString("")) &&
            service.personalizedLearningAllowed() && OpenLessAndroidPreferences.strokeUsageEnabled(service) && code.isNotEmpty()) {
            strokeRepository.recordPick(code, candidate, wasTop)
        }
    }

    private fun commitWord(word: String): Boolean {
        if (word.isEmpty() || service.isSensitiveField(service.currentInputEditorInfo)) return false
        val connection = service.currentInputConnection ?: return false
        val contextBeforeCommit = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        if (!connection.commitText(service.outputScript(word), 1)) return false
        if (service.personalizedLearningAllowed() && OpenLessAndroidPreferences.strokeUsageEnabled(service)) {
            phraseRepository.recordTypedContinuation(associationLearning.advance(contextBeforeCommit, word))
        } else {
            associationLearning.reset()
        }
        if (service.personalizedLearningAllowed()) confirmedText = (confirmedText + word).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
        return true
    }

    private fun refreshAssociations() {
        if (!service.personalizedLearningAllowed() || !OpenLessAndroidPreferences.strokeAssociationEnabled(service)) {
            populateCandidateRow(emptyList())
            return
        }
        val context = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        val query = ++phraseQueryEpoch
        if (context.isEmpty()) return
        phraseRepository.searchAsync(context) { result ->
            if (query != phraseQueryEpoch || service.inputMode != OpenLessImeService.InputMode.STROKE || confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT) != context) return@searchAsync
            val specs = result.map { candidate ->
                val matchedPrefix = candidate.matchedPrefix.ifEmpty { context }
                val displayText = service.outputScript(candidate.text)
                CandidateSpec(displayText) { commitAssociation(candidate.text, matchedPrefix) }
            }
            populateCandidateRow(specs)
        }
    }

    private fun commitAssociation(displayText: String, matchedContext: String) {
        if (service.isSensitiveField(service.currentInputEditorInfo) || !displayText.startsWith(matchedContext)) return
        val suffix = displayText.removePrefix(matchedContext)
        val connection = service.currentInputConnection ?: return
        if (suffix.isNotEmpty() && !connection.commitText(service.outputScript(suffix), 1)) return
        if (service.personalizedLearningAllowed() && OpenLessAndroidPreferences.strokeUsageEnabled(service)) {
            phraseRepository.recordUsage(matchedContext, displayText)
        }
        associationLearning.reset()
        confirmedText = (confirmedText + suffix).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
    }

    private fun deleteStroke() {
        associationLearning.reset()
        if (strokeCode.isNotEmpty()) {
            strokeCode = strokeCode.dropLast(1)
            strokeQueryEpoch++
            updateStrokePreview()
            if (strokeCode.isNotEmpty()) {
                appendStroke("")
            } else {
                lastStrokeCandidates = emptyList()
                renderCandidateRow(emptyList())
            }
        } else if (wordSegments.isNotEmpty()) {
            wordSegments.removeAt(wordSegments.lastIndex)
            updateStrokePreview()
            renderCandidateRow(emptyList())
        } else {
            service.deleteBackward()
            if (confirmedText.isNotEmpty()) {
                confirmedText = confirmedText.dropLast(1)
                phraseQueryEpoch++
                refreshAssociations()
            } else {
                populateCandidateRow(emptyList())
            }
        }
    }

    private fun clearStrokes() {
        strokeCode = ""
        strokeQueryEpoch++
        wordSegments.clear()
        lastStrokeCandidates = emptyList()
        candidateEntries = emptyList()
        refreshPanel()
    }

    /**
     * Something other than a candidate is about to reach the field (a mark,
     * digit, space, Return): drop the half-typed code, its segments and the
     * association context with them — what's on screen next no longer
     * follows the last committed word, and a stale code row would sit there
     * meaning nothing. Same as pinyin abandoning its encoding on punctuation.
     */
    private fun abandonComposition() {
        associationLearning.reset()
        clearStrokes()
        confirmedText = ""
        phraseQueryEpoch++
    }

    private fun commitDirect(text: String) {
        abandonComposition()
        service.currentInputConnection?.commitText(text, 1)
    }

    // internal, not private: RimePinyinController's own association context
    // buffer reuses the same cap for consistency (see phraseRepository's own
    // doc comment on why the two share one StrokePhraseRepository instance).
    internal companion object {
        const val MAX_ASSOCIATION_CONTEXT = 8
    }
}
