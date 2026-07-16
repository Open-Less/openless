package com.openless.app

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import androidx.annotation.Keep
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.KeyStoreException
import java.security.UnrecoverableKeyException
import javax.crypto.AEADBadTagException
import javax.crypto.BadPaddingException
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

internal const val CREDENTIAL_STATUS_OK: Byte = 0
internal const val CREDENTIAL_STATUS_KEY_MISSING: Byte = 1
internal const val CREDENTIAL_STATUS_AUTHENTICATION_FAILED: Byte = 2
internal const val CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE: Byte = 3
internal const val CREDENTIAL_STATUS_MALFORMED: Byte = 4

private fun credentialResponse(status: Byte, payload: ByteArray = byteArrayOf()): ByteArray {
    return byteArrayOf(status) + payload
}

internal fun credentialStatusForKeyLoadFailure(error: GeneralSecurityException): Byte {
    return when (error) {
        is KeyPermanentlyInvalidatedException -> CREDENTIAL_STATUS_KEY_MISSING
        else -> CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE
    }
}

/** AndroidKeyStore owner with fixed, secret-free status responses for JNI. */
internal class AndroidKeystoreCredentialVault(private val alias: String) {
    @Synchronized
    fun seal(plaintext: ByteArray, aad: ByteArray): ByteArray {
        return try {
            credentialResponse(
                CREDENTIAL_STATUS_OK,
                OpenLessCredentialCipher.seal(getOrCreateKey(), plaintext, aad),
            )
        } catch (error: KeyPermanentlyInvalidatedException) {
            credentialResponse(credentialStatusForKeyLoadFailure(error))
        } catch (error: UnrecoverableKeyException) {
            // Keystore2 wraps backend-busy and other provider failures in this
            // broad JCA exception too. Only an absent alias or the explicit
            // permanent-invalidated exception is safe to treat as data loss.
            credentialResponse(credentialStatusForKeyLoadFailure(error))
        } catch (_: IllegalArgumentException) {
            credentialResponse(CREDENTIAL_STATUS_MALFORMED)
        } catch (_: GeneralSecurityException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        } catch (_: IOException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        }
    }

    @Synchronized
    fun open(packet: ByteArray, aad: ByteArray): ByteArray {
        return try {
            val key = existingKey() ?: return credentialResponse(CREDENTIAL_STATUS_KEY_MISSING)
            credentialResponse(CREDENTIAL_STATUS_OK, OpenLessCredentialCipher.open(key, packet, aad))
        } catch (error: KeyPermanentlyInvalidatedException) {
            credentialResponse(credentialStatusForKeyLoadFailure(error))
        } catch (error: UnrecoverableKeyException) {
            credentialResponse(credentialStatusForKeyLoadFailure(error))
        } catch (_: AEADBadTagException) {
            credentialResponse(CREDENTIAL_STATUS_AUTHENTICATION_FAILED)
        } catch (_: BadPaddingException) {
            credentialResponse(CREDENTIAL_STATUS_AUTHENTICATION_FAILED)
        } catch (_: IllegalArgumentException) {
            credentialResponse(CREDENTIAL_STATUS_MALFORMED)
        } catch (_: GeneralSecurityException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        } catch (_: IOException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        }
    }

    @Synchronized
    fun deleteKey(): ByteArray {
        return try {
            val keyStore = loadKeyStore()
            if (keyStore.containsAlias(alias)) {
                keyStore.deleteEntry(alias)
            }
            credentialResponse(CREDENTIAL_STATUS_OK)
        } catch (_: GeneralSecurityException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        } catch (_: IOException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        }
    }

    @Synchronized
    fun keyExists(): ByteArray {
        return try {
            credentialResponse(
                CREDENTIAL_STATUS_OK,
                byteArrayOf(if (loadKeyStore().containsAlias(alias)) 1 else 0),
            )
        } catch (_: GeneralSecurityException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        } catch (_: IOException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        }
    }

    @Synchronized
    fun ensureKey(): ByteArray {
        return try {
            getOrCreateKey()
            credentialResponse(CREDENTIAL_STATUS_OK)
        } catch (error: KeyPermanentlyInvalidatedException) {
            credentialResponse(credentialStatusForKeyLoadFailure(error))
        } catch (error: UnrecoverableKeyException) {
            credentialResponse(credentialStatusForKeyLoadFailure(error))
        } catch (_: GeneralSecurityException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        } catch (_: IOException) {
            credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        }
    }

    @Throws(GeneralSecurityException::class, IOException::class)
    private fun existingKey(): SecretKey? {
        val keyStore = loadKeyStore()
        if (!keyStore.containsAlias(alias)) {
            return null
        }
        return keyStore.getKey(alias, null) as? SecretKey
    }

    @Throws(GeneralSecurityException::class, IOException::class)
    private fun getOrCreateKey(): SecretKey {
        existingKey()?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(
                alias,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        return generator.generateKey()
    }

    @Throws(KeyStoreException::class, IOException::class, GeneralSecurityException::class)
    private fun loadKeyStore(): KeyStore {
        return KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
    }

    private companion object {
        const val KEYSTORE_PROVIDER = "AndroidKeyStore"
    }
}

@Keep
object OpenLessCredentialVault {
    private const val KEY_ALIAS = "com.openless.app.credentials.v2"
    private const val MIGRATION_MARKER_ALIAS = "com.openless.app.credentials.v2.migrated"
    private val backend = AndroidKeystoreCredentialVault(KEY_ALIAS)
    private val migrationMarker = AndroidKeystoreCredentialVault(MIGRATION_MARKER_ALIAS)

    @JvmStatic
    fun seal(plaintext: ByteArray, aad: ByteArray): ByteArray = backend.seal(plaintext, aad)

    @JvmStatic
    fun open(packet: ByteArray, aad: ByteArray): ByteArray = backend.open(packet, aad)

    @JvmStatic
    fun deleteKey(): ByteArray = backend.deleteKey()

    @JvmStatic
    fun migrationComplete(): ByteArray = migrationMarker.keyExists()

    @JvmStatic
    fun markMigrationComplete(): ByteArray = migrationMarker.ensureKey()
}
