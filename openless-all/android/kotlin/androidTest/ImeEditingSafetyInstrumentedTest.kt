package com.openless.app

import android.content.Context
import android.content.ContextWrapper
import android.view.KeyEvent
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.ExtractedText
import android.view.inputmethod.ExtractedTextRequest
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputConnectionWrapper
import android.widget.EditText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Only fake editor connections; no service onCreate, Native, microphone or user text. */
@RunWith(AndroidJUnit4::class)
class ImeEditingSafetyInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private fun onMain(block: () -> Unit) = instrumentation.runOnMainSync(block)
    private fun set(service: OpenLessImeService, name: String, value: Any?) {
        OpenLessImeService::class.java.getDeclaredField(name).apply { isAccessible = true }.set(service, value)
    }
    private fun service(edit: EditText, connection: InputConnection): OpenLessImeService {
        val service = OpenLessImeService()
        ContextWrapper::class.java.getDeclaredMethod("attachBaseContext", Context::class.java)
            .apply { isAccessible = true }.invoke(service, context)
        set(service, "draftEdit", edit)
        set(service, "draftConnection", connection)
        return service
    }
    private fun base(edit: EditText) = object : BaseInputConnection(edit, true) {
        override fun getEditable() = edit.text
    }

    @Test fun backspaceDeletesAnEntireSupplementaryCodepoint() {
        var text = ""
        onMain {
            val edit = EditText(context).apply { setText("a😀"); setSelection(length()) }
            service(edit, base(edit)).deleteBackward()
            text = edit.text.toString()
        }
        assertEquals("a", text)
    }

    @Test fun backspacePreservesSelectionReplacementSemantics() {
        var text = ""
        onMain {
            val edit = EditText(context).apply { setText("abc"); setSelection(1, 2) }
            service(edit, base(edit)).deleteBackward()
            text = edit.text.toString()
        }
        assertEquals("ac", text)
    }

    @Test fun unsupportedCodepointDeletionFallsBackToCapturedConnectionKeyEvents() {
        val events = mutableListOf<Pair<Int, Int>>()
        onMain {
            val edit = EditText(context).apply { setText("a"); setSelection(1) }
            val connection = object : InputConnectionWrapper(base(edit), false) {
                override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int) = false
                override fun sendKeyEvent(event: KeyEvent): Boolean {
                    events += event.action to event.keyCode
                    return true
                }
            }
            service(edit, connection).deleteBackward()
        }
        assertEquals(listOf(KeyEvent.ACTION_DOWN to KeyEvent.KEYCODE_DEL, KeyEvent.ACTION_UP to KeyEvent.KEYCODE_DEL), events)
    }

    @Test fun rangeSelectionAddsPartialOffsetAndDoesNotSplitEmoji() {
        val selections = mutableListOf<Pair<Int, Int>>()
        onMain {
            val edit = EditText(context)
            val connection = object : InputConnectionWrapper(base(edit), false) {
                override fun getExtractedText(request: ExtractedTextRequest?, flags: Int) = ExtractedText().apply {
                    text = "a😀b"
                    startOffset = 10
                    selectionStart = 1
                    selectionEnd = 1
                }
                override fun setSelection(start: Int, end: Int): Boolean {
                    selections += start to end
                    return true
                }
            }
            val service = service(edit, connection)
            set(service, "clipboardSelectionMode", true)
            OpenLessImeService::class.java.getDeclaredMethod("sendClipboardCursorKey", Int::class.javaPrimitiveType)
                .apply { isAccessible = true }.invoke(service, KeyEvent.KEYCODE_DPAD_RIGHT)
        }
        assertEquals(listOf(11 to 13), selections)
    }

    @Test fun unknownExtractedTextDoesNotSelectFromGuessedZero() {
        val selections = mutableListOf<Pair<Int, Int>>()
        onMain {
            val edit = EditText(context)
            val connection = object : InputConnectionWrapper(base(edit), false) {
                override fun getExtractedText(request: ExtractedTextRequest?, flags: Int): ExtractedText? = null
                override fun setSelection(start: Int, end: Int): Boolean { selections += start to end; return true }
            }
            val service = service(edit, connection)
            set(service, "clipboardSelectionMode", true)
            OpenLessImeService::class.java.getDeclaredMethod("sendClipboardCursorKey", Int::class.javaPrimitiveType)
                .apply { isAccessible = true }.invoke(service, KeyEvent.KEYCODE_DPAD_RIGHT)
        }
        assertTrue(selections.isEmpty())
    }
}
