package com.openless.app

import java.io.ByteArrayInputStream
import java.io.IOException
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class OpenLessContentReaderTest {
    private val archiveLimit = 512 * 1024

    @Test
    fun readBoundedAcceptsInputAtLimit() {
        val bytes = ByteArray(archiveLimit) { index -> (index % 251).toByte() }

        val result = OpenLessContentReader.readBounded(ByteArrayInputStream(bytes), archiveLimit)

        assertArrayEquals(bytes, result)
    }

    @Test
    fun readBoundedRejectsInputBeyondLimit() {
        assertThrows(IOException::class.java) {
            OpenLessContentReader.readBounded(
                ByteArrayInputStream(ByteArray(archiveLimit + 1)),
                archiveLimit,
            )
        }
    }
}
