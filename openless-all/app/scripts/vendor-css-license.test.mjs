import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const notice = readFileSync(
  new URL('../public/licenses/shadcn-LICENSE.txt', import.meta.url),
  'utf8',
);
assert.match(notice, /Copyright \(c\) 2023 shadcn/);
assert.equal(
  readFileSync(new URL('../dist/licenses/shadcn-LICENSE.txt', import.meta.url), 'utf8'),
  notice,
  'frontend distributions must carry the vendored CSS license',
);
console.log('vendored CSS license is present in the built distribution');
