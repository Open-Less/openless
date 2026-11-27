package com.openless.app

import java.io.File
import java.lang.reflect.Modifier
import java.security.GeneralSecurityException
import java.security.InvalidKeyException
import java.security.UnrecoverableKeyException
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenLessCredentialCipherTest {
    private fun key(): SecretKey {
        return KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    }

    @Test
    fun roundTrip() {
        val key = key()
        val plaintext = "credential-secret".toByteArray()
        val aad = "format-version-account".toByteArray()

        val packet = OpenLessCredentialCipher.seal(key, plaintext, aad)

        assertArrayEquals(plaintext, OpenLessCredentialCipher.open(key, packet, aad))
        assertFalse(packet.toString(Charsets.UTF_8).contains("credential-secret"))
    }

    @Test
    fun freshNonce() {
        val key = key()
        val plaintext = "same plaintext".toByteArray()
        val aad = "same aad".toByteArray()

        val first = OpenLessCredentialCipher.seal(key, plaintext, aad)
        val second = OpenLessCredentialCipher.seal(key, plaintext, aad)

        assertFalse(first.contentEquals(second))
        assertFalse(
            first
                .copyOfRange(1, 1 + OpenLessCredentialCipher.NONCE_BYTES)
                .contentEquals(second.copyOfRange(1, 1 + OpenLessCredentialCipher.NONCE_BYTES))
        )
    }

    @Test
    fun tamperedCiphertext() {
        val key = key()
        val aad = "authenticated metadata".toByteArray()
        val packet = OpenLessCredentialCipher.seal(key, "secret".toByteArray(), aad)
        packet[packet.lastIndex] = (packet.last().toInt() xor 1).toByte()

        assertThrows(GeneralSecurityException::class.java) {
            OpenLessCredentialCipher.open(key, packet, aad)
        }
    }

    @Test
    fun tamperedNonce() {
        val key = key()
        val aad = "authenticated metadata".toByteArray()
        val packet = OpenLessCredentialCipher.seal(key, "secret".toByteArray(), aad)
        packet[1] = (packet[1].toInt() xor 1).toByte()

        assertThrows(GeneralSecurityException::class.java) {
            OpenLessCredentialCipher.open(key, packet, aad)
        }
    }

    @Test
    fun tamperedAad() {
        val key = key()
        val packet =
            OpenLessCredentialCipher.seal(
                key,
                "secret".toByteArray(),
                "account-a".toByteArray(),
            )

        assertThrows(GeneralSecurityException::class.java) {
            OpenLessCredentialCipher.open(key, packet, "account-b".toByteArray())
        }
    }

    @Test
    fun facadeMethodsExposeExactStaticJniSignatures() {
        val facade = OpenLessCredentialVault::class.java
        val signatures: List<Pair<String, Array<Class<*>>>> =
            listOf(
                "seal" to arrayOf<Class<*>>(ByteArray::class.java, ByteArray::class.java),
                "open" to arrayOf<Class<*>>(ByteArray::class.java, ByteArray::class.java),
                "deleteKey" to emptyArray(),
                "migrationComplete" to emptyArray(),
                "markMigrationComplete" to emptyArray(),
            )
        for ((name, parameters) in signatures) {
            val method = facade.getDeclaredMethod(name, *parameters)
            assertTrue("$name must be static for JNI", Modifier.isStatic(method.modifiers))
            assertEquals(ByteArray::class.java, method.returnType)
        }
    }

    @Test
    fun unrecoverableKeyExceptionRemainsRetryable() {
        assertEquals(
            CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE,
            credentialStatusForKeyLoadFailure(UnrecoverableKeyException("backend busy")),
        )
    }

    @Test
    fun invalidKeyExceptionIsTreatedAsUnrecoverable() {
        assertEquals(
            CREDENTIAL_STATUS_KEY_MISSING,
            credentialStatusForCipherKeyFailure(InvalidKeyException("Keystore operation failed")),
        )
    }

    @Test
    fun softwareAesRoundTripWithoutAndroidKeyStore() {
        val dir = File.createTempFile("ol-sw-aes", "dir")
        assertTrue(dir.delete())
        assertTrue(dir.mkdirs())
        try {
            val store = SoftwareAesCredentialStore(dir)
            val plaintext = "credential-secret".toByteArray()
            val aad = "format-version-account".toByteArray()
            val sealed = store.seal(plaintext, aad)
            assertEquals(CREDENTIAL_STATUS_OK, sealed.first())
            val packet = sealed.copyOfRange(1, sealed.size)
            val opened = store.open(packet, aad)
            assertEquals(CREDENTIAL_STATUS_OK, opened.first())
            assertArrayEquals(plaintext, opened.copyOfRange(1, opened.size))
            assertTrue(File(dir, SoftwareAesCredentialStore.SOFTWARE_KEY_NAME).isFile)
            assertFalse(store.isMigrated())
            assertEquals(CREDENTIAL_STATUS_OK, store.markMigrated().first())
            assertTrue(store.isMigrated())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun softwareAesMissingKeyIsReportedAsMissing() {
        val dir = File.createTempFile("ol-sw-aes-missing", "dir")
        assertTrue(dir.delete())
        assertTrue(dir.mkdirs())
        try {
            val store = SoftwareAesCredentialStore(dir)
            assertEquals(
                CREDENTIAL_STATUS_KEY_MISSING,
                store.open(byteArrayOf(12) + ByteArray(12 + 16), "aad".toByteArray()).first(),
            )
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun openFallbackPrefersSuccessAndNeverDowngradesARecoverableFailureToMissing() {
        val success = byteArrayOf(CREDENTIAL_STATUS_OK, 7)
        var afterSuccessCalled = false
        assertArrayEquals(
            success,
            credentialOpenWithFallback(
                { byteArrayOf(CREDENTIAL_STATUS_AUTHENTICATION_FAILED) },
                { success },
                {
                    afterSuccessCalled = true
                    byteArrayOf(CREDENTIAL_STATUS_OK, 8)
                },
            ),
        )
        assertFalse(afterSuccessCalled)
        assertEquals(
            CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE,
            credentialOpenWithFallback(
                    { byteArrayOf(CREDENTIAL_STATUS_KEY_MISSING) },
                    { byteArrayOf(CREDENTIAL_STATUS_AUTHENTICATION_FAILED) },
                    { byteArrayOf(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE) },
                )
                .first(),
        )
        assertEquals(
            CREDENTIAL_STATUS_AUTHENTICATION_FAILED,
            credentialOpenWithFallback(
                    { byteArrayOf(CREDENTIAL_STATUS_KEY_MISSING) },
                    { byteArrayOf(CREDENTIAL_STATUS_AUTHENTICATION_FAILED) },
                )
                .first(),
        )
        assertEquals(
            CREDENTIAL_STATUS_MALFORMED,
            credentialOpenWithFallback(
                    { byteArrayOf(CREDENTIAL_STATUS_KEY_MISSING) },
                    { byteArrayOf(CREDENTIAL_STATUS_MALFORMED) },
                )
                .first(),
        )
        assertEquals(
            CREDENTIAL_STATUS_KEY_MISSING,
            credentialOpenWithFallback(
                    { byteArrayOf(CREDENTIAL_STATUS_KEY_MISSING) },
                    { byteArrayOf(CREDENTIAL_STATUS_KEY_MISSING) },
                )
                .first(),
        )
    }
}
