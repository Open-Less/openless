import { normalizeAndroidOverlayTrigger } from '../../android/frontend/lib/androidTypes';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

assert(
  normalizeAndroidOverlayTrigger('never') === 'never',
  'never must remain disabled instead of being migrated to another trigger',
);
assert(
  normalizeAndroidOverlayTrigger('keyboard') === 'background',
  'legacy keyboard trigger must continue migrating to background',
);
assert(
  normalizeAndroidOverlayTrigger('always') === 'always',
  'always trigger must remain unchanged',
);

console.log('android overlay trigger tests passed');
