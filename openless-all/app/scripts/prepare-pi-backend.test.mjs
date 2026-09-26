import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { assertWithin, NODE_SHA256, pruneDevelopmentFiles, resolveTarget, verifyDigest } from './prepare-pi-backend.mjs';

test('PI release mapping packages matching native Node and Computer binaries', () => {
  assert.equal(resolveTarget('x86_64-pc-windows-msvc', 'win32', 'x64').nodeName, 'node.exe');
  assert.equal(resolveTarget(undefined, 'darwin', 'arm64').target, 'aarch64-apple-darwin');
  assert.equal(resolveTarget(undefined, 'linux', 'x64').archive, 'node-v22.23.2-linux-x64.tar.gz');
  assert.equal(Object.keys(NODE_SHA256).length, 6);
  assert.equal(resolveTarget('aarch64-linux-android', 'linux', 'x64'), null);
  assert.throws(() => resolveTarget('aarch64-apple-darwin', 'win32', 'x64'), /对应平台/);
  assert.throws(() => resolveTarget('x86_64-unknown-linux-musl', 'linux', 'x64'), /不支持/);
});

test('download checksum fails closed after any changed byte', () => {
  const data = Buffer.from('official distribution');
  const digest = createHash('sha256').update(data).digest('hex');
  assert.equal(verifyDigest(data, digest), digest);
  assert.throws(() => verifyDigest(Buffer.from('modified distribution'), digest), /SHA-256/);
});

test('build cleanup stays inside its explicit output directory', () => {
  const root = join(tmpdir(), 'openless-pi-build');
  assert.equal(assertWithin(root, join(root, 'staging')), join(root, 'staging'));
  assert.throws(() => assertWithin(root, root), /拒绝/);
  assert.throws(() => assertWithin(root, join(root, '..', 'unrelated')), /拒绝/);
});

test('NSIS payload drops declarations and source maps while retaining executable code and licenses', async () => {
  const root = await mkdtemp(join(tmpdir(), 'openless-pi-prune-'));
  try {
    const nested = join(root, 'node_modules', 'sdk', 'node_modules', 'dependency');
    await mkdir(nested, { recursive: true });
    for (const name of ['index.js', 'index.mjs', 'addon.node', 'package.json', 'LICENSE', 'data.map', 'index.d.ts', 'index.d.mts', 'index.d.cts', 'index.js.map', 'index.d.ts.map']) {
      await writeFile(join(nested, name), 'fixture', 'utf8');
    }
    assert.equal(await pruneDevelopmentFiles(root), 5);
    assert.deepEqual((await readdir(nested)).sort(), ['LICENSE', 'addon.node', 'data.map', 'index.js', 'index.mjs', 'package.json']);
  } finally {
    assertWithin(tmpdir(), root);
    await rm(root, { recursive: true, force: true });
  }
});
