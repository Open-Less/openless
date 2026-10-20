package com.openless.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.Before
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.runner.RunWith

/** Real packaged JNI + dictionary test, using a non-learning disposable session. */
@RunWith(AndroidJUnit4::class)
class RimeEngineIntegrationTest {
    @Before fun keepDictionaryTestsOffline() {
        InstrumentationRegistry.getInstrumentation().targetContext
            .getSharedPreferences("openless_ime_ui", android.content.Context.MODE_PRIVATE)
            .edit().putBoolean("public_dictionary_updates", false).commit()
    }
    @Test fun externalCandidateUsesRevisionAndPendingCommitContract() {
        OpenLessRime.initialize(InstrumentationRegistry.getInstrumentation().targetContext)
        val session = OpenLessRime.request("create", "", "private").getString("session")
        try {
            val state = typePinyin(session, "nihao")
            val revision = state.getLong("revision")
            try {
                OpenLessRime.request("external", session, "${revision - 1}@你好世界")
                fail("A cloud reply from an obsolete composition must be rejected")
            } catch (_: IllegalStateException) {}
            val selected = OpenLessRime.request("external", session, "$revision@你好世界")
            assertEquals("你好世界", selected.getString("commit"))
            assertEquals("", selected.getString("input"))
            assertEquals("你好世界", OpenLessRime.request("state", session).getString("commit"))
            assertEquals("", OpenLessRime.request("ack", session).getString("commit"))
        } finally { OpenLessRime.request("destroy", session) }
    }
    @Test fun candidatesDeletionStaleSelectionAndCommitAcknowledgement() {
        OpenLessRime.initialize(InstrumentationRegistry.getInstrumentation().targetContext)
        var state = OpenLessRime.request("create", "", "private")
        val session = state.getString("session")
        try {
            for (letter in "nihao") state = OpenLessRime.request("key", session, letter.code.toString())
            val oldRevision = state.getLong("revision")
            state = OpenLessRime.request("key", session, "65288")
            assertEquals("niha", state.getString("input"))
            try {
                OpenLessRime.request("select", session, "0@$oldRevision")
                fail("Old candidate must be rejected")
            } catch (_: IllegalStateException) {}
            state = OpenLessRime.request("key", session, "111")
            val candidates = state.getJSONArray("candidates")
            val index = (0 until candidates.length()).first { candidates.getString(it) == "你好" }
            state = OpenLessRime.request("select", session, "$index@${state.getLong("revision")}")
            assertEquals("你好", state.getString("commit"))
            assertEquals("你好", OpenLessRime.request("state", session).getString("commit"))
            assertEquals("", OpenLessRime.request("ack", session).getString("commit"))
            state = OpenLessRime.request("clear", session)
            assertEquals("", state.getString("input"))
            for (letter in "nihao") state = OpenLessRime.request("key", session, letter.code.toString())
            assertEquals("你好", OpenLessRime.request("key", session, "32").getString("commit"))
            OpenLessRime.request("ack", session)
        } finally { OpenLessRime.request("destroy", session) }
        try {
            OpenLessRime.request("state", session)
            fail("Destroyed session must be rejected")
        } catch (_: IllegalStateException) {}
    }
    @Test fun observingAndExpandingCandidatesPreservesSelectionAndNativeOrder() {
        OpenLessRime.initialize(InstrumentationRegistry.getInstrumentation().targetContext)
        val session = OpenLessRime.request("create", "", "private").getString("session")
        try {
            var state = OpenLessRime.request("state", session)
            for (letter in "shi") state = OpenLessRime.request("key", session, letter.code.toString())
            val revision = state.getLong("revision")
            val firstPage = state.getJSONArray("candidates")
            val expanded = OpenLessRime.request("candidates", session)
            assertEquals(revision, expanded.getLong("revision"))
            assertEquals(revision, OpenLessRime.request("state", session).getLong("revision"))
            val rows = expanded.getJSONArray("candidates")
            assertTrue("Candidates beyond the first page must be available", rows.length() > firstPage.length())
            assertTrue(rows.length() <= 64)
            for (i in 0 until firstPage.length()) assertEquals(firstPage.getString(i), rows.getString(i))
            val index = firstPage.length()
            state = OpenLessRime.request("select_absolute", session, "$index@$revision")
            assertEquals(rows.getString(index), state.getString("commit"))
            OpenLessRime.request("ack", session)
        } finally { OpenLessRime.request("destroy", session) }
    }

