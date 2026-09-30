// Counts characters by Unicode code point (scalar value).
//
// `String.prototype.length` counts UTF-16 code units, double-counting supplementary-plane
// characters like emoji / CJK Extension B; the backend Rust `polished.chars().count()`
// counts by Unicode scalar value, and the two must agree — otherwise the overview page's
// "char count" metric, the history detail page's "N chars", and the backend activity
// aggregation would each tell a different story.
// `Array.from(text).length` splits by code point (equal to the scalar-value count for valid
// UTF-16 text), matching Rust's `chars()`. Note this is a code-point count, not a grapheme
// cluster count — combining characters (e.g. e + U+0301) count as 2, consistent with the
// backend.
export function countCodePoints(text: string): number {
  return Array.from(text).length;
}
