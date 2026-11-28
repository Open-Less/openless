package com.openless.app

import android.content.ContextWrapper
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.File
import org.json.JSONArray
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class OpenLessClipboardHistoryInstrumentedTest {
    @get:Rule val temporaryFolder = TemporaryFolder()
    private lateinit var directory: File
    private lateinit var context: ContextWrapper
    private val history get() = File(directory, "openless_clipboard_history.json")
    private val staging get() = File(directory, "openless_clipboard_history.json.tmp")

    @Before fun setUp() {
        directory = temporaryFolder.newFolder("clipboard")
        context = object : ContextWrapper(null) {
            override fun getFilesDir(): File = directory
        }
    }

    @After fun tearDown() {
        unblock(staging)
        unblock(history)
        // A successful save clears the singleton's temporary copy before the next test.
        OpenLessClipboardHistory.clearNonFavorites(context)
    }

    private fun block(path: File) {
        assertTrue(path.mkdir())
        File(path, "blocker").writeText("keep the directory nonempty")
    }

    private fun unblock(path: File) {
        if (!path.isDirectory) return
        assertTrue(File(path, "blocker").delete())
        assertTrue(path.delete())
    }

    private fun texts() = OpenLessClipboardHistory.load(context).map { it.text }

    @Test fun blankTextDoesNotClaimPersistence() {
        assertFalse(OpenLessClipboardHistory.recordCopy(context, "   "))
        assertFalse(history.exists())
        assertTrue(texts().isEmpty())
    }

    @Test fun successfulWritesPreserveFavoriteAndDeduplicate() {
        assertTrue(OpenLessClipboardHistory.recordCopy(context, "favorite"))
        OpenLessClipboardHistory.toggleFavorite(context, "favorite")
        assertTrue(OpenLessClipboardHistory.recordCopy(context, "favorite"))
        val entries = OpenLessClipboardHistory.load(context)
        assertEquals(listOf("favorite"), entries.map { it.text })
        assertTrue(entries.single().favorite)
        assertTrue(JSONArray(history.readText()).getJSONObject(0).getBoolean("favorite"))
    }

    @Test fun failedWritesKeepBothNotesAndLeaveThePreviousFileIntact() {
        assertTrue(OpenLessClipboardHistory.recordCopy(context, "saved"))
        val before = history.readText()
        block(staging)

        assertFalse(OpenLessClipboardHistory.recordCopy(context, "first failed note"))
        assertFalse(OpenLessClipboardHistory.recordCopy(context, "second failed note"))
        assertEquals(listOf("second failed note", "first failed note", "saved"), texts())
        assertEquals(before, history.readText())

        unblock(staging)
        assertTrue(OpenLessClipboardHistory.recordCopy(context, "second failed note"))
        assertEquals(listOf("second failed note", "first failed note", "saved"), texts())
        val persisted = JSONArray(history.readText())
        assertEquals(3, persisted.length())
        assertEquals("first failed note", persisted.getJSONObject(1).getString("text"))
        assertFalse(staging.exists())
    }

    @Test fun failedReplacementAlsoRetainsTheText() {
        block(history)
        assertFalse(OpenLessClipboardHistory.recordCopy(context, "not replaced"))
        assertEquals(listOf("not replaced"), texts())
        assertTrue(File(history, "blocker").isFile)
        assertFalse(staging.exists())

        unblock(history)
        assertTrue(OpenLessClipboardHistory.recordCopy(context, "retry"))
        assertEquals(listOf("retry", "not replaced"), texts())
        assertEquals(2, JSONArray(history.readText()).length())
    }

    @Test fun temporaryHistoryStillSupportsFavoriteClearAndDelete() {
        block(staging)
        assertFalse(OpenLessClipboardHistory.recordCopy(context, "keep"))
        assertFalse(OpenLessClipboardHistory.recordCopy(context, "discard"))
        OpenLessClipboardHistory.toggleFavorite(context, "keep")
        OpenLessClipboardHistory.clearNonFavorites(context)
        assertEquals(listOf("keep"), texts())
        assertTrue(OpenLessClipboardHistory.load(context).single().favorite)
        OpenLessClipboardHistory.delete(context, "keep")
        assertTrue(texts().isEmpty())
    }
}
