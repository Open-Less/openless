#!/usr/bin/env node
// Called only inside the shared release-assets job concurrency lock.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

export const INDEX_NAME = 'updater-signatures.json';
export function signatureFromIndex(index, tag, name) {
  if (
    index.schema !== 1 ||
    index.release_tag !== tag ||
    !Object.hasOwn(index.artifacts ?? {}, name)
  )
    throw new Error('Invalid signature index or missing artifact');
  const value = index.artifacts[name];
  if (typeof value.signature !== 'string' || !value.signature.trim())
    throw new Error('Missing signature');
  return value.signature.trim();
}
export function mergeSignatures(directory, tag, previous) {
  if (
    previous &&
    (previous.schema !== 1 ||
      previous.release_tag !== tag ||
      !previous.artifacts ||
      Array.isArray(previous.artifacts))
  )
    throw new Error('Signature index schema/tag mismatch');
  const artifacts = Object.assign(Object.create(null), previous?.artifacts ?? {});
  for (const name of Object.keys(artifacts)) {
    if (basename(name) !== name || name.includes('\\') || name === '__proto__')
      throw new Error('Invalid artifact name');
    signatureFromIndex(previous, tag, name);
  }
  const signatures = readdirSync(directory)
    .filter((name) => name.endsWith('.sig'))
    .sort();
  if (!signatures.length) throw new Error('No local signatures to consolidate');
  for (const name of signatures) {
    const artifact = name.slice(0, -4);
    if (!existsSync(join(directory, artifact))) throw new Error(`Orphan signature: ${name}`);
    const signature = readFileSync(join(directory, name), 'utf8').trim();
    if (!signature) throw new Error(`Empty signature: ${name}`);
    const bytes = readFileSync(join(directory, artifact));
    artifacts[artifact] = {
      signature,
      size: bytes.length,
      sha256: createHash('sha256').update(bytes).digest('hex'),
    };
  }
  return {
    schema: 1,
    release_tag: tag,
    artifacts: Object.fromEntries(Object.entries(artifacts).sort(([a], [b]) => a.localeCompare(b))),
  };
}
function gh(args) {
  return spawnSync('gh', args, { encoding: 'utf8', maxBuffer: 2 * 1024 * 1024 });
}
function main() {
  const [directoryArg, tag, repository] = process.argv.slice(2);
  if (
    !directoryArg ||
    !/^v[^/]+-tauri$/.test(tag ?? '') ||
    !/^[\w.-]+\/[\w.-]+$/.test(repository ?? '')
  )
    throw new Error('Usage: merge-release-signatures.mjs <directory> <tag> <owner/repo>');
  const directory = resolve(directoryArg);
  const result = gh(['api', `repos/${repository}/releases/tags/${tag}`]);
  let previous;
  if (result.status === 0) {
    const release = JSON.parse(result.stdout);
    if (!release.draft) throw new Error('Refusing to mutate signatures on a published release');
    const asset = release.assets.find((a) => a.name === INDEX_NAME);
    if (!asset && release.assets.some((a) => a.name.endsWith('.sig'))) {
      console.log('Legacy draft: retain its original detached signature format');
      return;
    }
    if (asset) {
      const download = gh([
        'api',
        `repos/${repository}/releases/assets/${asset.id}`,
        '-H',
        'Accept: application/octet-stream',
      ]);
      if (download.status !== 0)
        throw new Error(`Cannot read existing signature index: ${download.stderr}`);
      previous = JSON.parse(download.stdout);
    }
  } else if (!result.stderr.includes('HTTP 404')) {
    throw new Error(`Cannot inspect release: ${result.stderr}`);
  }
  const index = mergeSignatures(directory, tag, previous);
  writeFileSync(join(directory, INDEX_NAME), JSON.stringify(index, null, 2) + '\n');
  for (const name of readdirSync(directory).filter((name) => name.endsWith('.sig')))
    unlinkSync(join(directory, name));
  console.log(`Consolidated ${Object.keys(index.artifacts).length} signatures in ${INDEX_NAME}`);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
