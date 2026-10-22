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
import vm from 'node:vm';

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

// Default runner is a hosted one and the toolchain comes from the public mirror
// that `prepare-harmony-sdk.sh` pins by checksum - no secrets, no self-hosted box.
assert.match(workflow, /default: '\["ubuntu-latest"\]'/);
assert.match(workflow, /prepare-harmony-sdk\.sh/);
const prepare = readFileSync(join(appRoot, 'scripts/prepare-harmony-sdk.sh'), 'utf8');
assert.match(prepare, /DEFAULT_URL=https:\/\//);
assert.match(prepare, /DEFAULT_SHA256=[0-9a-f]{64}/);
assert.match(prepare, /checksum mismatch/);
// No DevEco install: the standalone hvigor CLI must be usable.
assert.match(packaging, /@ohos\/hvigor\/bin\/hvigor\.js/);

// Distribution policy: the repository only ever carries the unsigned package,
// AppGallery gets the signed one, and the test channel signs with its own
// material.
// The default dispatch is the unsigned repository path: signing only happens for
// a store channel, and a plain release build must not need any secret.
assert.match(workflow, /channel:[\s\S]{0,600}default: none/);
assert.match(workflow, /options:[\s\S]{0,200}- none[\s\S]{0,80}- release[\s\S]{0,80}- test/);
assert.match(
  workflow,
  /Write the signing material for this channel\n\s+if: inputs\.channel != 'none'/,
  'the signing material step must be skipped for channel=none',
);
assert.match(workflow, /HARMONY_KEYSTORE_BASE64[\s\S]{0,400}prefix\.p12/);
assert.match(workflow, /OPENLESS_HARMONY_CHANNEL: \$\{\{ inputs\.channel \}\}/);
assert.match(workflow, /--channel "\$\{\{ inputs\.channel \}\}"/);
const attachStart = workflow.indexOf('gh release upload');
const releaseAttach = workflow.slice(
  attachStart,
  workflow.indexOf('attached HarmonyOS assets', attachStart),
);
assert.ok(!releaseAttach.includes('-signed.hap'), 'the Release must not carry the signed HAP');
assert.match(releaseAttach, /OpenLess_harmony_arm64\.hap/);
assert.match(packaging, /OpenLess_harmony_arm64-signed\.hap/);
assert.match(packaging, /OPENLESS_HARMONY_CHANNEL/);
assert.match(readFileSync(join(appRoot, 'scripts/sign-harmony-hap.py'), 'utf8'), /--prefix/);
assert.match(
  readFileSync(join(appRoot, 'scripts/verify-harmony-hap.sh'), 'utf8'),
  /--allow-unsigned/,
);

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
assert.doesNotMatch(
  workflow,
  /password\s*:\s*['"][^'"]+['"]/i,
  'no literal passwords in the workflow',
);

// Packaging and signing are separate, testable scripts.
assert.ok(existsSync(join(appRoot, 'scripts/package-harmony-hap.sh')));
assert.match(packaging, /sign-harmony-hap\.py/);
assert.match(packaging, /--prefix "\$SIGN_PREFIX"/);
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

// `env:` at job level outranks `$GITHUB_ENV`, so a placeholder key here would
// shadow the value the signing step exports and the build would look for
// signing material in `~/.local/share/harmony/signing/release`.
{
  const jobEnv = workflow.match(/\n    env:\n((?:      [A-Z_]+:.*\n)+)/)?.[1] ?? '';
  assert.doesNotMatch(
    jobEnv,
    /OPENLESS_HARMONY_SIGNING_DIR/,
    'release-harmony.yml must not declare OPENLESS_HARMONY_SIGNING_DIR as job-level env',
  );
  assert.match(
    workflow,
    /OPENLESS_HARMONY_SIGNING_DIR=\$dir.*GITHUB_ENV|echo "OPENLESS_HARMONY_SIGNING_DIR=\$dir" >> "\$GITHUB_ENV"/,
    'the signing step must export OPENLESS_HARMONY_SIGNING_DIR through $GITHUB_ENV',
  );
}

// The signing material must be validated before the build: a mismatched
// certificate or an expired profile would otherwise only surface after a
// six-minute build, or when a device refuses the install.
assert.match(
  workflow,
  /Validate the signing material before building[\s\S]*check-harmony-signing\.sh/,
  'release-harmony.yml must run check-harmony-signing.sh before building',
);
assert.ok(
  existsSync(join(appRoot, 'scripts/check-harmony-signing.sh')),
  'scripts/check-harmony-signing.sh must exist',
);

// Secret names carry the channel first (`HARMONY_RELEASE_CERT_BASE64`), so the
// resolver must insert the channel after `HARMONY_` rather than appending it.
assert.match(
  workflow,
  /specific="HARMONY_\$\{upper\}_\$\{base#HARMONY_\}"/,
  'channel-specific secret lookup must build HARMONY_<CHANNEL>_<FIELD>',
);
assert.match(
  workflow,
  /HARMONY_RELEASE_CERT_BASE64: \$\{\{ secrets\.HARMONY_RELEASE_CERT_BASE64 \}\}/,
  'the release channel secret must be wired into the job environment',
);

const fields = ['KEYSTORE_PASSWORD', 'KEY_PASSWORD'];
const passwordExpression = (field) => {
  const line = workflow.split('\n').find((line) => line.startsWith('      HARMONY_' + field + ':'));
  assert.ok(line, 'missing resolved ' + field);
  return line
    .slice(line.indexOf(':') + 1)
    .trim()
    .slice(3, -2)
    .trim();
};
for (const channel of ['release', 'test']) {
  const own = 'HARMONY_' + channel.toUpperCase() + '_';
  const other = 'HARMONY_' + (channel === 'release' ? 'TEST' : 'RELEASE') + '_';
  for (const mode of ['specific', 'generic', 'both', 'missing-key']) {
    const secrets = {};
    for (const prefix of ['HARMONY_', own, other])
      for (const field of fields) secrets[prefix + field] = '';
    for (const field of fields) {
      secrets[other + field] = 'wrong-channel-' + field;
      if (mode !== 'specific' && mode !== 'missing-key')
        secrets['HARMONY_' + field] = 'generic-' + field;
      if (mode !== 'generic' && !(mode === 'missing-key' && field === 'KEY_PASSWORD'))
        secrets[own + field] = channel + '-' + field;
      const actual = vm.runInNewContext(passwordExpression(field), {
        inputs: { channel },
        secrets,
      });
      const expected =
        mode === 'generic'
          ? 'generic-' + field
          : mode === 'missing-key' && field === 'KEY_PASSWORD'
            ? ''
            : channel + '-' + field;
      assert.equal(actual, expected, channel + '/' + mode + '/' + field);
    }
  }
}
for (const field of fields) {
  const secrets = Object.fromEntries(
    ['HARMONY_', 'HARMONY_RELEASE_', 'HARMONY_TEST_'].map((prefix) => [prefix + field, '']),
  );
  assert.equal(
    vm.runInNewContext(passwordExpression(field), { inputs: { channel: 'none' }, secrets }),
    '',
  );
}
// Workflow resolution changes, not the local environment-over-file contract.
assert.match(signing, /password = os\.environ\.get\('HARMONY_KEYSTORE_PASSWORD'/);
assert.match(signing, /key_password = key_password or password/);
console.log('harmony-release contract test passed');
