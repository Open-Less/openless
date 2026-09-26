package com.openless.app

import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView

/**
 * Everything specific to the 笔画 (stroke) input panel — encode entry, the
 * 字候选/联想候选 query+render+commit pipeline, and the number/symbol sub-panel
 * — split out of OpenLessImeService.kt, which otherwise hosted every panel's
 * build/query/render/commit logic in one class; this alone was a large,
 * mostly self-contained chunk of it (see the "OpenLessImeService.kt 拆分"
 * item this was pulled out of).
 *
 * Shared machinery — candidateItemView()/styleCandidateFirstState()
 * (English's own candidate row uses these too), showCandidateOverlay()/
 * candidateOverlayEntries, keyboardKey(), theme/layout helpers, and
 * InputConnection access — stays on [service] and is called back into
 * explicitly rather than duplicated here. This class owns exactly the
 * stroke-specific state the two old inline reset blocks
 * (selectInputMode()/onStartInput()) used to touch — see
 * resetForModeSwitch()/resetForNewInputSession(), which preserve each call
 * site's own exact field list rather than merging them into one generic
 * reset.
 */
internal class StrokeInputController(private val service: OpenLessImeService) {
    private val strokeRepository by lazy { StrokeInputRepository(service) }
    // internal, not private: Pinyin mode's own post-commit association
    // (LitePinyinController) reuses this exact instance rather than loading
    // a second copy of the same ~220k-entry phrase index — association data
    // is keyed by confirmed on-screen Chinese text, not by how it was
    // typed, so it's equally valid for either input mode.
    internal val phraseRepository by lazy { StrokePhraseRepository(service) }

    private var strokeCode = ""
    private var strokeQueryEpoch = 0L
    private val wordSegments = mutableListOf<String>()
    private var lastStrokeCandidates: List<String> = emptyList()
    private var confirmedText = ""
    private var phraseQueryEpoch = 0L
    private var strokePreview: TextView? = null
    private var clearStrokeButton: TextView? = null
    private var strokeCandidates: LinearLayout? = null

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

    /** Matches selectInputMode()'s own old inline reset exactly — mode-switch also resets the number/symbol sub-panel and the punctuation rail's page, which resetForNewInputSession() below does not. */
    fun resetForModeSwitch() {
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
        strokeNumberMode = false
        numberSymbolMode = false
        symbolPageIndex = 0
        confirmedText = ""
        phraseQueryEpoch++
        strokeCode = ""
        wordSegments.clear()
        lastStrokeCandidates = emptyList()
        updateClearStrokeButtonVisibility()
    }

