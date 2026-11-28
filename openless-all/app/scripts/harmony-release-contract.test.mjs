// Contract test for the HarmonyOS release path.
//
// Hosted runners obtain checksum-pinned Harmony tools. This contract guards the
// native build entry, immutable tag source, draft-only asset writes, and signing
// separation: unsigned HAPs go to GitHub; signed App Packs go only to AppGallery.
// Text-based so it also runs in the Linux contract job (no node_modules).
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const workflow = readFileSync(join(appRoot, '../../.github/workflows/release-harmony.yml'), 'utf8');
const packaging = readFileSync(join(appRoot, '../harmony/scripts/package-harmony-hap.sh'), 'utf8');
const signing = readFileSync(join(appRoot, '../harmony/scripts/sign-harmony-hap.py'), 'utf8');
const upload = readFileSync(join(appRoot, '../harmony/scripts/upload-appgallery.mjs'), 'utf8');
const releasing = readFileSync(join(appRoot, '../../RELEASING.md'), 'utf8');
const tauriRelease = readFileSync(
  join(appRoot, '../../.github/workflows/release-tauri.yml'),
  'utf8',
);

// Reusable and manual entry points require a tag; no independent push trigger
// may create a second HAP build outside the unified release.
assert.match(workflow, /^  workflow_dispatch:/m);
assert.doesNotMatch(workflow, /^  (push|pull_request):/m);
assert.match(workflow, /inputs:[\s\S]*?tag:[\s\S]*?required: true/);

// Automatic calls pin the triggering commit; manual repairs choose the input tag.
assert.match(
  workflow,
  /ref: \$\{\{ github\.event_name == 'push' && github\.sha \|\| inputs\.tag \}\}/,
);
assert.match(workflow, /TAG_VERSION=\$\{TAG_VERSION%-tauri\}/);
assert.match(workflow, /does not match package\.json version/);

// Default runner is a hosted one and the toolchain comes from the public mirror
// that `prepare-harmony-sdk.sh` pins by checksum - no secrets, no self-hosted box.
assert.match(workflow, /default: '\["ubuntu-latest"\]'/);
assert.match(workflow, /prepare-harmony-sdk\.sh/);

// The product targets HarmonyOS 7 (API 26) while staying installable on 6.1
// (API 23). Huawei maps 26.0.0 -> HarmonyOS 7.0 and 6.1.0 -> API 23, so the
// compiled HAP must keep that exact triple: a lower compile version drops the
// API 26 material APIs, a higher compatible one cuts off 6.1 devices.
{
  const profile = JSON.parse(readFileSync(join(appRoot, '../harmony/build-profile.json5'), 'utf8'));
  const product = profile.app.products.find((entry) => entry.name === 'default');
  assert.ok(product, 'build-profile.json5 must keep the default product');
  assert.equal(product.runtimeOS, 'HarmonyOS');
  assert.equal(product.compatibleSdkVersion, '6.1.0(23)');
  assert.equal(product.compileSdkVersion, '26.0.0');
  assert.equal(product.targetSdkVersion, '26.0.0');
  // compileSdkVersion drives the API surface; targetSdkVersion must not exceed
  // it or the HAP claims behaviour the compiler never checked.
  const minor = (value) => Number(/^(\d+)\./.exec(value)?.[1] ?? -1);
  assert.equal(minor(product.compileSdkVersion), 26);
  assert.equal(minor(product.targetSdkVersion), 26);
  assert.equal(/^(\d+)\.(\d+)\.(\d+)\((\d+)\)$/.exec(product.compatibleSdkVersion)?.[4], '23');
}

// Node dependencies support tag tooling/contract tests; the native HAP does not
// build a React bundle. Both CI and release must install the Tier-2 OHOS target
// before compiling the shared Rust host.
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
const prepare = readFileSync(join(appRoot, '../harmony/scripts/prepare-harmony-sdk.sh'), 'utf8');
// The toolchain default is the pinned API 26 (HarmonyOS 7) release, shipped as
// two ordered shards; the digest covers the concatenation.
assert.match(prepare, /DEFAULT_PARTS="https:\/\/[^"]*\.tar\.gz\.aa https:\/\/[^"]*\.tar\.gz\.ab"/);
assert.match(prepare, /DEFAULT_SHA256=[0-9a-f]{64}/);
assert.match(prepare, /HARMONY_MIN_API:-26/);
assert.match(prepare, /below the required API \$min_api/);
assert.match(prepare, /checksum mismatch/);
assert.doesNotMatch(prepare, /releases\/latest/);
// No DevEco install: the standalone hvigor CLI must be usable.
assert.match(packaging, /@ohos\/hvigor\/bin\/hvigor\.js/);
const nativeBuild = readFileSync(join(appRoot, '../harmony/scripts/build-harmony-app.sh'), 'utf8');
assert.match(packaging, /bash "\$HARMONY_ROOT\/scripts\/build-harmony-app\.sh"/);
assert.match(nativeBuild, /bash "\$HARMONY_ROOT\/scripts\/build-harmony-backend\.sh"/);
// Source versions are placeholders; packaging injects and restores the release identity.
const sourceApp = JSON.parse(
  readFileSync(join(appRoot, '../harmony/AppScope/app.json5'), 'utf8'),
).app;
assert.equal(sourceApp.versionCode, 1);
assert.equal(sourceApp.versionName, '0.1.0');
assert.match(nativeBuild, /trap restore_app_json EXIT/);
assert.doesNotMatch(nativeBuild, /npm run build|web-dist|dist\/index\.html/);
assert.ok(
  !existsSync(join(appRoot, 'scripts/build-harmony-app.sh')),
  'no retired app-local builder',
);
assert.ok(
  !existsSync(join(appRoot, '../harmony/scripts/build-harmony-frontend.sh')),
  'no second frontend builder',
);
// Reused Hvigor staging must discard deleted modules and obsolete web assets.
assert.match(nativeBuild, /shutil\.rmtree\(staged_ets\)/);
assert.match(nativeBuild, /shutil\.rmtree\(stale_web\)/);
const verify = readFileSync(join(appRoot, '../harmony/scripts/verify-harmony-hap.sh'), 'utf8');
assert.match(verify, /require_entry 'libs\/arm64-v8a\/libopenless_harmony_host\.so'/);
assert.match(verify, /HAP still carries the removed ArkWeb bundle/);

