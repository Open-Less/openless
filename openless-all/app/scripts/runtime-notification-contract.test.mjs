import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { mergeRuntimeServiceDebugManifest } from './copy-android-scaffolding.mjs';

const read = (path) => readFileSync(new URL('../' + path, import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const copy = read('scripts/copy-android-scaffolding.mjs');
assert.match(copy, /const KOTLIN_ANDROID_TEST_FILES = \[[\s\S]*?'OpenLessRuntimeServiceInstrumentedTest\.kt'/);
const runtime = read('android/kotlin/OpenLessRuntimeService.kt');
assert(!runtime.includes('等待 ChatGPT 授权'));
assert(runtime.includes('后台运行服务已就绪'));
assert(runtime.includes('stopSelfResult(startId)'));
assert(!runtime.includes('context.stopService('));
const ime = read('android/kotlin/OpenLessImeService.kt');
assert(ime.includes('OpenLessRuntimeService.setImeRequested(this, true)'));
assert(ime.includes('OpenLessRuntimeService.setImeRequested(this, false)'));
assert(!ime.includes('OpenLessRuntimeService.imeRequested ='));
// JNI context teardown must remain available for settings, overlay and microphone IPC.
assert(runtime.includes('OpenLessNative.nativeUnregisterActivityContext(this)'));
assert(runtime.includes('OpenLessNative.nativeRegisterActivityContext(applicationContext)'));

for (const application of ['', '<application />', '<application><activity android:name=".MainActivity" /></application>']) {
  const manifest = `<manifest xmlns:android="http://schemas.android.com/apk/res/android">\n${application}\n</manifest>`;
  for (const source of [manifest, manifest.replace(/\n/g, '\r\n')]) {
    const merged = mergeRuntimeServiceDebugManifest(source);
    assert(merged.changed);
    assert.match(merged.content, /android:name="com\.openless\.app\.OpenLessRuntimeService"/);
    assert.match(merged.content, /android:foregroundServiceType="specialUse"/);
    for (const permission of ['FOREGROUND_SERVICE', 'FOREGROUND_SERVICE_SPECIAL_USE', 'POST_NOTIFICATIONS']) {
      assert(merged.content.includes(`android.permission.${permission}"`));
    }
    assert.doesNotMatch(merged.content, /OpenLessApplication|OpenLessOverlayService|OpenLessImeService/);
    assert.equal(mergeRuntimeServiceDebugManifest(merged.content).changed, false);
    assert.equal(mergeRuntimeServiceDebugManifest(merged.content).content, merged.content);
  }
}
console.log('Runtime notification contracts and isolated Android instrumentation manifest passed');
