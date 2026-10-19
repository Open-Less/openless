import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolveReleaseCaskVersion } from './resolve-release-cask-version.mjs';

const files = (version) => [`OpenLess_${version}_aarch64.dmg`, `OpenLess_${version}_x64.dmg`];
for (const version of ['1.3.18', '2.0.0+build.20261015', '2.0.0+build.20261016-final']) {
  const base = version.split('+')[0];
  assert.equal(resolveReleaseCaskVersion(`v${base}-tauri`, files(version)), version);
}
for (const [tag, filenames] of [
  ['v2.0.0-Beta.6-tauri', files('2.0.0-Beta.6+build.20261015')],
  ['v2.0.0-tauri', files('2.0.1+build.20261015')],
  ['v2.0.0-tauri', files('2.0.0+')],
  ['v2.0.0-tauri', files('2.0.0+build.20261015+bad')],
  ['v2.0.0-tauri', files('2.0.0+build.20261015/bad')],
  ['v2.0.0-tauri', ['OpenLess_2.0.0_aarch64.dmg']],
  ['v2.0.0-tauri', [...files('2.0.0'), 'OpenLess_2.0.0+build.20261015_x64.dmg']],
  ['v2.0.0-tauri', ['OpenLess_2.0.0_aarch64.dmg', 'OpenLess_2.0.0+build.20261015_x64.dmg']],
  ['v2.0.0-tauri', ['Other_2.0.0_aarch64.dmg', 'OpenLess_2.0.0_x64.dmg']],
])
  assert.throws(() => resolveReleaseCaskVersion(tag, filenames));

// Expand the cask's actual Ruby interpolation contract, including its tag/base
// split. The old v#{version} tag fails this dated-version download assertion.
const cask = readFileSync(new URL('../../../Casks/openless.rb', import.meta.url), 'utf8');
const template = cask.match(/^  url "(.*)"$/m)?.[1];
assert.ok(template);
assert.match(cask, /depends_on macos: \(version\.major\.to_i >= 2 \? :sonoma : :monterey\)/);
const tauri = JSON.parse(
  readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
);
assert.equal(
  tauri.bundle.macOS.minimumSystemVersion,
  '14.0',
  'Update the 2.x cask minimum when the host minimum changes',
);
for (const version of ['1.3.18', '2.0.0+build.20261015']) {
  for (const arch of ['aarch64', 'x64']) {
    const url = template
      .replaceAll('#{version.to_s.split("+", 2).first}', version.split('+')[0])
      .replaceAll('#{version}', version)
      .replaceAll('#{arch}', arch);
    assert.equal(
      url,
      `https://github.com/Open-Less/openless/releases/download/v${version.split('+')[0]}-tauri/OpenLess_${version}_${arch}.dmg`,
    );
  }
}
const workflow = readFileSync(
  new URL('../../../.github/workflows/sync-homebrew-cask.yml', import.meta.url),
  'utf8',
);
assert.match(
  workflow,
  /version="\$\(node openless-all\/app\/scripts\/resolve-release-cask-version\.mjs "\$TAG" "\$RUNNER_TEMP\/dmg"\)"/,
);
const downloadPatterns = [...workflow.matchAll(/--pattern '([^']+)'/g)].map((match) => match[1]);
assert.equal(downloadPatterns.length, 2);
for (const version of ['1.3.18', '2.0.0+build.20261015']) {
  const selected = spawnSync(
    'bash',
    [
      '-c',
      'arm=$1; intel=$2; shift 2; for name; do if [[ "$name" == $arm || "$name" == $intel ]]; then printf "%s\\n" "$name"; fi; done',
      'download-patterns',
      ...downloadPatterns,
      ...files(version),
      'OpenLess_macos_aarch64.dmg',
      'OpenLess_macos_x86_64.dmg',
    ],
    { encoding: 'utf8' },
  );
  assert.equal(selected.status, 0, selected.stderr);
  const downloaded = selected.stdout.trim().split(/\r?\n/);
  assert.deepEqual(downloaded, files(version), 'Homebrew must only download versioned DMGs');
  assert.equal(resolveReleaseCaskVersion(`v${version.split('+')[0]}-tauri`, downloaded), version);
  const checksumNames = [...workflow.matchAll(/sha256sum "\$RUNNER_TEMP\/dmg\/([^"]+)"/g)].map(
    (match) => match[1].replaceAll('${version}', version),
  );
  assert.deepEqual(checksumNames, files(version), 'Homebrew must checksum the resolved version');
}
console.log('resolve-release-cask-version.test.mjs passed');
