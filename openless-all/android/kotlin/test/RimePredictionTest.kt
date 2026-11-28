package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Test

class RimePredictionTest {
    @Test fun movesOnlyExistingCandidatesAndPreservesDefault() {
        val rows = listOf("抱", "爆", "報", "报", "宝")
        assertEquals(listOf("抱", "报", "爆", "報", "宝"),
            RimePrediction.promote(rows, mapOf("报" to 0, "预报" to 1)))
    }

    @Test fun contextKeepsEightWholeCodePoints() {
        assertEquals("𠀀一二三四五六七", RimePrediction.append("天气𠀀一二三四五六", "七"))
    }
}
