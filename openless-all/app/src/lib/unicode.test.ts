import { countCodePoints } from './unicode';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

// 口径与后端 Rust `polished.chars().count()`（Unicode 标量值）一致：
// ASCII / CJK 按字计，emoji 与 CJK 扩展 B 等增补平面字符不得被 UTF-16 码元双算。
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
