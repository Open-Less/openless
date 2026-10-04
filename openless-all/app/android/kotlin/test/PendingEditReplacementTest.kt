package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class PendingEditReplacementTest {
    private fun armed(panelVisible: Boolean) = PendingEditReplacement(
        awaiting = true,
        panelVisible = panelVisible,
        original = "hello world",
        replacesWhole = true,
        addToDictionary = true,
    )

    @Test
    fun failureAfterTheEditPanelClosesDropsThePendingReplacement() {
        val released = armed(panelVisible = false).releaseIfPanelDismissed()
        assertFalse(released.awaiting)
        assertNull(released.original)
        assertFalse(released.replacesWhole)
        assertFalse(released.addToDictionary)
    }

    @Test
    fun failureWhileTheEditPanelIsOpenKeepsTheRetryArmed() {
        assertEquals(armed(panelVisible = true), armed(panelVisible = true).releaseIfPanelDismissed())
    }

    @Test
    fun idleDictationIsUnchanged() {
        val idle = PendingEditReplacement(
            awaiting = false,
            panelVisible = false,
            original = null,
            replacesWhole = false,
            addToDictionary = false,
        )
        assertEquals(idle, idle.releaseIfPanelDismissed())
    }
}
