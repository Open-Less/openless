import {
  DEFAULT_CAPSULE_SENSITIVITY,
  MIN_CAPSULE_SENSITIVITY,
  MAX_CAPSULE_SENSITIVITY,
  clampCapsuleSensitivity,
  mapVoiceLevel,
} from './capsuleSensitivity.ts';

function assertEqual<T>(actual: T, expected: T, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

function assertTrue(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message);
  }
}

// 1. Defaults and clamping
assertEqual(clampCapsuleSensitivity(undefined), DEFAULT_CAPSULE_SENSITIVITY, 'undefined -> default');
assertEqual(clampCapsuleSensitivity(null), DEFAULT_CAPSULE_SENSITIVITY, 'null -> default');
assertEqual(clampCapsuleSensitivity(NaN), DEFAULT_CAPSULE_SENSITIVITY, 'NaN -> default');
assertEqual(clampCapsuleSensitivity(0.1), MIN_CAPSULE_SENSITIVITY, '0.1 -> min (0.5)');
assertEqual(clampCapsuleSensitivity(5.0), MAX_CAPSULE_SENSITIVITY, '5.0 -> max (2.5)');
assertEqual(clampCapsuleSensitivity(1.44), 1.4, '1.44 -> 1.4 step clamp');
assertEqual(clampCapsuleSensitivity(2.0), 2.0, '2.0 -> 2.0');

// 2. Fan noise suppression below gate
// Laptop fan noise typically hovers around 0.005..0.012 RMS.
// Regardless of sensitivity, pure idle noise must produce strictly 0 so the wave stays calm.
for (const sens of [0.5, 1.0, 1.8, 2.5]) {
  assertEqual(mapVoiceLevel(0, sens), 0, `sens=${sens} raw=0`);
  assertEqual(mapVoiceLevel(0.005, sens), 0, `sens=${sens} raw=0.005`);
  assertEqual(mapVoiceLevel(0.011, sens), 0, `sens=${sens} raw=0.011`);
  assertEqual(mapVoiceLevel(0.012, sens), 0, `sens=${sens} raw=0.012`);
}

// 3. Sensitivity to soft voice above gate
// A quiet voice (e.g. 0.035..0.06) must produce a prominent, lively visual level (> 0.25 at default, > 0.45 at high sens)
const softVoice = 0.04;
const levelDefault = mapVoiceLevel(softVoice, 1.0);
const levelHigh = mapVoiceLevel(softVoice, 2.0);
const levelLow = mapVoiceLevel(softVoice, 0.6);

assertTrue(
  levelDefault >= 0.28,
  `default sensitivity should make soft voice (0.04) lively, got ${levelDefault}`,
);
assertTrue(
  levelHigh > levelDefault,
  `higher sensitivity should increase visual response (${levelHigh} > ${levelDefault})`,
);
assertTrue(
  levelLow < levelDefault,
  `lower sensitivity should decrease visual response (${levelLow} < ${levelDefault})`,
);

// 4. Monotonicity and bounds
let prev = 0;
for (let raw = 0; raw <= 1.0; raw += 0.01) {
  const v = mapVoiceLevel(raw, 1.5);
  assertTrue(v >= 0 && v <= 1.0, `value must stay in [0, 1], got ${v} at raw=${raw}`);
  assertTrue(v >= prev, `mapping must be monotonic: prev=${prev}, cur=${v} at raw=${raw}`);
  prev = v;
}

// 5. Loud voice gracefully caps at 1.0
assertEqual(mapVoiceLevel(0.8, 2.0), 1.0, 'loud voice cap at 2.0 sens');
assertEqual(mapVoiceLevel(1.0, 1.0), 1.0, 'loud voice cap at 1.0 sens');

console.log('capsuleSensitivity tests passed');
