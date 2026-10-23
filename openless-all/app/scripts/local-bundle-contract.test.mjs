import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const read = (path) => readFileSync(join(appRoot, path), 'utf8');
const migrationActivity = read('android/kotlin/OpenLessLocalMigrationActivity.kt');
const keyboardActivity = read('android/kotlin/OpenLessKeyboardSettingsActivity.kt');
const native = read('android/kotlin/OpenLessNative.kt');
const policy = read('android/kotlin/OpenLessMigrationBundlePolicy.kt');
const container = read('crates/openless-core/src/local_migration_bundle/container.rs');
const bridge = read('src-tauri/src/android/native_bridge.rs');
const scaffolding = read('scripts/copy-android-scaffolding.mjs');
const manifestMerge = read('scripts/merge-android-overlay-manifest.mjs');
const privacyTabs = read('src/pages/settings/tabs.tsx');
const migrationSection = read('src/pages/settings/LocalMigrationSection.tsx');
const migrationIpc = read('src/lib/ipc/local-migration.ts');
const permissionsCommands = read('src-tauri/src/commands/permissions_cmds.rs');
const mobileHandler = read('src-tauri/src/lib.rs');
const application = read('android/kotlin/OpenLessApplication.kt');
const settingsExport = read('android/kotlin/OpenLessSettingsExport.kt');

assert.match(policy, /openless\.android\.migration-bundle/);
assert.match(migrationActivity, /nativeExportLocalMigrationBundle/);
assert.match(migrationActivity, /nativeInspectLocalMigrationBundle/);
assert.match(migrationActivity, /nativePreviewLocalMigrationBundle/);
assert.match(migrationActivity, /nativeImportLocalMigrationBundle/);
assert.match(migrationActivity, /launchLegacyImportPicker/);
assert.match(migrationActivity, /showLegacyImportWarning/);
assert.match(migrationActivity, /OpenLessSettingsExport\.import\(/);
assert.match(migrationActivity, /OpenLessContentWriter\.writeBytes/);
assert.match(migrationActivity, /RadioGroup/);
assert.match(migrationActivity, /RadioButton/);
assert.match(migrationActivity, /checkedRadioButtonId/);
assert.doesNotMatch(migrationActivity, /setSingleChoiceItems/);
assert.match(migrationActivity, /migration_bundle_stale_preview/);
assert.match(
  migrationActivity,
  /previewMigration\(bundleBase64, password, mode, allowStaleRetry = false\)/,
);
assert.match(migrationActivity, /observedGeneration.*isNotBlank/);
assert.match(migrationActivity, /observedAndroidRevision.*isNotBlank/);
assert.doesNotMatch(
  migrationActivity,
  /nativeExtractLocalMigrationAndroidPreferences|importPortable/,
);
assert.match(scaffolding, /OpenLessMigrationState\.kt/);
assert.match(migrationActivity, /documents\.json/);
assert.doesNotMatch(migrationActivity, /pendingExportJson/);
assert.doesNotMatch(
  keyboardActivity,
  /nativeExportLocalMigrationBundle|nativePreviewLocalMigrationBundle|nativeImportLocalMigrationBundle|本地迁移 \/ 分享/,
);
assert.doesNotMatch(keyboardActivity, /showExportCategoryDialog|launchImportPicker/);
assert.match(keyboardActivity, /nativeAsrUsage/);
assert.doesNotMatch(keyboardActivity, /LegacyPinyinBackup/);
// The keyboard page exports its own learning data, separately from the migration bundle.
const keyboardData = read('android/kotlin/KeyboardDataFile.kt');
assert.match(keyboardActivity, /KeyboardDataFile\.export\(/);
assert.match(keyboardActivity, /KeyboardDataFile\.import\(/);
assert.match(keyboardData, /OpenLessRime\.exportUserDictionary/);
assert.match(keyboardData, /OpenLessRime\.importUserDictionary/);
assert.doesNotMatch(keyboardData, /nativeExportLocalMigrationBundle|OpenLessSettingsExport/);
assert.doesNotMatch(settingsExport, /LitePinyinLearnedPhrases/);
// Bundle schema 1 keeps the field, always empty now that Rime replaced LitePinyin.
assert.match(settingsExport, /put\("pinyinLearnedPhrases", JSONObject\(\)\)/);
assert.equal(existsSync(join(appRoot, 'android/kotlin/LegacyPinyinBackup.kt')), false);
assert.equal(existsSync(join(appRoot, 'android/kotlin/LitePinyinLearnedPhrases.kt')), false);
assert.match(scaffolding, /OpenLessLocalMigrationActivity\.kt/);
assert.match(native, /nativeExportLocalMigrationBundle/);
assert.match(native, /nativePreviewLocalMigrationBundle/);
assert.match(native, /nativeImportLocalMigrationBundle/);
assert.match(container, /BundleMode::ShareRedacted/);
assert.match(container, /CredentialPolicy::PasswordEncrypted/);
assert.match(container, /file\.encrypted\(\)/);
assert.match(container, /safe_entry_name/);
assert.match(bridge, /publish_local_migration_state_changed/);
assert.match(bridge, /missing expected generation/);
assert.match(scaffolding, /OpenLessMigrationBundlePolicy\.kt/);
assert.match(scaffolding, /OpenLessMigrationBundleContractTest\.kt/);
assert.match(scaffolding, /OpenLessMigrationBundleInstrumentedTest\.kt/);
assert.match(scaffolding, /RimeEngineIntegrationTest\.kt/);

assert.match(manifestMerge, /OpenLessLocalMigrationActivity/);
assert.match(manifestMerge, /android:exported="false"/);
assert.match(privacyTabs, /<LocalMigrationSection \/>/);
assert.match(migrationSection, /openAndroidLocalMigration/);
assert.match(migrationSection, /settings\.localMigration/);
assert.match(migrationIpc, /open_android_local_migration/);
assert.match(permissionsCommands, /open_android_local_migration/);
assert.match(permissionsCommands, /OpenLessLocalMigrationActivity/);
assert.match(mobileHandler, /commands::open_android_local_migration/);
assert.match(application, /OpenLessLocalMigrationActivity/);
assert.match(application, /!localMigrationStarted/);

console.log('local migration bundle contract passed');