// Both manual and automatic publication are draft-only. Check at the beginning
// and again immediately before writing, so publishing during a build is safe.
const preflightIndex = workflow.indexOf('- name: Require a draft before publishing');
const buildIndex = workflow.indexOf('- name: Build, sign and stage the HAP');
assert.ok(preflightIndex > 0 && preflightIndex < buildIndex);
const attachStep = workflow.slice(
  workflow.indexOf('- name: Attach the HAP to the GitHub Release'),
  workflow.indexOf('- name: Upload to HUAWEI AppGallery Connect'),
);
for (const guard of [
  workflow.slice(preflightIndex, workflow.indexOf('- uses: actions/setup-java@v4')),
  attachStep,
]) {
  assert.match(guard, /if: inputs\.publish/);
  assert.match(guard, /if \[ "\$draft" != true \]; then/);
  assert.doesNotMatch(guard, /github\.event_name/, 'manual dispatch cannot bypass the draft guard');
}
assert.match(attachStep, /exit 1[\s\S]*gh release upload/);

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
assert.match(
  readFileSync(join(appRoot, '../harmony/scripts/sign-harmony-hap.py'), 'utf8'),
  /--prefix/,
);
assert.match(
  readFileSync(join(appRoot, '../harmony/scripts/verify-harmony-hap.sh'), 'utf8'),
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
assert.ok(existsSync(join(appRoot, '../harmony/scripts/package-harmony-hap.sh')));
assert.match(packaging, /sign-harmony-hap\.py/);
assert.match(packaging, /--prefix "\$SIGN_PREFIX"/);
assert.match(
  packaging,
  /sha256sum \.\/OpenLess_harmony_arm64\.hap > SHA256SUMS/,
  'Release checksums must cover the single unsigned HAP uploaded to GitHub',
);
assert.match(packaging, /OpenLess_harmony_arm64\.hap/);
assert.doesNotMatch(packaging, /OpenLess_harmony\.hap/);
assert.doesNotMatch(workflow, /OpenLess_harmony\.hap/);
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
// `# ... node ../harmony/scripts/upload-appgallery.mjs \` and then `--channel` on its own
// line: `command not found` (127) - the first time the step ever executed.
assert.doesNotMatch(
  workflow,
  /run:\s*>-[ ]*\n\s*#/,
  'a folded run scalar must not start with a comment; it comments out the command',
);
assert.match(
  workflow,
  /if \[ -f \.\.\/harmony\/scripts\/upload-appgallery\.mjs \]; then\n\s+upload_script=\.\.\/harmony\/scripts\/upload-appgallery\.mjs\n\s*elif \[ -f scripts\/upload-appgallery\.mjs \]; then\n\s+upload_script=scripts\/upload-appgallery\.mjs\n\s*else\n\s+echo "::error::Missing source input:[^\n]+\n\s+exit 1\n\s*fi\n\s+node "\$upload_script" \\/,
  'upload must execute the guarded source-tree script in the literal run block',
);

// Both destinations are wired.
assert.match(workflow, /gh release upload/);
assert.match(workflow, /OpenLess_harmony_arm64\.hap/);

// Reusable workflows stay in the same run and resolve at the caller's immutable
// commit; Harmony must not wait for desktop build/publication before starting.
assert.match(workflow, /^  workflow_call:$/m);
const harmonyCall = tauriRelease.slice(tauriRelease.indexOf('\n  harmony:'));
assert.match(harmonyCall, /uses: \.\/\.github\/workflows\/release-harmony\.yml/);
assert.match(harmonyCall, /needs: prepare-release/);
assert.doesNotMatch(harmonyCall, /needs:.*(?:build|publish|android)/);
assert.match(harmonyCall, /tag: \$\{\{ github\.ref_name \}\}/);
assert.match(harmonyCall, /publish: true/);
assert.match(harmonyCall, /secrets: inherit/);
assert.doesNotMatch(tauriRelease, /gh workflow run|dispatch-platform-releases/);
assert.doesNotMatch(
  tauriRelease,
  /release-linux-egui|LINUX_ACCEPTED_RELEASE_TAG/,
  'Linux automation is paused',
);
assert.match(
  tauriRelease,
  /github\.event_name == 'push'[\s\S]{0,120}refs\/tags\/v/,
  'the hand-off must be tag-push only, so a branch or schedule run dispatches nothing',
);
assert.doesNotMatch(tauriRelease, /actions: write/, 'same-run calls need no dispatch write token');
assert.match(
  harmonyCall,
  /channel: \$\{\{ vars\.HARMONY_AUTO_CHANNEL \|\| 'none' \}\}/,
  'the automatic call must default to the unsigned repository path',
);
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
  existsSync(join(appRoot, '../harmony/scripts/check-harmony-signing.sh')),
  '../harmony/scripts/check-harmony-signing.sh must exist',
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
