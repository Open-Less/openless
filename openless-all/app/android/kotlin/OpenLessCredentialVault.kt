package com.openless.app

import android.os.Handler
import android.os.Looper
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.UserNotAuthenticatedException
import androidx.annotation.Keep
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.InvalidKeyException
import java.security.KeyStore
import java.security.KeyStoreException
import java.security.SecureRandom
import java.security.UnrecoverableKeyException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import javax.crypto.AEADBadTagException
import javax.crypto.BadPaddingException
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.SecretKeySpec

internal const val CREDENTIAL_STATUS_OK: Byte = 0
internal const val CREDENTIAL_STATUS_KEY_MISSING: Byte = 1
internal const val CREDENTIAL_STATUS_AUTHENTICATION_FAILED: Byte = 2
internal const val CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE: Byte = 3
internal const val CREDENTIAL_STATUS_MALFORMED: Byte = 4

private fun credentialResponse(status: Byte, payload: ByteArray = byteArrayOf()): ByteArray {
    return byteArrayOf(status) + payload
}

internal fun credentialOpenWithFallback(vararg attempts: () -> ByteArray): ByteArray {
    var temporarilyUnavailable: ByteArray? = null
    var malformed: ByteArray? = null
    var authenticationFailed: ByteArray? = null
    for (attempt in attempts) {
        val response = attempt()
        when (response.firstOrNull()) {
            CREDENTIAL_STATUS_OK -> return response
            CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE ->
                temporarilyUnavailable = temporarilyUnavailable ?: response
            CREDENTIAL_STATUS_MALFORMED -> malformed = malformed ?: response
            CREDENTIAL_STATUS_AUTHENTICATION_FAILED ->
                authenticationFailed = authenticationFailed ?: response
            CREDENTIAL_STATUS_KEY_MISSING -> {}
            else ->
                temporarilyUnavailable =
                    temporarilyUnavailable
                        ?: credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
        }
    }
    return temporarilyUnavailable
        ?: malformed
        ?: authenticationFailed
        ?: credentialResponse(CREDENTIAL_STATUS_KEY_MISSING)
}

private fun diagnosticResponse(status: Byte, error: Throwable): ByteArray {
    val name = buildString {
        append(error.javaClass.simpleName.take(40))
        error.cause?.javaClass?.simpleName?.let { cause ->
            append('/')
            append(cause.take(40))
        }
        keystoreNumericCode(error)?.let { code ->
            append(':')
            append(code)
        }
        append(':')
        append(if (Looper.myLooper() == Looper.getMainLooper()) "main" else "bg")
    }
    return credentialResponse(status, name.toByteArray(Charsets.UTF_8))
}

private fun keystoreNumericCode(error: Throwable): Int? {
    var current: Throwable? = error
    while (current != null) {
        try {
            for (methodName in arrayOf("getNumericErrorCode", "getErrorCode")) {
                val method =
                    current.javaClass.methods.firstOrNull { it.name == methodName && it.parameterCount == 0 }
                        ?: continue
                when (val value = method.invoke(current)) {
                    is Int -> return value
                }
            }
        } catch (_: Throwable) {}
        current = current.cause
    }
    return null
}

internal fun credentialStatusForKeyLoadFailure(error: GeneralSecurityException): Byte {
    return when (error) {
        is KeyPermanentlyInvalidatedException -> CREDENTIAL_STATUS_KEY_MISSING
        else -> CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE
    }
}

