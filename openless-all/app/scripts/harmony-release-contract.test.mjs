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
const tauriRelease = readFileSync(
  join(appRoot, '../../.github/workflows/release-tauri.yml'),
  'utf8',
);

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

// A hosted runner ships neither the app's `node_modules` (`build-harmony-frontend.sh`
// runs `npm run build`) nor the Tier-2 OHOS target std (`build-harmony-backend.sh`
// runs `cargo build --target aarch64-unknown-linux-ohos`). CI installs both; the
// first real dispatch of this workflow did not and died with `can't find crate for
// `core`` (error[E0463]) after the Rime/NDK compile.
assert.match(workflow, /dtolnay\/rust-toolchain@stable/);
assert.match(workflow, /targets: aarch64-unknown-linux-ohos/);
assert.match(workflow, /run: npm ci/);
{
  // Keep the target list in step with the CI job that really builds the HAP: a
  // target added to one workflow only breaks the tag build.
  const ciWorkflow = readFileSync(join(appRoot, '../../.github/workflows/ci.yml'), 'utf8');
  const ohosTargets = (source) =>
    [...source.matchAll(/targets:\s*([^\n]+)/g)]
      .map((match) => match[1].trim())
      .filter((targets) => targets.includes('ohos'));
  assert.ok(ohosTargets(workflow).length > 0, 'release-harmony.yml must install the OHOS target');
  assert.deepEqual(ohosTargets(workflow), ohosTargets(ciWorkflow));
}
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
assert.match(
  packaging,
  /sha256sum \.\/OpenLess_harmony_arm64\.hap \.\/OpenLess_harmony\.hap > SHA256SUMS/,
  'Release checksums must cover only the unsigned HAP pair uploaded to GitHub',
);
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
// The upload step must stay a literal `run: |` block. As a folded `>-` scalar its
// leading explanatory comment joined the command line, so bash saw
// `# ... node scripts/upload-appgallery.mjs \` and then `--channel` on its own
// line: `command not found` (127) - the first time the step ever executed.
assert.doesNotMatch(
  workflow,
  /run:\s*>-[ ]*\n\s*#/,
  'a folded run scalar must not start with a comment; it comments out the command',
);
assert.match(workflow, /node scripts\/upload-appgallery\.mjs \\/);

// Both destinations are wired.
assert.match(workflow, /gh release upload/);
assert.match(workflow, /OpenLess_harmony_arm64\.hap/);

// This entry point is dispatch-only, so the Tauri tag build has to hand the tag
// over explicitly. Without that hand-off the workflow sits at zero runs and every
// Release ships without a HAP - which is exactly what happened to 2.0.x.
assert.match(tauriRelease, /^  dispatch-platform-releases:$/m, 'release-tauri.yml must hand the tag to this workflow');
assert.match(
  tauriRelease,
  /gh workflow run release-harmony\.yml[\s\S]{0,300}-f "tag=\$RELEASE_TAG"/,
  'the hand-off must dispatch with the tag as an input',
);
assert.match(
  tauriRelease,
  /gh workflow run release-harmony\.yml[\s\S]{0,80}--ref "\$RELEASE_TAG"[\s\S]{0,180}-f "tag=\$RELEASE_TAG"/,
  'the downstream workflow definition must be loaded from the same immutable release tag',
);
assert.match(tauriRelease, /gh workflow run release-harmony\.yml[\s\S]{0,400}-f publish=true/);
assert.match(
  tauriRelease,
  /gh workflow run release-linux-egui\.yml[\s\S]{0,80}--ref "\$RELEASE_TAG"[\s\S]{0,180}-f "tag=\$RELEASE_TAG"/,
  'the Linux workflow definition must use the same immutable release tag',
);
assert.match(
  tauriRelease,
  /github\.event_name == 'push'[\s\S]{0,120}refs\/tags\/v/,
  'the hand-off must be tag-push only, so a branch or schedule run dispatches nothing',
);
assert.match(tauriRelease, /actions: write/, 'GITHUB_TOKEN needs actions: write to dispatch');
assert.match(
  tauriRelease,
  /HARMONY_CHANNEL: \$\{\{ vars\.HARMONY_AUTO_CHANNEL \|\| 'none' \}\}/,
  'the automatic hand-off must default to the unsigned repository path',
);
assert.match(tauriRelease, /channel=\$HARMONY_CHANNEL/);
assert.match(releasing, /HARMONY_AUTO_CHANNEL/);

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
