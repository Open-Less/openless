// Contract test for the HarmonyOS release path.
//
// The HAP cannot be built on a hosted runner (the DevEco SDK is account-gated),
// so the risks this test guards are structural: the entry point must pin the tag,
// run on the SDK-bearing runner by default, keep every credential in Secrets, and
// hand the signed HAP to both the GitHub Release and AppGallery Connect.
// Text-based so it also runs in the Linux contract job (no node_modules).
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const workflow = readFileSync(join(appRoot, '../../.github/workflows/release-harmony.yml'), 'utf8');
const packaging = readFileSync(join(appRoot, 'scripts/package-harmony-hap.sh'), 'utf8');
const signing = readFileSync(join(appRoot, 'scripts/sign-harmony-hap.py'), 'utf8');
const upload = readFileSync(join(appRoot, 'scripts/upload-appgallery.mjs'), 'utf8');
const releasing = readFileSync(join(appRoot, '../../RELEASING.md'), 'utf8');

// Dispatch-only, with the tag as a required input: the HAP can never come from a
// branch or ride along with an unrelated push.
assert.match(workflow, /^on:\n  workflow_dispatch:/m);
assert.doesNotMatch(workflow, /^  (push|pull_request):/m);
assert.match(workflow, /inputs:[\s\S]*?tag:[\s\S]*?required: true/);

// The tag is what gets built.
assert.match(workflow, /ref: \$\{\{ inputs\.tag \}\}/);
assert.match(workflow, /TAG_VERSION=\$\{TAG_VERSION%-tauri\}/);
assert.match(workflow, /does not match package\.json version/);

// Default runner is a hosted one, with the SDK restored from a pinned tarball;
// the workflow explains the requirement instead of failing with a mystery error.
assert.match(workflow, /default: '\["ubuntu-latest"\]'/);
assert.match(workflow, /prepare-harmony-sdk\.sh/);
assert.match(workflow, /HARMONY_SDK_SHA256/);
assert.match(workflow, /no HarmonyOS SDK: set the HARMONY_SDK_URL/);
assert.match(readFileSync(join(appRoot, 'scripts/prepare-harmony-sdk.sh'), 'utf8'), /checksum mismatch/);
// No DevEco install: the standalone hvigor CLI must be usable.
assert.match(packaging, /@ohos\/hvigor\/bin\/hvigor\.js/);

// Credentials arrive as Secrets/Variables, never as literals.
for (const secret of [
  'HARMONY_KEYSTORE_BASE64',
  'HARMONY_CERT_BASE64',
  'HARMONY_PROFILE_BASE64',
  'HARMONY_KEYSTORE_PASSWORD',
  'HARMONY_KEY_ALIAS',
  'APPGALLERY_CLIENT_ID',
  'APPGALLERY_CLIENT_SECRET',
  'APPGALLERY_APP_ID',
]) {
  assert.ok(workflow.includes(secret), `release-harmony.yml must consume ${secret}`);
  assert.ok(releasing.includes(secret), `RELEASING.md must document ${secret}`);
}
assert.doesNotMatch(workflow, /password\s*:\s*['"][^'"]+['"]/i, 'no literal passwords in the workflow');

// Packaging and signing are separate, testable scripts.
assert.ok(existsSync(join(appRoot, 'scripts/package-harmony-hap.sh')));
assert.match(packaging, /sign-harmony-hap\.py/);
assert.match(packaging, /--release/);
assert.match(packaging, /sha256sum \.\/\*\.hap > SHA256SUMS/);
assert.match(packaging, /OpenLess_harmony_arm64\.hap/);
assert.match(packaging, /OpenLess_harmony\.hap/);
assert.match(signing, /HARMONY_KEYSTORE_PASSWORD/);
assert.match(signing, /\[redacted\]/);

// AppGallery upload: documented endpoint chain, dry-run, and a skip path.
assert.match(upload, /api\/oauth2\/v1\/token/);
assert.match(upload, /api\/publish\/v2\/upload-url\/for-obs/);
assert.match(upload, /api\/publish\/v3\/app-package-info/);
assert.match(upload, /--dry-run/);
assert.match(upload, /--skip-if-unconfigured/);
assert.match(workflow, /upload-appgallery\.mjs/);

// Both destinations are wired.
assert.match(workflow, /gh release upload/);
assert.match(workflow, /OpenLess_harmony_arm64\.hap/);

console.log('harmony-release contract test passed');
