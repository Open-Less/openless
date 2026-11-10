import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const runGh = (args) =>
  execFileSync('gh', args, {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });

// One short preflight creates the shared draft before any platform starts.
// Existing published releases are read-only; platform retries must not reopen one.
export function prepareRelease(tag, repository, gh = runGh) {
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?-tauri$/.test(tag)) {
    throw new Error('Expected an immutable v<version>-tauri release tag');
  }
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error('Invalid repository');
  const prerelease = tag.endsWith('-beta-tauri') || tag.includes('-Beta.');
  let release;
  try {
    release = JSON.parse(
      gh(['api', `repos/${repository}/releases/tags/${encodeURIComponent(tag)}`]),
    );
  } catch (error) {
    if (!/HTTP 404/.test(String(error.stderr ?? ''))) throw error;
    gh([
      'release',
      'create',
      tag,
      '--repo',
      repository,
      '--verify-tag',
      '--draft',
      '--title',
      `OpenLess ${tag}`,
      '--notes',
      '',
      ...(prerelease ? ['--prerelease'] : []),
    ]);
    return 'created';
  }
  if (release.tag_name !== tag) throw new Error('Release tag mismatch');
  if (release.draft !== true) throw new Error('Refusing to modify an already published release');
  if (release.prerelease !== prerelease) throw new Error('Release channel mismatch');
  return 'existing draft';
}

if (resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) {
  const result = prepareRelease(process.argv[2], process.argv[3]);
  console.log(`Shared release: ${result}`);
}
