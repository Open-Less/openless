import { readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

// Stable tags omit build metadata; the DMG filenames must retain the complete
// application version. Never derive a dated artifact name from the tag alone.
export function resolveReleaseCaskVersion(tag, filenames) {
  const match = /^v(\d+\.\d+\.\d+)-tauri$/.exec(tag);
  if (!match) throw new Error('Homebrew requires a stable Tauri release tag');
  const base = match[1];
  const versions = ['aarch64', 'x64'].map((arch) => {
    const suffix = `_${arch}.dmg`;
    const candidates = filenames.filter((name) => name.endsWith(suffix));
    if (candidates.length !== 1 || !candidates[0].startsWith('OpenLess_'))
      throw new Error(`Expected exactly one OpenLess ${arch} DMG`);
    return candidates[0].slice('OpenLess_'.length, -suffix.length);
  });
  if (versions[0] !== versions[1]) throw new Error('macOS DMG versions must match');
  const version = versions[0];
  const [release, metadata, ...extra] = version.split('+');
  if (
    release !== base ||
    extra.length ||
    (metadata !== undefined && !/^[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*$/.test(metadata))
  )
    throw new Error('DMG application version must match the stable release tag');
  return version;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [tag, directory] = process.argv.slice(2);
  if (!tag || !directory)
    throw new Error('Usage: resolve-release-cask-version.mjs <tag> <dmg-dir>');
  console.log(resolveReleaseCaskVersion(tag, readdirSync(directory)));
}
