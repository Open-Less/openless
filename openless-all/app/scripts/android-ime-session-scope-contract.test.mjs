import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const text = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const bridge = text('../../android/kotlin/OpenLessOverlayBridge.kt');
const host = text('../../android/kotlin/OpenLessImeService.kt');
const native = text('../src-tauri/src/android/native_bridge.rs');
const jni = text('../src-tauri/src/android/jni.rs');
const copy = text('../../android/scripts/copy-android-scaffolding.mjs');

// These are ABI and source-wiring checks, not substitutes for scoped frame
// model tests, Android View behavior tests, or a full matching-native build.
test('session metadata preserves the legacy callback ABI', () => {
  assert.match(
    bridge,
    /fun onCapsuleStateChanged\(state: String, message: String\?, level: Float\)/,
  );
  assert.match(bridge, /onCapsuleStateChangedWithSession\(state, message, level, null\)/);
  assert.ok(jni.includes('"(Ljava/lang/String;Ljava/lang/String;F)V"'));
  assert.ok(jni.includes('"(Ljava/lang/String;Ljava/lang/String;FLjava/lang/String;)V"'));
  assert.ok(jni.includes('"onCapsuleStateChangedWithSession"'));
});

test('the Core payload identity reaches the Android bridge without snapshot guessing', () => {
  const outlet = native.slice(
    native.indexOf('pub fn notify_capsule_state('),
    native.indexOf('pub fn show_overlay('),
  );
  assert.match(outlet, /notify_overlay_bridge_with_session\(/);
  assert.match(outlet, /payload\.session_id\.as_deref\(\)/);
  assert.doesNotMatch(outlet, /backend\.snapshot\(|CORE_BACKEND\.get\(/);
});

test('the IME accepts owned frames only after a verified started binding', () => {
  assert.match(host, /imeFrameGate\.bind\(owner, identity\)/);
  assert.match(host, /imeFrameGate\.isBound\(owner, identity\)/);
  assert.match(host, /when \(imeFrameGate\.accept\(frame\)\)/);
  assert.match(host, /ImeFrameDisposition\.OWNED -> \{/);
  assert.match(host, /ImeFrameDisposition\.UNIDENTIFIED, ImeFrameDisposition\.STALE -> Unit/);
  assert.match(host, /imeFrameGate\.finish\(activeImeOwner\)/);
});

test('the scoped model and compatibility fixture are staged for testing', () => {
  for (const name of [
    'ImeScopedFrameGate.kt',
    'ImeScopedFrameGateTest.kt',
    'ImeBridgeCompatibilityInstrumentedTest.kt',
    'ImeSessionScopeInstrumentedTest.kt',
  ]) {
    assert.ok(copy.includes(`'${name}'`), `${name} must reach the generated Android project`);
  }
  const fixture = text(
    '../../android/kotlin/androidTest/ImeBridgeCompatibilityInstrumentedTest.kt',
  );
  assert.match(fixture, /@RunWith\(AndroidJUnit4::class\)/);
});