internal fun credentialStatusForCipherKeyFailure(error: InvalidKeyException): Byte {
    return when (error) {
        is UserNotAuthenticatedException -> CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE
        // Cipher cannot use this key, so the envelope cannot be recovered.
        else -> CREDENTIAL_STATUS_KEY_MISSING
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
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: UnrecoverableKeyException) {
            // Keystore2 wraps backend-busy and other provider failures in this
            // broad JCA exception too. Only an absent alias or the explicit
            // permanent-invalidated exception is safe to treat as data loss.
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: InvalidKeyException) {
            diagnosticResponse(credentialStatusForCipherKeyFailure(error), error)
        } catch (_: IllegalArgumentException) {
            credentialResponse(CREDENTIAL_STATUS_MALFORMED)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    @Synchronized
    fun open(packet: ByteArray, aad: ByteArray): ByteArray {
        return try {
            val key = existingKey() ?: return credentialResponse(CREDENTIAL_STATUS_KEY_MISSING)
            credentialResponse(
                CREDENTIAL_STATUS_OK,
                OpenLessCredentialCipher.open(key, packet, aad),
            )
        } catch (error: KeyPermanentlyInvalidatedException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: UnrecoverableKeyException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: InvalidKeyException) {
            diagnosticResponse(credentialStatusForCipherKeyFailure(error), error)
        } catch (_: AEADBadTagException) {
            credentialResponse(CREDENTIAL_STATUS_AUTHENTICATION_FAILED)
        } catch (_: BadPaddingException) {
            credentialResponse(CREDENTIAL_STATUS_AUTHENTICATION_FAILED)
        } catch (_: IllegalArgumentException) {
            credentialResponse(CREDENTIAL_STATUS_MALFORMED)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
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
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    @Synchronized
    fun keyExists(): ByteArray {
        return try {
            credentialResponse(
                CREDENTIAL_STATUS_OK,
                byteArrayOf(if (loadKeyStore().containsAlias(alias)) 1 else 0),
            )
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    @Synchronized
    fun ensureKey(): ByteArray {
        return try {
            getOrCreateKey()
            credentialResponse(CREDENTIAL_STATUS_OK)
        } catch (error: KeyPermanentlyInvalidatedException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: UnrecoverableKeyException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
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
        existingKey()?.let {
            return it
        }
        return createKey()
    }

    @Throws(GeneralSecurityException::class, IOException::class)
    private fun createKey(): SecretKey {
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
                .build()
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

/**
 * App-private AES-GCM wrapping key used when AndroidKeyStore/KeyMint rejects
 * AES-GCM (observed as KeyStoreException numeric 10 on some HyperOS devices).
 * The raw key is UID-scoped, same as the envelope file; it is not hardware-backed.
 */
internal class SoftwareAesCredentialStore(private val directory: File) {
    fun keyExists(): Boolean {
        val file = keyFile()
        return file.isFile && file.length() == KEY_BYTES.toLong()
    }

    fun isMigrated(): Boolean = migratedFile().isFile

    fun seal(plaintext: ByteArray, aad: ByteArray): ByteArray {
        return try {
            val key = loadOrCreateKey()
            credentialResponse(CREDENTIAL_STATUS_OK, OpenLessCredentialCipher.seal(key, plaintext, aad))
        } catch (_: IllegalArgumentException) {
            credentialResponse(CREDENTIAL_STATUS_MALFORMED)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    fun open(packet: ByteArray, aad: ByteArray): ByteArray {
        return try {
            val key = loadExistingKey() ?: return credentialResponse(CREDENTIAL_STATUS_KEY_MISSING)
            credentialResponse(
                CREDENTIAL_STATUS_OK,
                OpenLessCredentialCipher.open(key, packet, aad),
            )
        } catch (_: AEADBadTagException) {
            credentialResponse(CREDENTIAL_STATUS_AUTHENTICATION_FAILED)
        } catch (_: BadPaddingException) {
            credentialResponse(CREDENTIAL_STATUS_AUTHENTICATION_FAILED)
        } catch (_: IllegalArgumentException) {
            credentialResponse(CREDENTIAL_STATUS_MALFORMED)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    fun deleteKey(): ByteArray {
        return try {
            deleteIfPresent(keyFile())
            deleteIfPresent(migratedFile())
            credentialResponse(CREDENTIAL_STATUS_OK)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    fun markMigrated(): ByteArray {
        return try {
            if (!directory.exists() && !directory.mkdirs() && !directory.isDirectory) {
                throw IOException("software-migrated-dir")
            }
            migratedFile().writeBytes(byteArrayOf(1))
            restrictPrivate(migratedFile())
            credentialResponse(CREDENTIAL_STATUS_OK)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    private fun keyFile() = File(directory, SOFTWARE_KEY_NAME)

    private fun migratedFile() = File(directory, SOFTWARE_MIGRATED_NAME)

    @Throws(GeneralSecurityException::class, IOException::class)
    private fun loadExistingKey(): SecretKey? {
        val file = keyFile()
        if (!file.isFile) {
            return null
        }
        val raw = file.readBytes()
        if (raw.size != KEY_BYTES) {
            throw GeneralSecurityException("software-key-size")
        }
        return SecretKeySpec(raw, "AES")
    }

    @Throws(GeneralSecurityException::class, IOException::class)
    private fun loadOrCreateKey(): SecretKey {
        loadExistingKey()?.let {
            return it
        }
        if (!directory.exists() && !directory.mkdirs() && !directory.isDirectory) {
            throw IOException("software-key-dir")
        }
        val raw = ByteArray(KEY_BYTES)
        SecureRandom().nextBytes(raw)
        val file = keyFile()
        val tmp = File(directory, "$SOFTWARE_KEY_NAME.tmp")
        try {
            FileOutputStream(tmp).use { output ->
                output.write(raw)
                output.fd.sync()
            }
            restrictPrivate(tmp)
            if (!tmp.renameTo(file)) {
                return loadExistingKey() ?: throw IOException("software-key-install")
            }
        } finally {
            if (tmp.exists()) {
                tmp.delete()
            }
        }
        return SecretKeySpec(raw, "AES")
    }

    @Throws(IOException::class)
    private fun deleteIfPresent(file: File) {
        if (file.exists() && !file.delete()) {
            throw IOException("software-key-delete")
        }
    }

    private fun restrictPrivate(file: File) {
        file.setReadable(false, false)
        file.setWritable(false, false)
        file.setReadable(true, true)
        file.setWritable(true, true)
    }

    companion object {
        const val SOFTWARE_KEY_NAME = "credentials.sw.key"
        const val SOFTWARE_MIGRATED_NAME = "credentials.sw.migrated"
        const val KEY_BYTES = 32
    }
}

@Keep
object OpenLessCredentialVault {
    // v2 alias on this HyperOS device became unusable (InvalidKeyException /
    // ProviderException). v3 is a fresh Keystore2 slot after envelope wipe.
    private const val KEY_ALIAS = "com.openless.app.credentials.v3"
    private const val MIGRATION_MARKER_ALIAS = "com.openless.app.credentials.v3.migrated"
    private const val LEGACY_KEY_ALIAS = "com.openless.app.credentials.v2"
    private const val LEGACY_MIGRATION_MARKER_ALIAS = "com.openless.app.credentials.v2.migrated"
    private val backend = AndroidKeystoreCredentialVault(KEY_ALIAS)
    private val migrationMarker = AndroidKeystoreCredentialVault(MIGRATION_MARKER_ALIAS)
    private val legacyBackend = AndroidKeystoreCredentialVault(LEGACY_KEY_ALIAS)
    private val legacyMigrationMarker =
        AndroidKeystoreCredentialVault(LEGACY_MIGRATION_MARKER_ALIAS)

    @JvmStatic
    fun seal(plaintext: ByteArray, aad: ByteArray): ByteArray =
        runOnMain {
            val software = softwareStore()
            if (software?.keyExists() == true) {
                return@runOnMain software.seal(plaintext, aad)
            }
            val keystore = backend.seal(plaintext, aad)
            if (
                keystore.first() == CREDENTIAL_STATUS_OK ||
                    keystore.first() == CREDENTIAL_STATUS_MALFORMED
            ) {
                return@runOnMain keystore
            }
            val fallback = software?.seal(plaintext, aad) ?: return@runOnMain keystore
            if (fallback.first() == CREDENTIAL_STATUS_OK) fallback else keystore
        }

    @JvmStatic
    fun open(packet: ByteArray, aad: ByteArray): ByteArray =
        runOnMain {
            val software = softwareStore()
            credentialOpenWithFallback(
                {
                    software?.open(packet, aad)
                        ?: credentialResponse(CREDENTIAL_STATUS_KEY_MISSING)
                },
                { backend.open(packet, aad) },
                { legacyBackend.open(packet, aad) },
            )
        }

    @JvmStatic
    fun deleteKey(): ByteArray =
        runOnMain {
            val software = softwareStore()?.deleteKey() ?: credentialResponse(CREDENTIAL_STATUS_OK)
            val keystore = backend.deleteKey()
            val legacy = legacyBackend.deleteKey()
            when {
                software.first() != CREDENTIAL_STATUS_OK -> software
                keystore.first() != CREDENTIAL_STATUS_OK -> keystore
                legacy.first() != CREDENTIAL_STATUS_OK ->
                    credentialResponse(
                        CREDENTIAL_STATUS_OK,
                        "legacy-key-cleanup-deferred".toByteArray(Charsets.UTF_8),
                    )
                else -> keystore
            }
        }

    @JvmStatic
    fun migrationComplete(): ByteArray =
        runOnMain {
            val software = softwareStore()
            if (software?.isMigrated() == true) {
                credentialResponse(CREDENTIAL_STATUS_OK, byteArrayOf(1))
            } else {
                val current = migrationMarker.keyExists()
                if (
                    current.first() != CREDENTIAL_STATUS_OK ||
                        current.contentEquals(
                            credentialResponse(CREDENTIAL_STATUS_OK, byteArrayOf(1))
                        )
                ) {
                    current
                } else {
                    legacyMigrationMarker.keyExists()
                }
            }
        }

    @JvmStatic
    fun markMigrationComplete(): ByteArray =
        runOnMain {
            val software = softwareStore()
            if (software?.keyExists() == true) {
                return@runOnMain software.markMigrated()
            }
            val keystore = migrationMarker.ensureKey()
            if (keystore.first() == CREDENTIAL_STATUS_OK) {
                return@runOnMain keystore
            }
            software?.markMigrated() ?: keystore
        }

    private fun softwareStore(): SoftwareAesCredentialStore? {
        val context = OpenLessAppContext.context ?: return null
        return SoftwareAesCredentialStore(File(context.filesDir, "OpenLess"))
    }

    private fun runOnMain(block: () -> ByteArray): ByteArray {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            return block()
        }
        val result = arrayOfNulls<ByteArray>(1)
        val error = arrayOfNulls<Throwable>(1)
        val latch = CountDownLatch(1)
        Handler(Looper.getMainLooper()).post {
            try {
                result[0] = block()
            } catch (thrown: Throwable) {
                error[0] = thrown
            } finally {
                latch.countDown()
            }
        }
        if (!latch.await(8, TimeUnit.SECONDS)) {
            return diagnosticResponse(
                CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE,
                TimeoutException("keystore-main-timeout"),
            )
        }
        error[0]?.let { thrown ->
            return diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, thrown)
        }
        return result[0] ?: credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
    }
}
