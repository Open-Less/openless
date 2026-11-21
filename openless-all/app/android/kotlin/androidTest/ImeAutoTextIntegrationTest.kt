package com.openless.app

import android.content.ContextWrapper
import android.text.Editable
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.ExtractedText
import android.view.inputmethod.ExtractedTextRequest
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputConnectionWrapper
import android.widget.EditText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class ImeAutoTextIntegrationTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    @Test fun candidateSpaceDoesNotFollowCaretMoves() = instrumentation.runOnMainSync {
        val (service, edit) = editor("old new ")
        armCandidateSpace(service)
        edit.setSelection(4)
        service.onUpdateSelection(8, 8, 4, 4, -1, -1)
        call(service, "commitEnglishChar", ",")
        val changedUnrelatedSpace = edit.text.toString() == "old, new "
        probe("P1", "H1", changedUnrelatedSpace)
        assertFalse(changedUnrelatedSpace)
        assertEquals("old ,new ", edit.text.toString())
    }

    @Test fun doubleSpaceDoesNotFollowCaretMoves() = instrumentation.runOnMainSync {
        val (service, edit) = editor("one two ")
        edit.setSelection(3)
        call(service, "commitEnglishSpace")
        edit.setSelection(edit.length())
        service.onUpdateSelection(4, 4, 9, 9, -1, -1)
        call(service, "commitEnglishSpace")
        val changedUnrelatedSpace = edit.text.toString().endsWith("two. ")
        probe("P2", "H2", changedUnrelatedSpace)
        assertFalse(changedUnrelatedSpace)
        assertEquals("one  two  ", edit.text.toString())
    }

    @Test fun splitAssociationLearningRetainsTheOriginalPrefix() {
        val lookup: (String) -> List<StrokePhraseRepository.Candidate> = { prefix ->
            listOf("天气", "天气预报").filter { it.startsWith(prefix) }
                .map { StrokePhraseRepository.Candidate(it, 1, prefix) }
        }
        val history = TypedContinuationHistory()
        assertEquals(emptyList<StrokePhraseRepository.Candidate>(), history.advance("天气", "预").mapNotNull {
            typedContinuation(it.contextBefore, it.word, lookup)
        })
        val completed = history.advance("天气预", "报").mapNotNull {
            typedContinuation(it.contextBefore, it.word, lookup)
        }
        val missesOriginalPrefix = completed.none { it.matchedPrefix == "天气" }
        probe("P3", "H3", missesOriginalPrefix)
        assertFalse(missesOriginalPrefix)
        assertEquals(listOf("天气", "天气预"), completed.map { it.matchedPrefix })
    }

    @Test fun candidatePunctuationAndOwnSelectionUpdatesStillWork() = instrumentation.runOnMainSync {
        val (service, edit) = editor("word ")
        armCandidateSpace(service)
        service.onUpdateSelection(4, 4, 5, 5, -1, -1)
        call(service, "commitEnglishChar", "?")
        service.onUpdateSelection(5, 5, 6, 6, -1, -1)
        call(service, "commitEnglishChar", "!")
        assertEquals("word?! ", edit.text.toString())
    }

    @Test fun consecutiveSpacesStillMakeOneFullStop() = instrumentation.runOnMainSync {
        val (service, edit) = editor("word")
        call(service, "commitEnglishSpace")
        service.onUpdateSelection(4, 4, 5, 5, -1, -1)
        call(service, "commitEnglishSpace")
        assertEquals("word. ", edit.text.toString())
        call(service, "commitEnglishSpace")
        assertEquals("word.  ", edit.text.toString())
    }

    @Test fun selectedTextAndUnknownPositionsDoNotDeleteNeighboringSpaces() = instrumentation.runOnMainSync {
        val (service, edit) = editor("a word ")
        armCandidateSpace(service)
        edit.setSelection(2, 6)
        call(service, "commitEnglishChar", ",")
        assertEquals("a , ", edit.text.toString())
        val (unknownService, unknownEdit) = editor("word ", reportsPosition = false)
        armCandidateSpace(unknownService)
        call(unknownService, "commitEnglishChar", ",")
        assertEquals("word ,", unknownEdit.text.toString())
        call(unknownService, "commitEnglishSpace")
        call(unknownService, "commitEnglishSpace")
        assertEquals("word ,  ", unknownEdit.text.toString())
    }

    @Test fun replacingTheInputConnectionDropsTheOldCandidateSpace() = instrumentation.runOnMainSync {
        val (service, edit) = editor("word ")
        armCandidateSpace(service)
        setField(service, "draftConnection", connection(edit, true))
        call(service, "commitEnglishChar", ",")
        assertEquals("word ,", edit.text.toString())
    }

    @Test fun pinyinEditorBackspaceDropsDeletedAssociationContext() {
        lateinit var controller: RimePinyinController
        var controllerCreated = false
        lateinit var edit: EditText
        val deleted = CountDownLatch(1)
        instrumentation.targetContext.getSharedPreferences("openless_ime_ui", android.content.Context.MODE_PRIVATE)
            .edit().putBoolean("public_dictionary_updates", false).commit()
        OpenLessRime.initialize(instrumentation.targetContext)
        try {
            instrumentation.runOnMainSync {
                val (service, field) = editor("天气预")
                edit = field
                setField(service, "latinInputMode", OpenLessImeService.LatinInputMode.PINYIN)
                controller = OpenLessImeService::class.java.getDeclaredMethod("getRimePinyinController")
                    .apply { isAccessible = true }.invoke(service) as RimePinyinController
                controllerCreated = true
                controller.setLearningAllowed(true)
                controller.recordCommittedText("天气")
                controller.associationLearning.advance("天气", "预")
                controller.recordCommittedText("预")
                setField(service, "draftConnection", object : InputConnectionWrapper(connection(edit, true), false) {
                    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                        val result = super.deleteSurroundingText(beforeLength, afterLength)
                        deleted.countDown()
                        return result
                    }
                })
                call(service, "englishDeleteBackward")
            }
            assertTrue("Empty-encoding Backspace must reach the editor", deleted.await(20, TimeUnit.SECONDS))
            instrumentation.runOnMainSync {
                assertEquals("天气", edit.text.toString())
                assertEquals("", controller.associationContext())
                val lookup: (String) -> List<StrokePhraseRepository.Candidate> = { prefix ->
                    listOf("天气预报").filter { it.startsWith(prefix) }
                        .map { StrokePhraseRepository.Candidate(it, 1, prefix) }
                }
                val learned = controller.associationLearning.advance(controller.associationContext(), "报")
                    .mapNotNull { typedContinuation(it.contextBefore, it.word, lookup) }
                assertEquals(emptyList<StrokePhraseRepository.Candidate>(), learned)
            }
        } finally {
            instrumentation.runOnMainSync { if (controllerCreated) controller.shutdown() }
        }
    }

    private fun editor(text: String, reportsPosition: Boolean = true): Pair<OpenLessImeService, EditText> {
        val service = OpenLessImeService()
        ContextWrapper::class.java.getDeclaredMethod("attachBaseContext", android.content.Context::class.java)
            .apply { isAccessible = true }.invoke(service, instrumentation.targetContext)
        service.inputMode = OpenLessImeService.InputMode.ENGLISH
        val edit = EditText(instrumentation.targetContext).apply {
            setText(text)
            setSelection(length())
        }
        setField(service, "draftEdit", edit)
        setField(service, "draftConnection", connection(edit, reportsPosition))
        return service to edit
    }

    private fun connection(edit: EditText, reportsPosition: Boolean): InputConnection = object : BaseInputConnection(edit, true) {
        override fun getEditable(): Editable = edit.text
        override fun getExtractedText(request: ExtractedTextRequest?, flags: Int): ExtractedText? {
            if (!reportsPosition) return null
            return ExtractedText().apply {
                text = edit.text.toString()
                startOffset = 0
                selectionStart = edit.selectionStart
                selectionEnd = edit.selectionEnd
            }
        }
    }

    private fun armCandidateSpace(service: OpenLessImeService) {
        val anchor = OpenLessImeService::class.java.getDeclaredMethod("englishEditAnchor", InputConnection::class.java)
            .apply { isAccessible = true }.invoke(service, service.currentInputConnection)
        setField(service, "englishAutoSpace", anchor)
    }

    private fun setField(service: OpenLessImeService, name: String, value: Any?) {
        OpenLessImeService::class.java.getDeclaredField(name).apply { isAccessible = true }.set(service, value)
    }

    private fun call(service: OpenLessImeService, name: String, vararg args: String) {
        OpenLessImeService::class.java.getDeclaredMethod(name, *args.map { String::class.java }.toTypedArray())
            .apply { isAccessible = true }.invoke(service, *args)
    }

    private fun probe(id: String, hypothesis: String, observed: Boolean) {
        val record = JSONObject().apply {
            put("schema", "diagnosing-bugs.v1")
            put("session_id", "a5809584")
            put("run_id", "ime1230-fixed")
            put("probe_id", id)
            put("hypothesis_ids", org.json.JSONArray(listOf(hypothesis)))
            put("event", "regression_boundary")
            put("outcome", if (observed) "bug_observed" else "bug_absent")
            put("safe", JSONObject().put("matched", observed))
        }
        android.util.Log.i("Ime1230Evidence", record.toString())
        synchronized(ImeAutoTextIntegrationTest::class.java) {
            java.io.File(instrumentation.targetContext.filesDir, "ime1230-a5809584.ndjson")
                .appendText("$record\n")
        }
    }
}
