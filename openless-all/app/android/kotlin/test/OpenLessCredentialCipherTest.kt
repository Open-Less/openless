package com.openless.app

import java.security.GeneralSecurityException
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
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
            first.copyOfRange(1, 1 + OpenLessCredentialCipher.NONCE_BYTES).contentEquals(
                second.copyOfRange(1, 1 + OpenLessCredentialCipher.NONCE_BYTES),
            ),
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
        val packet = OpenLessCredentialCipher.seal(
            key,
            "secret".toByteArray(),
            "account-a".toByteArray(),
        )

        assertThrows(GeneralSecurityException::class.java) {
            OpenLessCredentialCipher.open(key, packet, "account-b".toByteArray())
        }
    }
}
