package com.openless.android;

import android.content.Context;
import android.content.SharedPreferences;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;

import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.security.SecureRandom;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

final class SecureValueStore {
    private static final String PREFS = "openless_secure_values";
    private static final String KEY_ALIAS = "openless.credentials.v1";
    private static final String PREFIX = "v1:";
    private static final int GCM_TAG_BITS = 128;
    private static final int IV_BYTES = 12;

    private final SharedPreferences prefs;

    SecureValueStore(Context context) {
        prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    String get(String key, String fallbackPlaintext) {
        String stored = prefs.getString(key, "");
        if (stored == null || stored.isEmpty()) {
            return fallbackPlaintext == null ? "" : fallbackPlaintext;
        }
        if (!stored.startsWith(PREFIX)) {
            return stored;
        }
        try {
            return decrypt(stored.substring(PREFIX.length()));
        } catch (Exception e) {
            return fallbackPlaintext == null ? "" : fallbackPlaintext;
        }
    }

    void put(String key, String value) {
        String normalized = value == null ? "" : value;
        if (normalized.isEmpty()) {
            prefs.edit().remove(key).apply();
            return;
        }
        try {
            prefs.edit().putString(key, PREFIX + encrypt(normalized)).apply();
        } catch (Exception e) {
            // Do not silently downgrade credentials to plaintext storage.
            prefs.edit().remove(key).apply();
        }
    }

    private String encrypt(String value) throws Exception {
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        byte[] iv = new byte[IV_BYTES];
        new SecureRandom().nextBytes(iv);
        cipher.init(Cipher.ENCRYPT_MODE, getOrCreateKey(), new GCMParameterSpec(GCM_TAG_BITS, iv));
        byte[] ciphertext = cipher.doFinal(value.getBytes(StandardCharsets.UTF_8));
        byte[] combined = new byte[iv.length + ciphertext.length];
        System.arraycopy(iv, 0, combined, 0, iv.length);
        System.arraycopy(ciphertext, 0, combined, iv.length, ciphertext.length);
        return Base64.encodeToString(combined, Base64.NO_WRAP);
    }

    private String decrypt(String encoded) throws Exception {
        byte[] combined = Base64.decode(encoded, Base64.NO_WRAP);
        if (combined.length <= IV_BYTES) {
            return "";
        }
        byte[] iv = new byte[IV_BYTES];
        byte[] ciphertext = new byte[combined.length - IV_BYTES];
        System.arraycopy(combined, 0, iv, 0, IV_BYTES);
        System.arraycopy(combined, IV_BYTES, ciphertext, 0, ciphertext.length);
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, getOrCreateKey(), new GCMParameterSpec(GCM_TAG_BITS, iv));
        return new String(cipher.doFinal(ciphertext), StandardCharsets.UTF_8);
    }

    private SecretKey getOrCreateKey() throws Exception {
        KeyStore keyStore = KeyStore.getInstance("AndroidKeyStore");
        keyStore.load(null);
        if (keyStore.containsAlias(KEY_ALIAS)) {
            return (SecretKey) keyStore.getKey(KEY_ALIAS, null);
        }
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        KeyGenParameterSpec spec = new KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .build();
        generator.init(spec);
        return generator.generateKey();
    }
}
