package com.openless.app
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CloudNoteDeliveryTest {
    @Test fun blankTranscriptIsNotRetained() {
        assertFalse(cloudNoteShouldRetain(""))
        assertFalse(cloudNoteShouldRetain("   "))
    }

    @Test fun failedSubmitKeepsTheTranscriptInTheMessage() {
        assertTrue(cloudNoteShouldRetain("明天开会"))
        val message = cloudNoteFailureMessage(
            cloudNoteFailureDetail(CloudNoteFailure.Http, 500),
            retained = true,
        )
        assertEquals("云笔记提交失败（500），内容已保存在键盘剪贴板", message.first)
        assertEquals(
            "Cloud note submit failed (500). The text was saved in the keyboard clipboard.",
            message.second,
        )
    }

    @Test fun missingDestinationAndNetworkFailuresAlsoSayTheTextWasKept() {
        val missing = cloudNoteFailureMessage(
            cloudNoteFailureDetail(CloudNoteFailure.MissingDestination),
            retained = true,
        )
        assertEquals("请先在设置中填写云笔记的地址和 Token，内容已保存在键盘剪贴板", missing.first)
        val network = cloudNoteFailureMessage(
            cloudNoteFailureDetail(CloudNoteFailure.Network),
            retained = false,
        )
        assertEquals("云笔记提交失败，请检查网络", network.first)
        assertFalse(network.second.contains("clipboard"))
    }
}
