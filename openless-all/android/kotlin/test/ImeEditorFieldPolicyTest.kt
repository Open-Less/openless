package com.openless.app

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ImeEditorFieldPolicyTest {
    @Test fun numericPhoneAndDateFieldsStartWithNumericKeys() {
        for (type in listOf(0x2, 0x3, 0x4)) assertTrue(ImeEditorFieldPolicy.prefersNumeric(type))
    }
    @Test fun numericFlagsAndVariationsKeepTheirClass() {
        for (type in listOf(0x1002, 0x2002, 0x12, 0x3002, 0x24)) assertTrue(ImeEditorFieldPolicy.prefersNumeric(type))
    }
    @Test fun ordinaryTextDoesNotOverwriteThePreferredPage() {
        for (type in listOf(0, 0x1, 0x20001, 0x81, 0xe1)) assertFalse(ImeEditorFieldPolicy.prefersNumeric(type))
    }
}
