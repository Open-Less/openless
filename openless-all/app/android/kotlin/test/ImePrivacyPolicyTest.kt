package com.openless.app
import org.junit.Assert.*
import org.junit.Test
class ImePrivacyPolicyTest {
    private var reads = 0
    private fun capture(
        enabled: Boolean = true, inputType: Int = 1, imeOptions: Int = 0, packageName: String? = "com.tencent.mm",
        before: CharSequence? = "前文", after: CharSequence? = "后文",
    ) = ImePrivacyPolicy.captureCursorContext(enabled, inputType, imeOptions, packageName,
        { reads++; before }, { reads++; after })

    @Test fun disabledSwitchNeverReadsTheEditor() {
        assertNull(capture(enabled = false))
        assertEquals(0, reads)
    }
    @Test fun passwordsNullAndPrivateEditorsAreNeverRead() {
        for (input in listOf(0x81, 0xe1, 0x91, 0x12, 0)) assertNull(capture(inputType = input))
        assertNull(capture(imeOptions = 0x1000000))
        assertEquals(0, reads)
    }
    @Test fun sensitivePackagesAreNeverRead() {
        for (name in listOf("com.x8bit.bitwarden", "com.kunzisoft.keepass.free", "com.termux",
            "com.lastpass.lpandroid", "com.dashlane", "com.agilebits.onepassword", "com.onepassword.android")) {
            assertTrue(name, ImePrivacyPolicy.isSensitivePackage(name))
            assertNull(name, capture(packageName = name))
        }
        assertFalse(ImePrivacyPolicy.isSensitivePackage("com.tencent.mm"))
        assertFalse(ImePrivacyPolicy.isSensitivePackage(null))
        assertEquals(0, reads)
    }
    @Test fun eitherSideAloneIsEnoughAndBothEmptyIsNothing() {
        assertEquals(AndroidCursorContext("前文", "后文", "com.tencent.mm"), capture())
        assertEquals(AndroidCursorContext("前文", "", "com.tencent.mm"), capture(after = null))
        assertEquals(AndroidCursorContext("", "后文", null), capture(before = "", packageName = null))
        assertNull(capture(before = null, after = null))
        assertNull(capture(before = " \n", after = ""))
    }
    @Test fun aThrowingEditorDegradesToNoContext() {
        assertNull(ImePrivacyPolicy.captureCursorContext(true, 1, 0, "app",
            { throw IllegalStateException() }, { throw SecurityException() }))
        assertEquals(AndroidCursorContext("", "后文", "app"), ImePrivacyPolicy.captureCursorContext(true, 1, 0, "app",
            { throw IllegalStateException() }, { "后文" }))
    }
    @Test fun readsStayWithinTheRequestedWindow() {
        val asked = mutableListOf<Int>()
        ImePrivacyPolicy.captureCursorContext(true, 1, 0, "app", { asked += it; "a" }, { asked += it; "b" })
        assertEquals(listOf(600, 200), asked)
    }
    @Test fun emojiCutByTheReadWindowDoesNotLeaveHalfACharacter() {
        val emoji = "🙂"
        val context = capture(before = emoji.substring(1) + "前文" + emoji, after = emoji + "后文" + emoji.substring(0, 1))!!
        assertEquals("前文$emoji", context.before)
        assertEquals("${emoji}后文", context.after)
    }
}
