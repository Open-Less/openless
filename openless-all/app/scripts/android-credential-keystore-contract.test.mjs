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
  /recover_verified_v2_temporary/,
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
requirePattern(
  jni,
  /fn\s+app_files_dir\s*\([\s\S]*?getFilesDir[\s\S]*?getAbsolutePath/,
  'JNI bridge must resolve the app-private Context files directory',
);
requirePattern(
  jni,
  /fn\s+with_tao_android_env[\s\S]*?main_android_context/,
  'startup persistence must use Tao\'s non-panicking Android context registry',
);
const androidCredentialPath = credentials.match(
  /fn\s+android_credentials_path\s*\([^)]*\)\s*->\s*Result<PathBuf>[\s\S]*?\r?\n}\r?\n/,
);
if (!androidCredentialPath) {
  throw new Error('missing Android credential path resolver');
}
requirePattern(
  androidCredentialPath[0],
  /app_files_dir\(\)[\s\S]*?join\("OpenLess"\)[\s\S]*?join\(ANDROID_CREDENTIALS_FILE\)/,
  'Android credentials must use the app-private files directory',
);
if (/TAURI_ANDROID_APP_DATA_DIR|temp_dir/.test(androidCredentialPath[0])) {
  throw new Error('Android credential storage must not fall back to environment or temporary storage');
}
for (const pattern of [
  /android_legacy_credentials_paths/,
  /load_android_credentials_from_source_with_crypto/,
  /remove_migrated_android_legacy_credentials/,
  /android_legacy_root_migrates_to_private_destination_and_is_erased/,
]) {
  requirePattern(credentials, pattern, `Android credential root migration is missing ${pattern}`);
}
requirePattern(
  rustStore,
  /let verified = open_envelope\(&persisted, crypto\)\?;[\s\S]*?if verified != plaintext \{[\s\S]*?\}[\s\S]*?mark_migration_complete\(\)[\s\S]*?fault\(WriteStage::AfterVerification\)/,
  'legacy downgrade barrier must be durable before the v2 envelope is installed',
);
requirePattern(
  rustStore,
  /verified_v2_commit_barrier_recovers_verified_temp_after_pre_rename_failure/,
  'Rust store must recover the verified pre-rename v2 candidate without accepting legacy',
);
requirePattern(
  rustStore,
  /invalidated_key_clears_pending_v2_recovery_candidate/,
  'Rust store must clear an unrecoverable pending v2 candidate so credentials can be reconfigured',
);

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

requirePattern(
  ci,
  /targets:\s*aarch64-linux-android,x86_64-linux-android/,
  'PR CI must install the Rust target used by the x86_64 emulator',
);
requirePattern(
  ci,
  /testX86_64DebugUnitTest/,
  'PR CI must execute JVM credential tests for the x86_64 flavor',
);
requirePattern(
  ci,
  /assembleX86_64DebugAndroidTest/,
  'PR CI must compile x86_64 instrumentation tests',
);
requirePattern(
  ci,
  /connectedX86_64DebugAndroidTest/,
  'PR CI must execute x86_64 Android Keystore instrumentation tests on a device',
);
if (
  /:app:(?:testDebugUnitTest|assembleDebugAndroidTest|connectedDebugAndroidTest)\b/.test(
    ci,
  )
) {
  throw new Error('PR CI must not use Android tasks without the required ABI flavor');
}
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
