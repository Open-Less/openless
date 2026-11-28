package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Test

class LatinInputModeTest {
    @Test
    fun toggledModeAlternatesBetweenEnglishAndPinyin() {
        assertEquals(
            OpenLessImeService.LatinInputMode.PINYIN,
            OpenLessImeService.LatinInputMode.ENGLISH.toggled(),
        )
        assertEquals(
            OpenLessImeService.LatinInputMode.ENGLISH,
            OpenLessImeService.LatinInputMode.PINYIN.toggled(),
        )
    }
}
