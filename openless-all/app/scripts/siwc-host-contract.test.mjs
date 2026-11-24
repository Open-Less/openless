import vm from 'node:vm';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (path) => readFileSync(new URL('../' + path, import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const commands = read('src-tauri/src/commands/siwc.rs');
assert(/app\s*\.shell\(\)\s*\.open\(launch\.authorization_url\(\),\s*None\)/.test(commands));
assert(commands.includes('siwc_authorization_lease(true)'));
assert(commands.includes('siwc_authorization_lease(false)'));
assert.equal((commands.match(/ensure_main_window/g) || []).length, 3);
const registry = read('src-tauri/src/lib.rs');
for (const name of ['siwc_status', 'siwc_models', 'siwc_action'])
  assert.equal((registry.match(new RegExp('commands::' + name + ',', 'g')) || []).length, 2);
const runtime = read('android/kotlin/OpenLessRuntimeService.kt');
for (const method of ['acquireAuthorization', 'releaseAuthorization'])
  assert(new RegExp('@androidx.annotation.Keep\\s+@JvmStatic fun ' + method).test(runtime));
// IME teardown now uses the same current-demand reconciliation as JNI releases;
// behavioral lease/IME overlap and stale-release coverage runs in Android instrumentation.
assert(runtime.includes('authorizationUsers = maxOf(0, authorizationUsers - 1)'));
assert(runtime.includes('imeRequested || authorizationUsers > 0'));
assert(runtime.includes('if (!runtimeRequestedLocked())'));
assert(runtime.includes('mainHandler.post'));
assert(runtime.includes('synchronized(demandLock)'));
assert(
  read('android/kotlin/OpenLessImeService.kt').includes(
    'OpenLessRuntimeService.setImeRequested(this, false)',
  ),
);
const assets = read('harmony/rust/src/siwc.rs');
assert(assets.includes('OH_Asset_Update'));
assert(assets.includes('blob(VERSION, previous)'));
assert(assets.includes('number(SYNC, 0)'));
assert(!assets.includes('__credential_writes'));
const vault = read('src-tauri/src/persistence/siwc.rs');
assert(vault.includes('com.openless.app.siwc'));
assert(vault.includes('entry("vault.v1")'));
assert(vault.includes('write_verified'));
assert(!read('src/lib/ipc/siwc.ts').includes('authorizationUrl'));
console.log(
  'SIWC host contracts: desktop/mobile commands, Android lease, local-only vaults and native CAS passed',
);

// Exercise the actual manifest merger on an existing generated service, without disk writes.
const mergeSource = read('scripts/merge-android-overlay-manifest.mjs')
  .replace(/^#!.*\n/, '')
  .replace(/^import .*;\n/gm, '')
  .replaceAll('import.meta.url', JSON.stringify('file:///fixture/merge.mjs'));
let manifest =
  '<manifest><application><service android:name=".OpenLessRuntimeService" android:exported="false" android:foregroundServiceType="specialUse"><property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE" android:value="ime_runtime" /></service></application></manifest>';
const merge = () =>
  vm.runInNewContext(mergeSource, {
    URL,
    fileURLToPath: () => '/fixture/AndroidManifest.xml',
    existsSync: () => true,
    readFileSync: () => manifest,
    writeFileSync: (_path, value) => {
      manifest = value;
    },
    console: { log() {}, error() {} },
    process: {
      argv: ['node', 'merge.mjs'],
      exit(code) {
        throw Error('merge exit ' + code);
      },
    },
  });
merge();
assert(manifest.includes('user-initiated ChatGPT authorization callback'));
assert(!manifest.includes('android:value="ime_runtime"'));
const updated = manifest;
merge();
assert.equal(manifest, updated);
console.log(
  'SIWC Android manifest: existing service purpose updated idempotently without microphone gating',
);
