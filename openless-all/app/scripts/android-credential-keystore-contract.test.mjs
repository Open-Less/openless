import { access, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const repoRoot = fileURLToPath(new URL('../../..', import.meta.url));

const paths = {
  cipher: new URL('../android/kotlin/OpenLessCredentialCipher.kt', import.meta.url),
  vault: new URL('../android/kotlin/OpenLessCredentialVault.kt', import.meta.url),
  unitTest: new URL(
    '../android/kotlin/test/OpenLessCredentialCipherTest.kt',
    import.meta.url,
  ),
  instrumentedTest: new URL(
    '../android/kotlin/androidTest/OpenLessCredentialVaultInstrumentedTest.kt',
    import.meta.url,
  ),
  rustStore: new URL(
    '../src-tauri/src/persistence/android_credentials.rs',
    import.meta.url,
  ),
  credentials: new URL('../src-tauri/src/persistence/credentials.rs', import.meta.url),
  jni: new URL('../src-tauri/src/android/jni.rs', import.meta.url),
  copyScript: new URL('./copy-android-scaffolding.mjs', import.meta.url),
  ci: new URL('../../../.github/workflows/ci.yml', import.meta.url),
};

function display(url) {
  return fileURLToPath(url).replace(`${repoRoot}/`, '');
}

async function requiredSource(name, url) {
  try {
    await access(url);
  } catch {
    throw new Error(`missing ${name}: ${display(url)}`);
  }
  return readFile(url, 'utf8');
}

function requirePattern(source, pattern, message) {
  if (!pattern.test(source)) {
    throw new Error(message);
  }
}

const [cipher, vault, unitTest, instrumentedTest, rustStore, credentials, jni, copyScript, ci] =
  await Promise.all([
    requiredSource('pure AES-GCM codec', paths.cipher),
    requiredSource('Android Keystore bridge', paths.vault),
    requiredSource('JVM cipher tests', paths.unitTest),
    requiredSource('Android Keystore instrumentation tests', paths.instrumentedTest),
    requiredSource('Rust Android credential store', paths.rustStore),
    requiredSource('credentials integration', paths.credentials),
    requiredSource('JNI bridge', paths.jni),
    requiredSource('Android scaffolding copier', paths.copyScript),
    requiredSource('PR CI workflow', paths.ci),
  ]);

requirePattern(cipher, /AES\/GCM\/NoPadding/, 'cipher must use AES/GCM/NoPadding');
requirePattern(cipher, /NONCE_BYTES\s*=\s*12/, 'cipher must require a 12-byte nonce');
requirePattern(cipher, /TAG_BITS\s*=\s*128/, 'cipher must use a 128-bit GCM tag');
requirePattern(cipher, /updateAAD/, 'cipher must authenticate caller-provided AAD');

for (const pattern of [
  /AndroidKeyStore/,
  /setBlockModes\([^)]*BLOCK_MODE_GCM/,
  /setEncryptionPaddings\([^)]*ENCRYPTION_PADDING_NONE/,
  /setKeySize\(256\)/,
  /setRandomizedEncryptionRequired\(true\)/,
  /fun\s+seal\s*\(/,
  /fun\s+open\s*\(/,
  /fun\s+deleteKey\s*\(/,
  /fun\s+migrationComplete\s*\(/,
  /fun\s+markMigrationComplete\s*\(/,
]) {
  requirePattern(vault, pattern, `Keystore bridge is missing ${pattern}`);
}
if (
  /catch\s*\([^:]+:\s*UnrecoverableKeyException\)\s*\{\s*credentialResponse\(CREDENTIAL_STATUS_KEY_MISSING\)/.test(
    vault,
  )
) {
  throw new Error('UnrecoverableKeyException must never trigger destructive key-missing cleanup');
}
for (const pattern of [
  /is\s+KeyPermanentlyInvalidatedException\s*->\s*CREDENTIAL_STATUS_KEY_MISSING/,
  /else\s*->\s*CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE/,
]) {
  requirePattern(vault, pattern, `Keystore failure classifier is missing ${pattern}`);
}
if (/\b(?:Log\.|println\s*\()/.test(vault)) {
  throw new Error('Keystore bridge must not log secret-bearing inputs or crypto exceptions');
}

for (const pattern of [
  /roundTrip/,
  /freshNonce/,
  /tamperedNonce/,
  /tamperedCiphertext/,
  /tamperedAad/,
  /unrecoverableKeyExceptionRemainsRetryable/,
]) {
  requirePattern(unitTest, pattern, `JVM crypto tests are missing ${pattern}`);
}
for (const pattern of [/assertNull\([^)]*\.encoded/, /deletedKey/, /tamperedCiphertext/]) {
  requirePattern(
    instrumentedTest,
    pattern,
    `Android Keystore instrumentation tests are missing ${pattern}`,
  );
}

for (const pattern of [
  /openless-android-credentials/,
  /version:\s*u32/,
  /account:\s*String/,
  /nonce:\s*String/,
  /ciphertext:\s*String/,
  /serde\(deny_unknown_fields\)/,
  /KeyMissingOrInvalidated/,
  /AuthenticationFailed/,
  /TemporarilyUnavailable/,
  /migration_complete/,
  /mark_migration_complete/,
  /recover_verified_sanitized_legacy/,
  /mode\(0o600\)/,
  /sync_all\(\)/,
]) {
  requirePattern(rustStore, pattern, `Rust v2 store is missing ${pattern}`);
}
if (/Stub:\s*base64 envelope/.test(credentials)) {
  throw new Error('legacy Base64 stub is still the Android credential writer');
}

for (const pattern of [
  /keystore_seal/,
  /keystore_open/,
  /keystore_delete_key/,
  /keystore_migration_complete/,
  /keystore_mark_migration_complete/,
  /JByteArray/,
]) {
  requirePattern(jni, pattern, `JNI bridge is missing ${pattern}`);
}

for (const file of [
  'OpenLessCredentialCipher.kt',
  'OpenLessCredentialVault.kt',
  'OpenLessCredentialCipherTest.kt',
  'OpenLessCredentialVaultInstrumentedTest.kt',
]) {
  if (!copyScript.includes(file)) {
    throw new Error(`Android scaffolding does not copy ${file}`);
  }
}
requirePattern(
  copyScript,
  /testInstrumentationRunner[\s\S]*androidx\.test\.runner\.AndroidJUnitRunner/,
  'generated Android project must declare the AndroidX instrumentation runner',
);

requirePattern(ci, /testDebugUnitTest/, 'PR CI must execute JVM credential tests');
requirePattern(ci, /assembleDebugAndroidTest/, 'PR CI must compile instrumentation tests');
requirePattern(
  ci,
  /connectedDebugAndroidTest/,
  'PR CI must execute Android Keystore instrumentation tests on a device',
);
requirePattern(
  rustStore,
  /successful_v2_rejects_legacy_base64_downgrade/,
  'Rust store must test that legacy migration closes after v2 succeeds',
);
requirePattern(
  credentials,
  /android_bearer_is_scrubbed_before_failed_keystore_migration_returns/,
  'credentials integration must preserve the fail-closed Marketplace bearer scrub',
);

console.log('android-credential-keystore-contract.test.mjs passed');