    fun buildStrokeView(): View {
        // Matches the punctuation rail's own 0.16f width share below (body's
        // "0.16f/0.65f/0.19f" split) so a downward drag anywhere on the rail
        // is excluded from swipe-to-dismiss and left entirely to SwipeRail.
        val root = OpenLessImeService.SwipeModeContainer(service, verticalDismissExclusionRatio = 0.16f) { direction -> service.swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, service.keyboardPanelHeightPx())
            minimumHeight = service.keyboardPanelHeightPx()
            setPadding(service.dp(4), service.dp(3), service.dp(4), service.dp(3))
            setBackgroundColor(service.tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
        }
        val header = LinearLayout(service).apply { gravity = Gravity.CENTER_VERTICAL }
        header.addView(service.buildBrandView(), LinearLayout.LayoutParams(0, service.dp(38), 1f))
        header.addView(service.buildModeToggle(), LinearLayout.LayoutParams(service.dp(240), service.dp(38)))
        // Stroke mode's root padding is much tighter (4dp/3dp) to fit its dense
        // grid. Compensate with margins so the header/toggle still land at the
        // same canonical 16dp/8dp inset as every other panel.
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, service.dp(38)).apply {
            marginStart = service.dp(12)
            marginEnd = service.dp(12)
            topMargin = service.dp(5)
        })

        // Stroke mode follows the reference layout: a compact stroke row,
        // candidate row, punctuation column, stroke grid, and action rail.
        // Encode + candidate rows are fixed-height (24dp + 36dp = 60dp, same
        // total as before this pass) and never resize with content — only
        // the candidate list scrolls horizontally — so the stroke keys below
        // never move. Both rows share one rounded background (an existing
        // panel color, not a new one) so they read as a single continuous
        // strip rather than two separate cards.
        val top = LinearLayout(service).apply {
            orientation = LinearLayout.VERTICAL
            background = service.buildEncodeAreaBackground()
        }
        val strokeRow = LinearLayout(service).apply { gravity = Gravity.CENTER_VERTICAL }
        strokePreview = TextView(service).apply {
            text = ""
            textSize = 16.5f
            setTextColor(service.strokeEncodeAccentColor)
            gravity = Gravity.CENTER_VERTICAL
            setSingleLine(true)
            setPadding(service.dp(10), 0, 0, 0)
        }
        strokeRow.addView(strokePreview, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))
        // Clear-code button: a 40x30dp hit target with a small glyph, not a
        // heavy independent button — tapping it is the same clearStrokes()
        // already wired to the action rail's "清除" key.
        clearStrokeButton = TextView(service).apply {
            text = "✕"
            textSize = 13f
            gravity = Gravity.CENTER
            setTextColor(service.tone(Color.rgb(150, 150, 150), Color.rgb(130, 130, 135)))
            contentDescription = service.ui("清除笔画编码", "Clear stroke code")
            setOnClickListener { clearStrokes() }
        }
        strokeRow.addView(clearStrokeButton, LinearLayout.LayoutParams(service.dp(40), ViewGroup.LayoutParams.MATCH_PARENT))
        updateClearStrokeButtonVisibility()
        top.addView(strokeRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, service.dp(24)))

        val candidateRow = LinearLayout(service).apply { gravity = Gravity.CENTER_VERTICAL }
        // A plain setOnTouchListener on the ScrollView never actually fires
        // here: each candidate is its own clickable keyboardKey() view, so
        // it claims ACTION_DOWN before the ScrollView's own onTouchEvent
        // ever runs. Overriding onInterceptTouchEvent instead runs at the
        // right point in the dispatch chain — before any child gets a
        // chance to claim the touch — so it reliably blocks
        // SwipeModeContainer's mode-switch gesture from stealing a drag
        // that starts on top of a candidate button.
        val candidatesScroll = object : android.widget.HorizontalScrollView(service) {
            override fun onInterceptTouchEvent(ev: android.view.MotionEvent): Boolean {
                if (ev.actionMasked == android.view.MotionEvent.ACTION_DOWN) {
                    parent?.requestDisallowInterceptTouchEvent(true)
                }
                return super.onInterceptTouchEvent(ev)
            }
        }.apply {
            isHorizontalScrollBarEnabled = false
            isFillViewport = false
            overScrollMode = View.OVER_SCROLL_NEVER
            strokeCandidates = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
            strokeCandidates?.orientation = LinearLayout.HORIZONTAL
            addView(strokeCandidates, ViewGroup.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        candidateRow.addView(candidatesScroll, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f))
        // "Show more" — opens the full candidate/association list in a
        // floating overlay instead of growing this row or the panel height.
        val expandCandidatesButton = OpenLessImeService.StrokeActionView(
            service,
            "triangle-down",
            iconColor = service.tone(Color.rgb(180, 180, 180), Color.rgb(130, 130, 135)),
        ).apply {
            contentDescription = service.ui("展开更多候选", "Show more candidates")
        }
        expandCandidatesButton.visibility = View.GONE
        expandCandidatesButton.setOnClickListener { service.showCandidateOverlay(expandCandidatesButton) }
        candidateRow.addView(expandCandidatesButton, LinearLayout.LayoutParams(service.dp(28), ViewGroup.LayoutParams.MATCH_PARENT))
        // Only shown once the candidates actually overflow the visible
        // scroll width — otherwise it sat there whether or not there was
        // anything more to show, which read as an odd stray control.
        // renderCandidateRow()/refreshAssociations() both just repopulate
        // strokeCandidates and let layout happen, so a global layout
        // listener (fires after every layout pass, including the one
        // triggered by add/removeAllViews) is what re-checks this instead
        // of hooking every candidate-population call site individually.
        candidatesScroll.viewTreeObserver.addOnGlobalLayoutListener {
            val candidates = strokeCandidates
            expandCandidatesButton.visibility =
                if (candidates != null && candidates.width > candidatesScroll.width) View.VISIBLE else View.GONE
        }
        top.addView(candidateRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, service.dp(36)))
        root.addView(top, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, service.dp(60)))

        val body = LinearLayout(service).apply { gravity = Gravity.CENTER }
        // Swiping the rail up/down cycles through punctuationGroups instead of
        // scrolling — one swipe always advances exactly one group.
        val punctuation = OpenLessImeService.SwipeRail(service) { direction ->
            val count = punctuationGroups.size
            punctuationGroupIndex = ((punctuationGroupIndex + direction) % count + count) % count
            service.refreshInputView()
        }.apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            // Zero vertical padding so the rail's own top/bottom edges land
            // exactly on the grid/actions columns' top/bottom edges (all
            // three share the same MATCH_PARENT body height) — horizontal
            // padding is kept since it only insets key width, not row
            // position.
            setPadding(service.dp(2), 0, service.dp(2), 0)
            background = service.roundedButton(service.tone(Color.rgb(45, 45, 45), Color.rgb(230, 230, 234)), service.dp(4))
        }
        punctuationGroups[punctuationGroupIndex].forEachIndexed { index, mark ->
            punctuation.addView(service.keyboardKey(mark, 1f, action = { service.currentInputConnection?.commitText(mark, 1) }).apply {
                textSize = 18f
                // The rail is one connected key surface; separators provide the only visual split.
                background = GradientDrawable().apply { setColor(Color.TRANSPARENT) }
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)
            })
            if (index < 4) {
                punctuation.addView(View(service).apply {
                    setBackgroundColor(service.tone(Color.rgb(28, 28, 28), Color.rgb(205, 205, 210)))
                }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, service.dp(1)))
            }
        }
        // Match the reference proportions: both side rails occupy the same share of the panel.
        body.addView(punctuation, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 0.16f))

        val grid = LinearLayout(service).apply { orientation = LinearLayout.VERTICAL; gravity = Gravity.CENTER }
        val strokeRows = listOf(
            listOf("1\n一" to "h", "2\n丨" to "s", "3\n丿" to "p"),
            listOf("4\n丶" to "n", "5\n乙" to "z", "6\n通配" to "*"),
            listOf("7\n分词" to " ", "8\n：" to ":", "9\n；" to ";"),
            listOf("繁" to "script", "🎙" to "voice", "符号" to "symbols"),
        )
        strokeRows.forEach { rowItems ->
            val row = LinearLayout(service).apply { gravity = Gravity.CENTER }
            rowItems.forEach { (label, code) ->
                val key = if (code == "script") service.keyboardKey(label, 1f, action = {
                    service.toggleScriptPreference()
                }, graphicCode = "script").apply {
                    if (service.traditionalOutput) {
                        background = service.roundedButton(service.tone(Color.rgb(112, 78, 92), Color.rgb(232, 205, 213)), service.dp(5))
                    }
                } else if (code == "voice") service.keyboardKey("0", 1f, action = {
                    service.currentInputConnection?.commitText(" ", 1)
                }, swipeUpAction = {
                    service.currentInputConnection?.commitText("0", 1)
                }, swipePreview = "0", microphoneIcon = true, longPressDelayMs = 900L, longPressAction = {
                    service.inputMode = OpenLessImeService.InputMode.VOICE
                    service.saveInputMode(service.inputMode)
                    clearStrokes()
                    service.refreshInputView()
                    if (!service.recording) service.toggleDictation()
                }) else {
                    val swipeDigit = label.substringBefore("\n").takeIf { it.length == 1 && it[0].isDigit() }
                    service.keyboardKey(label, 1f, action = {
                    when (code) {
                        "symbols" -> {
                            strokeNumberMode = true
                            numberSymbolMode = true
                            symbolPageIndex = 0
                            service.refreshInputView()
                        }
                        " " -> segmentStroke()
                        else -> if (code in listOf("h", "s", "p", "n", "z", "*")) appendStroke(code) else service.currentInputConnection?.commitText(code, 1)
                    }
                    }, swipeUpAction = swipeDigit?.let { digit ->
                        { service.currentInputConnection?.commitText(digit, 1) }
                    }, swipePreview = swipeDigit, strokeIconCode = code.takeIf {
                        it in listOf("h", "s", "p", "n", "z")
                    }, graphicCode = code.takeIf {
                        it in listOf("*", ":", ";", " ", "symbols")
                    })
                }
                key.textSize = if (code == "voice") 10f else 17f
                // dp(1) on every side gives a uniform ~2dp gap both ways
                // (keys stay clearly separated) and puts as much of the
                // reclaimed margin as possible into visible key size, not
                // new padding. The action rail's vertical margin is kept at
                // the same dp(1)/dp(1) below so row top/bottom edges still
                // land exactly together across both columns.
                key.layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f).apply {
                    setMargins(service.dp(1), service.dp(1), service.dp(1), service.dp(1))
                }
                row.addView(key)
            }
            grid.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        body.addView(grid, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 0.65f))

        val actions = LinearLayout(service).apply { orientation = LinearLayout.VERTICAL; gravity = Gravity.CENTER }
        listOf("←" to { deleteStroke() }, "↵" to { service.sendEnterKey() }, "清除" to { clearStrokes() }, "123" to {
            strokeNumberMode = true
            numberSymbolMode = false
            symbolPageIndex = 0
            service.refreshInputView()
        }).forEach { (label, action) ->
            actions.addView(service.keyboardKey(label, 1f, action, repeatOnLongPress = label == "←", repeatAction = action,
                graphicActionCode = label).apply {
                textSize = if (label == "←" || label == "↵") 30f else 17f
                // Vertical margin matches the grid keys' dp(1)/dp(1) exactly
                // so every row's top/bottom edge lines up across both
                // columns; horizontal margin is independent (single column).
                layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
                    setMargins(service.dp(1), service.dp(1), service.dp(1), service.dp(1))
                }
                background = service.roundedButton(Color.rgb(153, 26, 40), service.dp(5))
            })
        }
        body.addView(actions, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 0.19f))
        root.addView(body, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        // Rebuilds caused by switching back from the numeric panel must restore
        // both the visible code and its candidates from the retained buffer.
        if (strokeCode.isNotEmpty()) {
            strokePreview?.text = service.displayStrokeCode(strokeCode)
            refreshStrokeCandidates(strokeCode)
        }
        return root
    }

    fun buildStrokeNumberView(): View {
        service.refreshLanguage()
        val root = OpenLessImeService.SwipeModeContainer(service) { direction -> service.swipeInputMode(direction) }.apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, service.keyboardPanelHeightPx())
            minimumHeight = service.keyboardPanelHeightPx()
            setPadding(service.dp(8), service.dp(8), service.dp(8), service.dp(8))
            setBackgroundColor(service.tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
        }
        val header = LinearLayout(service).apply { gravity = Gravity.CENTER_VERTICAL }
        header.addView(service.buildBrandView().apply { setPadding(service.dp(8), 0, 0, 0) }, LinearLayout.LayoutParams(0, service.dp(38), 1f))
        header.addView(service.buildModeToggle(), LinearLayout.LayoutParams(service.dp(240), service.dp(38)))
        // This panel's own root padding (8dp) is narrower than the voice panel's
        // (16dp). Compensate with margins so the header/toggle still land at the
        // same canonical 16dp/8dp inset as every other panel.
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, service.dp(38)).apply {
            marginStart = service.dp(8)
            marginEnd = service.dp(8)
        })

        // Backspace/Enter/Voice/Return/Symbols stay in Chinese regardless of UI
        // language: they're functional keys on the number/symbol panel, not
        // content the user is composing.
        val rows = if (numberSymbolMode) {
            val page = numberPanelSymbolPages[symbolPageIndex.coerceIn(numberPanelSymbolPages.indices)]
            listOf(
                page.subList(0, 4) + "退格",
                page.subList(4, 8) + "回车",
                page.subList(8, 12) + "语音",
                listOf("▲", "${symbolPageIndex + 1}/${numberPanelSymbolPages.size}", "▼", "数字", "返回"),
            )
        } else {
            listOf(
                listOf("@", "1", "2", "3", "退格"),
                listOf(":", "4", "5", "6", "回车"),
                listOf(",", "7", "8", "9", "语音"),
                // "0" sits directly under "8", flanked by +/. — the same
                // layout convention as a phone dial pad's "* 0 #" row.
                listOf("+", "0", ".", "符号", "返回"),
            )
        }
        val body = LinearLayout(service).apply { orientation = LinearLayout.VERTICAL }
        rows.forEachIndexed { rowIndex, rowItems ->
            val row = LinearLayout(service).apply { gravity = Gravity.CENTER }
            rowItems.forEachIndexed { index, label ->
                val isAction = index == rowItems.lastIndex
                val action: () -> Unit = when {
                    rowIndex == 0 && isAction -> ({ service.deleteBackward() })
                    rowIndex == 1 && isAction -> ({ service.sendEnterKey() })
                    rowIndex == 2 && isAction -> ({
                        service.inputMode = OpenLessImeService.InputMode.VOICE
                        service.saveInputMode(service.inputMode)
                        strokeNumberMode = false
                        numberSymbolMode = false
                        symbolPageIndex = 0
                        service.refreshInputView()
                    })
                    // Symbol-page navigation only replaces row 3's first three
                    // cells (previously "+ - .") while in symbol mode; the
                    // middle cell is just a page indicator, not clickable.
                    rowIndex == 3 && index == 0 && numberSymbolMode -> ({
                        symbolPageIndex = (symbolPageIndex - 1 + numberPanelSymbolPages.size) % numberPanelSymbolPages.size
                        service.refreshInputView()
                    })
                    rowIndex == 3 && index == 1 && numberSymbolMode -> ({})
                    rowIndex == 3 && index == 2 && numberSymbolMode -> ({
                        symbolPageIndex = (symbolPageIndex + 1) % numberPanelSymbolPages.size
                        service.refreshInputView()
                    })
                    rowIndex == 3 && index == 3 -> ({
                        numberSymbolMode = !numberSymbolMode
                        symbolPageIndex = 0
                        service.refreshInputView()
                    })
                    rowIndex == 3 && isAction -> ({
                        strokeNumberMode = false
                        numberSymbolMode = false
                        symbolPageIndex = 0
                        service.refreshInputView()
                    })
                    else -> ({ service.currentInputConnection?.commitText(label, 1) })
                }
                row.addView(service.keyboardKey(label, 1f, action, repeatOnLongPress = rowIndex == 0 && isAction, repeatAction = action).apply {
                    textSize = if (isAction) 15f else 20f
                    if (isAction) background = service.roundedButton(Color.rgb(153, 26, 40), service.dp(7))
                })
            }
            body.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        root.addView(body, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        return root
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

    private fun updateStrokePreview() {
        // Plain "+" concatenation on a CharSequence would call toString() and
        // drop the ImageSpan the 5th stroke relies on — TextUtils.concat()
        // preserves spans across both pieces.
        strokePreview?.text = android.text.TextUtils.concat(wordSegments.joinToString(""), service.displayStrokeCode(strokeCode))
        updateClearStrokeButtonVisibility()
    }

    /** Only shows the encode row's "✕" once there's actually something to clear — otherwise it just sat there doing nothing. */
    private fun updateClearStrokeButtonVisibility() {
        clearStrokeButton?.visibility = if (strokeCode.isEmpty() && wordSegments.isEmpty()) View.GONE else View.VISIBLE
    }

    private fun refreshStrokeCandidates(code: String) {
        val query = ++strokeQueryEpoch
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
        val specs = mutableListOf<CandidateSpec>()
        // The very first candidate shown — whichever one that is — is
        // highlighted in the same red as the right-hand action rail, since
        // it's what a bare space/enter would commit.
        var firstCandidate = true
        if (wordSegments.isNotEmpty()) {
            val word = wordSegments.joinToString("")
            val displayWord = service.outputScript(word)
            // Matches candidateItemView()'s own dp(11)-per-side padding
            // (2*11=22) plus ~22dp per glyph at its 20sp text size.
            val wordWidth = service.dp((displayWord.codePointCount(0, displayWord.length) * 22 + 22).coerceAtLeast(50))
            specs.add(CandidateSpec(displayWord, firstCandidate, wordWidth) { commitWord(word) })
            firstCandidate = false
        }
        strokeMatches.forEach { candidate ->
            val displayCandidate = service.outputScript(candidate)
            // Was dp(38); briefly widened to dp(46) alongside
            // candidateItemView()'s own now-reverted padding/text-size
            // experiment (see that function's comment) — settled on dp(45).
            specs.add(CandidateSpec(displayCandidate, firstCandidate, service.dp(45)) { commitStrokeCandidate(candidate) })
            firstCandidate = false
        }
        populateCandidateRow(specs)
        service.candidateOverlayEntries = specs.map { it.label to it.action }
    }

    /** One candidate slot's content, independent of whatever View (if any) ends up showing it — see populateCandidateRow(). */
    private data class CandidateSpec(val label: String, val isFirst: Boolean, val widthPx: Int, val action: () -> Unit)

    /**
     * Repopulates strokeCandidates with [specs] by reusing existing child
     * views in place — retexting/rewidthing/rebinding the click target of
     * whatever's already sitting at each index — instead of this row's old
     * removeAllViews()-then-addView()-every-candidate approach. Rebuilding
     * up to MAX_CANDIDATES real keyboardKey()-backed Views (a background
     * drawable allocated then immediately discarded, plus a full touch-
     * listener closure with long-press/swipe-retarget plumbing this row
     * never uses) on literally every keystroke is the actual source of
     * candidate-row lag on a slow device, not the async dictionary lookup
     * feeding it — this call site is on the hot path (every stroke and
     * every phrase-association refresh), so only the surplus or shortfall
     * between the previous and new candidate count now creates or removes
     * a View at all.
     */
    private fun populateCandidateRow(specs: List<CandidateSpec>) {
        val row = strokeCandidates ?: return
        specs.forEachIndexed { index, spec ->
            when (val existing = row.getChildAt(index)) {
                is TextView -> {
                    existing.text = spec.label
                    existing.setOnClickListener { spec.action() }
                    service.styleCandidateFirstState(existing, spec.isFirst)
                    val params = existing.layoutParams as LinearLayout.LayoutParams
                    if (params.width != spec.widthPx) {
                        params.width = spec.widthPx
                        existing.layoutParams = params
                    }
                }
                else -> row.addView(
                    service.candidateItemView(spec.label, spec.isFirst, action = spec.action),
                    LinearLayout.LayoutParams(spec.widthPx, ViewGroup.LayoutParams.MATCH_PARENT),
                )
            }
        }
        while (row.childCount > specs.size) {
            row.removeViewAt(row.childCount - 1)
        }
    }

    /** Commits the current character together with any segments already marked via 分词. */
    private fun commitStrokeCandidate(candidate: String) {
        // Picking anything other than the top-ranked result is a correction
        // — learn it, so this code favors `candidate` from now on. Picking
        // the top result needs no recording: it's already where it should be.
        if (OpenLessAndroidPreferences.strokeUsageEnabled(service) &&
            strokeCode.isNotEmpty() && candidate != lastStrokeCandidates.firstOrNull()
        ) {
            strokeRepository.recordPersonalPick(strokeCode, candidate)
        }
        commitWord((wordSegments + candidate).joinToString(""))
    }

    private fun commitWord(word: String) {
        if (word.isEmpty() || service.isSensitiveField(service.currentInputEditorInfo)) return
        val connection = service.currentInputConnection ?: return
        val contextBeforeCommit = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        if (!connection.commitText(service.outputScript(word), 1)) return
        if (OpenLessAndroidPreferences.strokeUsageEnabled(service)) {
            phraseRepository.recordUsage(contextBeforeCommit, word)
        }
        confirmedText = (confirmedText + word).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
    }

    private fun refreshAssociations() {
        if (!OpenLessAndroidPreferences.strokeAssociationEnabled(service)) {
            populateCandidateRow(emptyList())
            service.candidateOverlayEntries = emptyList()
            return
        }
        val context = confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT)
        val query = ++phraseQueryEpoch
        if (context.isEmpty()) return
        phraseRepository.searchAsync(context) { result ->
            if (query != phraseQueryEpoch || service.inputMode != OpenLessImeService.InputMode.STROKE || confirmedText.takeLast(MAX_ASSOCIATION_CONTEXT) != context) return@searchAsync
            val specs = result.mapIndexed { index, candidate ->
                val matchedPrefix = candidate.matchedPrefix.ifEmpty { context }
                val displayText = service.outputScript(candidate.text)
                val candidateWidth = service.dp((displayText.codePointCount(0, displayText.length) * 22 + 16).coerceAtLeast(46))
                CandidateSpec(displayText, index == 0, candidateWidth) { commitAssociation(candidate.text, matchedPrefix) }
            }
            populateCandidateRow(specs)
            service.candidateOverlayEntries = specs.map { it.label to it.action }
        }
    }

    private fun commitAssociation(displayText: String, matchedContext: String) {
        if (service.isSensitiveField(service.currentInputEditorInfo) || !displayText.startsWith(matchedContext)) return
        val suffix = displayText.removePrefix(matchedContext)
        val connection = service.currentInputConnection ?: return
        if (suffix.isNotEmpty() && !connection.commitText(service.outputScript(suffix), 1)) return
        if (OpenLessAndroidPreferences.strokeUsageEnabled(service)) {
            phraseRepository.recordUsage(matchedContext, displayText)
        }
        confirmedText = (confirmedText + suffix).takeLast(MAX_ASSOCIATION_CONTEXT)
        clearStrokes()
        refreshAssociations()
    }

    private fun deleteStroke() {
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
                strokeCandidates?.removeAllViews()
            }
        }
    }

    private fun clearStrokes() {
        strokeCode = ""
        strokeQueryEpoch++
        wordSegments.clear()
        lastStrokeCandidates = emptyList()
        strokePreview?.text = ""
        strokeCandidates?.removeAllViews()
        updateClearStrokeButtonVisibility()
    }

    // internal, not private: LitePinyinController's own association context
    // buffer reuses the same cap for consistency (see phraseRepository's own
    // doc comment on why the two share one StrokePhraseRepository instance).
    internal companion object {
        const val MAX_ASSOCIATION_CONTEXT = 8
    }
}
