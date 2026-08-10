import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const packageJson = JSON.parse(
  await readFile(new URL('../package.json', import.meta.url), 'utf8'),
);
const packageLock = JSON.parse(
  await readFile(new URL('../package-lock.json', import.meta.url), 'utf8'),
);
const lockRoot = packageLock.packages?.[''];

assert.ok(lockRoot, 'package-lock.json must contain the root package entry');
assert.deepEqual(
  lockRoot.dependencies,
  packageJson.dependencies,
  'package-lock root dependencies must match package.json',
);
assert.deepEqual(
  lockRoot.devDependencies,
  packageJson.devDependencies,
  'package-lock root devDependencies must match package.json',
);

const dompurifyTypes = packageLock.packages['node_modules/@types/dompurify'];
const trustedTypes = packageLock.packages['node_modules/@types/trusted-types'];

assert.equal(dompurifyTypes?.dev, true, '@types/dompurify must remain development-only');
assert.equal(
  Boolean(trustedTypes?.dev || trustedTypes?.devOptional),
  true,
  '@types/trusted-types must not be classified as a production-only dependency',
);

console.log('package-lock-root-contract.test.mjs passed');
