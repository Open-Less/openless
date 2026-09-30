import { countCodePoints } from './unicode';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

// Matches the backend Rust `polished.chars().count()` convention (Unicode scalar values):
// ASCII / CJK count per character; emoji and supplementary-plane chars such as CJK Extension B
// must not be double-counted as UTF-16 code units.
assert(countCodePoints('') === 0, 'empty string should count 0');
assert(countCodePoints('hello') === 5, 'ASCII code points');
assert(countCodePoints('你好，世界') === 5, 'CJK code points');
assert(countCodePoints('😀') === 1, 'emoji surrogate pair must count as 1, not 2');
assert(countCodePoints('😀a') === 2, 'emoji + ASCII');
assert(countCodePoints('𠮷') === 1, 'CJK Extension B (surrogate pair) must count as 1');
assert(
  countCodePoints('e\u0301') === 2,
  'combining marks count per code point, matching Rust chars()',
);