    @Test fun rangedCandidatesDoNotChangeSpaceSelectionOrPage() {
        OpenLessRime.initialize(InstrumentationRegistry.getInstrumentation().targetContext)
        for (useSpace in listOf(true, false)) {
            val session = OpenLessRime.request("create", "", "private").getString("session")
            try {
                for (letter in "shi") OpenLessRime.request("key", session, letter.code.toString())
                val before = OpenLessRime.request("state", session)
                val revision = before.getLong("revision")
                val offset = before.getJSONArray("candidates").length()
                val range = OpenLessRime.request("candidates", session, "$offset@$revision")
                val rows = range.getJSONArray("candidates")
                assertTrue(rows.length() in 1..32)
                assertEquals(offset, range.getInt("candidateOffset"))
                assertEquals(revision, range.getLong("revision"))
                val after = OpenLessRime.request("state", session)
                assertEquals(before.getInt("page"), after.getInt("page"))
                assertEquals(before.getJSONArray("candidates").toString(), after.getJSONArray("candidates").toString())
                try {
                    OpenLessRime.request("candidates", session, "$offset@${revision - 1}")
                    fail("A range from an obsolete composition must be rejected")
                } catch (_: IllegalStateException) {}
                val selected = if (useSpace) OpenLessRime.request("key", session, "32")
                    else OpenLessRime.request("select_absolute", session, "$offset@$revision")
                assertEquals(if (useSpace) before.getJSONArray("candidates").getString(0) else rows.getString(0),
                    selected.getString("commit"))
                OpenLessRime.request("ack", session)
            } finally { OpenLessRime.request("destroy", session) }
        }
    }

    private fun typePinyin(session: String, input: String): org.json.JSONObject {
        for (letter in input) OpenLessRime.request("key", session, letter.code.toString())
        return OpenLessRime.request("candidates", session)
    }

    private fun choose(session: String, state: org.json.JSONObject, word: String): org.json.JSONObject {
        val rows = state.getJSONArray("candidates")
        val index = (0 until rows.length()).firstOrNull { rows.getString(it) == word }
        assertNotNull("Expected candidate $word", index)
        return OpenLessRime.request("select_absolute", session, "$index@${state.getLong("revision")}")
    }

