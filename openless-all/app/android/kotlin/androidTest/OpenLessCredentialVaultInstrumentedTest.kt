package com.openless.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.security.KeyStore
import java.util.UUID
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class OpenLessCredentialVaultInstrumentedTest {
    private lateinit var alias: String
    private lateinit var vault: AndroidKeystoreCredentialVault

    @Before
    fun setUp() {
        alias = "com.openless.app.credentials.test.${UUID.randomUUID()}"
        vault = AndroidKeystoreCredentialVault(alias)
    }

    @After
    fun tearDown() {
        vault.deleteKey()
    }

    private fun payload(response: ByteArray): ByteArray {
        assertEquals(CREDENTIAL_STATUS_OK, response.first())
        return response.copyOfRange(1, response.size)
    }

    private fun legacyVault() =
        AndroidKeystoreCredentialVault("com.openless.app.credentials.v2")

    private fun softwareStore(): SoftwareAesCredentialStore {
        val filesDir = InstrumentationRegistry.getInstrumentation().targetContext.filesDir
        return SoftwareAesCredentialStore(File(filesDir, "OpenLess"))
    }

    private fun resetFacadeState() {
        OpenLessCredentialVault.deleteKey()
        legacyVault().deleteKey()
    }

    @Test
    fun roundTripUsesNonExportableKey() {
        val plaintext = "instrumented credential".toByteArray()
        val aad = "format-version-account".toByteArray()
        val packet = payload(vault.seal(plaintext, aad))

        assertArrayEquals(plaintext, payload(vault.open(packet, aad)))
        val key = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.getKey(alias, null)
        assertNull(key.encoded)
    }

    @Test
    fun keyMarkerStateTracksKeyCreationAndDeletion() {
        assertArrayEquals(byteArrayOf(0), payload(vault.keyExists()))
        assertEquals(CREDENTIAL_STATUS_OK, vault.ensureKey().first())
        assertArrayEquals(byteArrayOf(1), payload(vault.keyExists()))
        assertEquals(CREDENTIAL_STATUS_OK, vault.deleteKey().first())
        assertArrayEquals(byteArrayOf(0), payload(vault.keyExists()))
    }

    @Test
    fun publicFacadeRoundTripExercisesJvmStaticEntryPoints() {
        OpenLessCredentialVault.deleteKey()
        try {
            val plaintext = "facade credential".toByteArray()
            val aad = "facade aad".toByteArray()
            val packet = payload(OpenLessCredentialVault.seal(plaintext, aad))

            assertArrayEquals(plaintext, payload(OpenLessCredentialVault.open(packet, aad)))
        } finally {
            OpenLessCredentialVault.deleteKey()
        }
    }

    @Test
    fun publicFacadeReadsBeta1V2Envelope() {
        resetFacadeState()
        try {
            val plaintext = "beta1 credential".toByteArray()
            val aad = "format-version-account".toByteArray()
            val packet = payload(legacyVault().seal(plaintext, aad))

            assertArrayEquals(plaintext, payload(OpenLessCredentialVault.open(packet, aad)))
        } finally {
            resetFacadeState()
        }
    }

    @Test
    fun softwareKeyCreatedBeforeEnvelopeCommitDoesNotHideV2Envelope() {
        resetFacadeState()
        try {
            val plaintext = "still-v2 credential".toByteArray()
            val aad = "format-version-account".toByteArray()
            val packet = payload(legacyVault().seal(plaintext, aad))
            payload(softwareStore().seal("discarded candidate".toByteArray(), aad))

            assertArrayEquals(plaintext, payload(OpenLessCredentialVault.open(packet, aad)))
        } finally {
            resetFacadeState()
        }
    }

    @Test
    fun deletedKeyIsReportedAsMissing() {
        val aad = "format-version-account".toByteArray()
        val packet = payload(vault.seal("secret".toByteArray(), aad))
        assertEquals(CREDENTIAL_STATUS_OK, vault.deleteKey().first())

        assertEquals(CREDENTIAL_STATUS_KEY_MISSING, vault.open(packet, aad).first())
    }

    @Test
    fun tamperedCiphertextIsRejected() {
        val aad = "format-version-account".toByteArray()
        val packet = payload(vault.seal("secret".toByteArray(), aad))
        packet[packet.lastIndex] = (packet.last().toInt() xor 1).toByte()

        assertEquals(
            CREDENTIAL_STATUS_AUTHENTICATION_FAILED,
            vault.open(packet, aad).first(),
        )
    }
}
