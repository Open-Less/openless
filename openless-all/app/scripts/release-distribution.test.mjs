import assert from 'node:assert/strict';
import {
  mkdtempSync,
  readFileSync,
  writeFileSync,
  rmSync,
  mkdirSync,
  chmodSync,
  existsSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import ts from 'typescript';
import yaml from 'js-yaml';
import { mergeSignatures, signatureFromIndex } from './merge-release-signatures.mjs';
const audit = spawnSync(
  process.execPath,
  [new URL('./audit-harmony-ipc.mjs', import.meta.url).pathname, '--check'],
  { encoding: 'utf8' },
);
assert.equal(audit.status, 0, audit.stdout + audit.stderr);
const root = mkdtempSync(join(tmpdir(), 'openless-distribution-'));
try {
  const tag = 'v2.0.2-Beta.7-tauri';
  const desktop = join(root, 'desktop');
  const android = join(root, 'android');
  mkdirSync(desktop);
  mkdirSync(android);
  const put = (directory, name, bytes, signature) => {
    writeFileSync(join(directory, name), bytes);
    writeFileSync(join(directory, name + '.sig'), signature);
  };
  put(desktop, 'OpenLess_2.0.2-Beta.7_x64-setup.exe', 'desktop', 'desktop-signature');
  put(android, 'OpenLess_2.0.2-Beta.7_arm64-v8a.apk', 'android', 'android-signature');
  const first = mergeSignatures(desktop, tag);
  const both = mergeSignatures(android, tag, first);
  assert.equal(Object.keys(both.artifacts).length, 2);
  assert.equal(
    signatureFromIndex(both, tag, 'OpenLess_2.0.2-Beta.7_x64-setup.exe'),
    'desktop-signature',
  );
  assert.equal(both.artifacts['OpenLess_2.0.2-Beta.7_arm64-v8a.apk'].size, 7);
  assert.deepEqual(
    mergeSignatures(desktop, tag, both),
    both,
    'publisher retry preserves the other platform',
  );
  assert.deepEqual(
    mergeSignatures(desktop, tag, mergeSignatures(android, tag)),
    both,
    'either publisher can finish first',
  );
  assert.throws(() => mergeSignatures(android, 'v3-tauri', both), /schema\/tag/);
  assert.throws(() => signatureFromIndex(both, tag, 'missing.exe'));
  put(android, 'orphan.apk', '', '');
  assert.throws(() => mergeSignatures(android, tag), /Empty signature/);

  const bin = join(root, 'bin');
  mkdirSync(bin);
  // `gh release view` resolves drafts; the mock only has to answer that call and
  // the asset download (which must use the numeric API URL, not the node id).
  writeFileSync(
    join(bin, 'gh'),
    '#!/bin/sh\nif [ -n "$MOCK_FAIL" ]; then echo "HTTP $MOCK_FAIL" >&2; exit 1; fi\n' +
      'if [ "$MOCK_MISSING" = 1 ]; then echo "release not found" >&2; exit 1; fi\n' +
      'case "$1 $2" in\n' +
      '  "release view") printf "%s" "$MOCK_RELEASE" ;;\n' +
      '  "api https://api.github.com/repos/Open-Less/openless/releases/assets/7") printf "%s" "$MOCK_INDEX" ;;\n' +
      '  *) echo "unexpected gh invocation: $*" >&2; exit 1 ;;\n' +
      'esac\n',
  );
  chmodSync(join(bin, 'gh'), 0o755);
  const runMerge = (release, failure = '', extra = {}) =>
    spawnSync(
      process.execPath,
      [
        new URL('./merge-release-signatures.mjs', import.meta.url).pathname,
        desktop,
        tag,
        'Open-Less/openless',
      ],
      {
        encoding: 'utf8',
        env: {
          ...process.env,
          PATH: bin + ':' + process.env.PATH,
          MOCK_RELEASE: JSON.stringify(release),
          MOCK_FAIL: failure,
          ...extra,
        },
      },
    );
  assert.notEqual(runMerge({ tagName: tag, isDraft: false, assets: [] }).status, 0);
  assert.ok(existsSync(join(desktop, 'OpenLess_2.0.2-Beta.7_x64-setup.exe.sig')));
  assert.equal(
    runMerge({ tagName: tag, isDraft: true, assets: [{ name: 'historical.exe.sig' }] }).status,
    0,
  );
  assert.notEqual(
    runMerge({ tagName: 'v2.0.2-Beta.6-tauri', isDraft: true, assets: [] }).status,
    0,
    'a draft for another tag must not be mutated',
  );
  assert.ok(!existsSync(join(desktop, 'updater-signatures.json')));
  assert.notEqual(runMerge({}, '503').status, 0, 'API failures must not discard other platforms');
  // A real draft found through `release view` keeps the other platform's entry.
  const existingIndex = JSON.stringify({
    schema: 1,
    release_tag: tag,
    artifacts: {
      'OpenLess_2.0.2-Beta.7_arm64-v8a.apk': {
        signature: 'android-signature',
        size: 7,
        sha256: '',
      },
    },
  });
  assert.equal(
    runMerge(
      {
        tagName: tag,
        isDraft: true,
        assets: [
          {
            name: 'updater-signatures.json',
            id: 'RA_kwDOSNoZCc4rVTlL',
            apiUrl: 'https://api.github.com/repos/Open-Less/openless/releases/assets/7',
          },
        ],
      },
      '',
      { MOCK_INDEX: existingIndex },
    ).status,
    0,
  );
  const merged = JSON.parse(readFileSync(join(desktop, 'updater-signatures.json'), 'utf8'));
  assert.deepEqual(Object.keys(merged.artifacts), [
    'OpenLess_2.0.2-Beta.7_arm64-v8a.apk',
    'OpenLess_2.0.2-Beta.7_x64-setup.exe',
  ]);
  rmSync(join(desktop, 'OpenLess_2.0.2-Beta.7_x64-setup.exe.sig'), { force: true });
  rmSync(join(desktop, 'updater-signatures.json'), { force: true });
  put(desktop, 'OpenLess_2.0.2-Beta.7_x64-setup.exe', 'desktop', 'desktop-signature');
  assert.equal(runMerge({ tagName: tag, isDraft: true, assets: [] }).status, 0);
  assert.ok(existsSync(join(desktop, 'updater-signatures.json')));
  assert.ok(!existsSync(join(desktop, 'OpenLess_2.0.2-Beta.7_x64-setup.exe.sig')));

  const tauri = yaml.load(
    readFileSync(new URL('../../../.github/workflows/release-tauri.yml', import.meta.url), 'utf8'),
  );
  const apk = yaml.load(
    readFileSync(new URL('../../../.github/workflows/android-apk.yml', import.meta.url), 'utf8'),
  );
  assert.match(tauri.jobs.publish.concurrency.group, /openless-release-assets-/);
  assert.match(apk.jobs['publish-android-release'].concurrency.group, /openless-release-assets-/);
  for (const job of [tauri.jobs.publish, apk.jobs['publish-android-release']]) {
    assert.equal(job.concurrency['cancel-in-progress'], false);
    const merge = job.steps.find((step) => step.name === 'Consolidate release signatures');
    const publish = job.steps.find((step) => step.uses?.startsWith('softprops/action-gh-release@'));
    assert.ok(job.steps.indexOf(merge) < job.steps.indexOf(publish));
  }
  const mirror = yaml.load(
    readFileSync(new URL('../../../.github/workflows/mirror-release.yml', import.meta.url), 'utf8'),
  );
  assert.equal(mirror.permissions.contents, 'read');
  assert.ok(mirror.on.release.types.includes('published'));
  assert.equal(
    mirror.jobs.mirror.steps[0].with.ref,
    '${{ github.event.repository.default_branch }}',
  );
  const python = spawnSync(
    'python3',
    [new URL('./test_release_mirror.py', import.meta.url).pathname],
    { encoding: 'utf8' },
  );
  assert.equal(python.status, 0, python.stdout + python.stderr);

  const source = readFileSync(new URL('../src/lib/updateDownload.ts', import.meta.url), 'utf8');
  const compiled = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const { downloadWithGithubFallback } = await import(
    'data:text/javascript;base64,' + Buffer.from(compiled).toString('base64')
  );
  const cdn = { source: 'cdn', version: '2.0.2-Beta.7', signature: 'same-signature' };
  const github = { ...cdn, source: 'github' };
  const calls = [];
  const fallback = async (original) => {
    assert.equal(original, cdn);
    return github;
  };
  const failedCdn = async (candidate) => {
    calls.push(candidate);
    if (candidate.source === 'cdn') throw new Error('connection reset halfway');
  };
  assert.equal(await downloadWithGithubFallback(cdn, failedCdn, () => true, fallback), github);
  assert.deepEqual(calls, [cdn, github]);
  let retries = 0;
  assert.equal(
    await downloadWithGithubFallback(
      cdn,
      async () => {},
      () => true,
      async () => {
        retries++;
        return github;
      },
    ),
    cdn,
  );
  assert.equal(retries, 0);
  await assert.rejects(
    downloadWithGithubFallback(
      github,
      async () => {
        throw new Error('github down');
      },
      () => false,
      fallback,
    ),
    /github down/,
  );
  await assert.rejects(
    downloadWithGithubFallback(
      cdn,
      async () => {
        throw new Error('both down');
      },
      () => true,
      fallback,
    ),
    /both down/,
  );
} finally {
  rmSync(root, { recursive: true, force: true });
}
console.log('release distribution and mid-download fallback contracts passed');
