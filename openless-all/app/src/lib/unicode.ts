// 按 Unicode 码点（标量值）计数字符数。
//
// `String.prototype.length` 按 UTF-16 码元计数，emoji / CJK 扩展 B 等增补平面字符
// 会被双算；后端 Rust `polished.chars().count()` 按 Unicode 标量值计数，两者必须对齐，
// 否则概览页「字数」指标、历史详情页「N 字」与后端 activity 聚合会各说各话。
// `Array.from(text).length` 按码点切分（对合法 UTF-16 文本即等于标量值数），与 Rust
// `chars()` 同口径。注意这是码点数、不是字素簇数——组合字符（如 e + U+0301）
// 会计成 2 个，与后端一致。
export function countCodePoints(text: string): number {
  return Array.from(text).length;
}
