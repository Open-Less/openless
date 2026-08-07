package com.openless.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLessAccessibilityComponentIdsTest {
    private val full = "com.openless.app/com.openless.app.OpenLessAccessibilityService"
    private val shortForm = "com.openless.app/.OpenLessAccessibilityService"
    private val thirdParty = "com.example/.OtherService"
    private val similarClass =
        "com.openless.app/com.openless.app.OpenLessAccessibilityServiceFake"

    @Test
    fun componentIdsEqualTreatsShortAndFullFormsAsEqual() {
        assertTrue(OpenLessAccessibilityComponentIds.componentIdsEqual(shortForm, full))
        assertTrue(OpenLessAccessibilityComponentIds.componentIdsEqual(full, shortForm))
        assertEquals(full, OpenLessAccessibilityComponentIds.normalizeComponentKey(shortForm))
    }

    @Test
    fun enabledListContainsMatchesShortFormEntry() {
        assertTrue(OpenLessAccessibilityComponentIds.enabledListContains(shortForm, full))
    }

    @Test
    fun enabledListContainsMatchesInMultiServiceColonList() {
        val services = "$thirdParty:$shortForm"
        assertTrue(OpenLessAccessibilityComponentIds.enabledListContains(services, full))
    }

    @Test
    fun enabledListContainsRejectsSimilarClassNameSubstring() {
        assertFalse(OpenLessAccessibilityComponentIds.enabledListContains(similarClass, full))
        assertFalse(OpenLessAccessibilityComponentIds.componentIdsEqual(similarClass, full))
    }

    @Test
    fun enabledListContainsReturnsFalseForEmptyList() {
        assertFalse(OpenLessAccessibilityComponentIds.enabledListContains("", full))
    }
}
