package com.openless.app

import java.security.GeneralSecurityException
import javax.crypto.Cipher
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Pure JCA AES-GCM packet codec; AndroidKeyStore ownership lives in the facade. */
internal object OpenLessCredentialCipher {
    internal const val NONCE_BYTES = 12
    internal const val TAG_BITS = 128
    private const val TAG_BYTES = TAG_BITS / 8
    private const val TRANSFORMATION = "AES/GCM/NoPadding"

    @Throws(GeneralSecurityException::class)
    fun seal(key: SecretKey, plaintext: ByteArray, aad: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val nonce = cipher.iv
        require(nonce.size == NONCE_BYTES) { "unexpected AES-GCM nonce length" }
        cipher.updateAAD(aad)
        val ciphertext = cipher.doFinal(plaintext)
        return byteArrayOf(nonce.size.toByte()) + nonce + ciphertext
    }

    @Throws(GeneralSecurityException::class)
    fun open(key: SecretKey, packet: ByteArray, aad: ByteArray): ByteArray {
        if (packet.isEmpty()) {
            throw IllegalArgumentException("malformed credential packet")
        }
        val nonceLength = packet[0].toInt() and 0xff
        if (
            nonceLength != NONCE_BYTES ||
            packet.size < 1 + nonceLength + TAG_BYTES
        ) {
            throw IllegalArgumentException("malformed credential packet")
        }
        val nonce = packet.copyOfRange(1, 1 + nonceLength)
        val ciphertext = packet.copyOfRange(1 + nonceLength, packet.size)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, nonce))
        cipher.updateAAD(aad)
        return cipher.doFinal(ciphertext)
    }
}
