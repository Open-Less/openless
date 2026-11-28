package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ImeSurfaceLayoutTest {
    @Test fun normalKeyboardKeepsPreferredHeight() {
        assertEquals(ImeSurfaceSize(300, 0), ImeSurfaceLayout.resolve(300, 0, 900, 220))
    }
    @Test fun draftAndKeysFitNormally() {
        assertEquals(ImeSurfaceSize(300, 140), ImeSurfaceLayout.resolve(300, 140, 900, 220))
    }
    @Test fun longDraftReservesUsableKeysAndScrolls() {
        assertEquals(ImeSurfaceSize(220, 140), ImeSurfaceLayout.resolve(300, 500, 360, 220))
    }
    @Test fun tinyWindowsDoNotProduceNegativeDimensions() {
        assertEquals(ImeSurfaceSize(100, 0), ImeSurfaceLayout.resolve(300, 140, 100, 220))
        assertEquals(ImeSurfaceSize(0, 0), ImeSurfaceLayout.resolve(300, 140, -1, 220))
    }
    @Test fun invalidPreferencesAreClampedWithoutInventingSpace() {
        assertEquals(ImeSurfaceSize(0, 0), ImeSurfaceLayout.resolve(-10, -20, 400, -1))
        assertEquals(ImeSurfaceSize(200, 0), ImeSurfaceLayout.resolve(200, 0, 400, 800))
    }
    @Test fun dimensionsAreBoundedAcrossConstrainedWindows() {
        for (available in listOf(-1, 0, 40, 100, 220, 320, 480, 900)) {
            for (keys in listOf(-10, 0, 220, 300, 420, 540)) {
                for (draft in listOf(-10, 0, 80, 140, 600)) {
                    val result = ImeSurfaceLayout.resolve(keys, draft, available, 220)
                    assertTrue(result.keyboard >= 0 && result.draft >= 0)
                    assertTrue(result.total <= available.coerceAtLeast(0))
                    assertTrue(result.keyboard <= keys.coerceAtLeast(0))
                    assertTrue(result.draft <= draft.coerceAtLeast(0))
                }
            }
        }
    }
}
