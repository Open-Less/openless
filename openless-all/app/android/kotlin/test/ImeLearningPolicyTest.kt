package com.openless.app
import org.junit.Assert.*
import org.junit.Test
class ImeLearningPolicyTest {
    @Test fun passwordsAndPrivateEditorsNeverLearn() {
        for (input in listOf(0x81, 0xe1, 0x91, 0x12)) {
            assertTrue(ImeLearningPolicy.isPassword(input))
            assertFalse(ImeLearningPolicy.allowsLearning(input, 0))
        }
        assertTrue(ImeLearningPolicy.allowsLearning(1, 0))
        assertFalse(ImeLearningPolicy.allowsLearning(1, 0x1000000))
        assertFalse(ImeLearningPolicy.allowsLearning(0, 0))
    }
}
