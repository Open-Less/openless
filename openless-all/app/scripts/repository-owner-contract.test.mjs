import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = fileURLToPath(new URL('../../..', import.meta.url));
const legacyRepo = 'appergb/openless';

const repositoryReferences = [
  '.github/ISSUE_TEMPLATE/config.yml',
  '.github/workflows/android-apk.yml',
  '.github/workflows/release-tauri.yml',
  'Casks/openless.rb',
  'README.md',
  'README.zh.md',
  'USAGE.md',
  'openless-all/app/src-tauri/src/android/updater_logic.rs',
  'openless-all/app/src-tauri/src/commands/mod.rs',
  'openless-all/app/src-tauri/src/commands/settings.rs',
  'openless-all/app/src-tauri/src/update_source.rs',
  'openless-all/app/src-tauri/tauri.conf.json',
  'openless-all/app/src/components/AutoUpdate.tsx',
  'openless-all/app/src/components/SettingsModal.tsx',
  'openless-all/app/src/pages/settings/AboutSection.tsx',
  'openless-all/app/scripts/write-updater-manifest.mjs',
];

for (const relativePath of repositoryReferences) {
  const content = await readFile(join(repoRoot, relativePath), 'utf8');
  assert(
    !content.includes(legacyRepo),
    `${relativePath} still references the pre-transfer repository`,
  );
}

const tauriConfig = JSON.parse(
  await readFile(join(repoRoot, 'openless-all/app/src-tauri/tauri.conf.json'), 'utf8'),
);
const updaterEndpoints = tauriConfig?.plugins?.updater?.endpoints;
assert(
  Array.isArray(updaterEndpoints) && updaterEndpoints.length > 0,
  'desktop updater endpoints are missing',
);

// Updater manifests are served by the project CDN, not by the GitHub Release assets. Both
// halves of that arrangement live in different files, so pin them together here: the endpoint
// in tauri.conf.json must come from the same CDN base that `update_source.rs` declares, and it
// must be the Stable path.
const updateSource = await readFile(
  join(repoRoot, 'openless-all/app/src-tauri/src/update_source.rs'),
  'utf8',
);
const cdnBase = /CDN_BASE: &str = "(https:\/\/[^"]+)"/.exec(updateSource)?.[1];
assert(cdnBase, 'update_source.rs must declare the CDN_BASE constant');
assert(
  updateSource.includes('/updates/{path}/latest-'),
  'update_source.rs must derive manifest URLs from the CDN update path',
);
for (const endpoint of updaterEndpoints) {
  assert(
    endpoint.startsWith(`${cdnBase}/updates/stable/latest-`),
    `desktop updater endpoint must point at the CDN Stable manifest: ${endpoint}`,
  );
  assert(
    !endpoint.includes('github.com'),
    `desktop updater endpoint must not read GitHub Release assets: ${endpoint}`,
  );
  assert(
    endpoint.includes('{{target}}') && endpoint.includes('{{arch}}'),
    `desktop updater endpoint must keep the {{target}}/{{arch}} placeholders: ${endpoint}`,
  );
}

console.log('repository-owner-contract.test.mjs passed');