    @Test fun nativeLearningSurvivesSessionRecreationAndPrivateSelectionsDoNotTrainIt() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        OpenLessRime.initialize(context)
        var session = OpenLessRime.request("create", "", "private").getString("session")
        val baseline: String
        try {
            val state = typePinyin(session, "ni")
            assertFalse(state.getBoolean("learning"))
            baseline = state.getJSONArray("candidates").getString(0)
        } finally { OpenLessRime.request("destroy", session) }
        assertNotEquals("Test needs a non-default candidate", "泥", baseline)
        repeat(6) {
            val created = OpenLessRime.request("create", "", "learning")
            assertTrue(created.getBoolean("learning"))
            session = created.getString("session")
            try {
                val state = choose(session, typePinyin(session, "ni"), "泥")
                assertEquals("泥", state.getString("commit"))
                OpenLessRime.request("ack", session)
            } finally { OpenLessRime.request("destroy", session) }
        }
        // Each preceding session was destroyed, closing its user dictionary.
        // Private sessions neither consult nor update the personal ranking.
        repeat(6) {
            session = OpenLessRime.request("create", "", "private").getString("session")
            try {
                val state = typePinyin(session, "ni")
                assertEquals(baseline, state.getJSONArray("candidates").getString(0))
                assertEquals("逆", choose(session, state, "逆").getString("commit"))
                OpenLessRime.request("ack", session)
            } finally { OpenLessRime.request("destroy", session) }
        }
        session = OpenLessRime.request("create", "", "learning").getString("session")
        try {
            assertEquals("泥", typePinyin(session, "ni").getJSONArray("candidates").getString(0))
        } finally { OpenLessRime.request("destroy", session) }
        val database = java.io.File(context.filesDir, "rime-user/pinyin_simp.userdb")
        assertTrue("Learning must be stored in the app's persistent profile", database.isDirectory)
        assertTrue(database.listFiles()?.any { it.length() > 0 } == true)
    }

    @Test fun selectingSeparateSyllablesLearnsANewPhrase() {
        OpenLessRime.initialize(InstrumentationRegistry.getInstrumentation().targetContext)
        repeat(3) {
            val session = OpenLessRime.request("create", "", "learning").getString("session")
            try {
                var state = typePinyin(session, "nihao")
                state = choose(session, state, "拟")
                assertEquals("", state.getString("commit"))
                state = OpenLessRime.request("candidates", session)
                state = choose(session, state, "蚝")
                assertEquals("拟蚝", state.getString("commit"))
                OpenLessRime.request("ack", session)
            } finally { OpenLessRime.request("destroy", session) }
        }
        val session = OpenLessRime.request("create", "", "learning").getString("session")
        try {
            val state = typePinyin(session, "nihao")
            assertEquals("拟蚝", state.getJSONArray("candidates").getString(0))
        } finally { OpenLessRime.request("destroy", session) }
        val privateSession = OpenLessRime.request("create", "", "private").getString("session")
        try {
            val rows = typePinyin(privateSession, "nihao").getJSONArray("candidates")
            assertFalse((0 until rows.length()).any { rows.getString(it) == "拟蚝" })
        } finally { OpenLessRime.request("destroy", privateSession) }
    }

    private fun withController(test: (RimePinyinController, StringBuilder) -> Unit) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        // Cold dictionary compilation is a one-time deployment, not input queue work.
        // Complete it on the test thread before applying the queue's 30-second deadline.
        OpenLessRime.initialize(instrumentation.targetContext)
        val controller = RimePinyinController(instrumentation.targetContext)
        val committed = StringBuilder()
        instrumentation.runOnMainSync { controller.bindCommit { committed.append(it); true } }
        try { test(controller, committed) }
        finally { instrumentation.runOnMainSync { controller.shutdown() } }
    }

    private fun drain(controller: RimePinyinController, actions: () -> Unit) {
        val done = CountDownLatch(1)
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            actions()
            controller.refresh { done.countDown() }
        }
        assertTrue("Input queue did not finish", done.await(30, TimeUnit.SECONDS))
    }

    @Test fun fastSpaceAndBackspaceUseQueuedCompositionRatherThanStaleUi() = withController { controller, committed ->
        val deletions = AtomicInteger()
        drain(controller) {
            for (letter in "nihao") controller.appendLetter(letter) {}
            controller.space {}
            // The second space must reach the editor, not the now-empty Rime session.
            controller.space {}
            controller.backspace({}, { deletions.incrementAndGet() })
        }
        assertEquals("你好 ", committed.toString())
        assertEquals(1, deletions.get())
        assertTrue(controller.isEmpty())
        drain(controller) {
            controller.appendLetter('n') {}
            controller.backspace({}, { deletions.incrementAndGet() })
        }
        assertEquals(1, deletions.get())
        assertTrue(controller.isEmpty())
    }

    @Test fun queuedEnterConfirmsOnlyOnceThenUsesEditorAction() = withController { controller, committed ->
        val enters = AtomicInteger()
        drain(controller) {
            for (letter in "nihao") controller.appendLetter(letter) {}
            controller.confirmFirst({}, { enters.incrementAndGet() })
            controller.confirmFirst({}, { enters.incrementAndGet() })
        }
        assertEquals("你好", committed.toString())
        assertEquals(1, enters.get())
        assertTrue(controller.isEmpty())
    }

    @Test fun refreshKeepsCompositionAndApostropheSeparatesSyllables() = withController { controller, committed ->
        drain(controller) {
            for (letter in "xi") controller.appendLetter(letter) {}
            controller.insertLiteral("'") {}
            for (letter in "an") controller.appendLetter(letter) {}
        }
        assertEquals("xi'an", controller.currentEncoding())
        assertEquals("", committed.toString())
        drain(controller) { controller.refresh {} }
        assertEquals("xi'an", controller.currentEncoding())
        drain(controller) { controller.selectCandidate("西安") {} }
        assertEquals("西安", committed.toString())
    }

    @Test fun failedCommitRetriesOnceWithoutLosingFollowingPunctuation() = withController { controller, committed ->
        val attempts = AtomicInteger()
        drain(controller) {
            controller.bindCommit { text ->
                if (attempts.incrementAndGet() == 1) false else { committed.append(text); true }
            }
            for (letter in "nihao") controller.appendLetter(letter) {}
            controller.insertLiteral("!") {}
        }
        assertEquals("你好!", committed.toString())
        assertEquals(3, attempts.get())
        drain(controller) { controller.refresh {} }
        assertEquals("你好!", committed.toString())
    }

    @Test fun clearingQueuedInputCannotWriteToNextEditor() = withController { controller, committed ->
        drain(controller) {
            for (letter in "nihao") controller.appendLetter(letter) {}
            controller.space {}
            controller.clear()
        }
        assertEquals("", committed.toString())
        assertTrue(controller.isEmpty())
    }

}
