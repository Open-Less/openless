/**
 * Android ABI matrix helpers for CI (#1103).
 *
 * CLI target names match `tauri android build --target`.
 * Gradle ABI folder names match APK lib/ layout.
 */

export const ANDROID_ABI_MATRIX = [
  {
    abi: 'aarch64',
    rustTarget: 'aarch64-linux-android',
    gradleAbi: 'arm64-v8a',
    outputKey: 'arm64_v8a',
  },
  {
    abi: 'armv7',
    rustTarget: 'armv7-linux-androideabi',
    gradleAbi: 'armeabi-v7a',
    outputKey: 'armeabi_v7a',
  },
  {
    abi: 'i686',
    rustTarget: 'i686-linux-android',
    gradleAbi: 'x86',
    outputKey: 'x86',
  },
  {
    abi: 'x86_64',
    rustTarget: 'x86_64-linux-android',
    gradleAbi: 'x86_64',
    outputKey: 'x86_64',
  },
];

const BY_ABI = new Map(ANDROID_ABI_MATRIX.map((entry) => [entry.abi, entry]));
const BY_GRADLE = new Map(ANDROID_ABI_MATRIX.map((entry) => [entry.gradleAbi, entry]));

export function entryForAbi(abi) {
  const entry = BY_ABI.get(abi);
  if (!entry) {
    throw new Error(
      `Unknown Android ABI "${abi}". Expected one of: ${ANDROID_ABI_MATRIX.map((e) => e.abi).join(', ')}`,
    );
  }
  return entry;
}

export function entryForGradleAbi(gradleAbi) {
  const entry = BY_GRADLE.get(gradleAbi);
  if (!entry) {
    throw new Error(`Unknown Gradle ABI "${gradleAbi}"`);
  }
  return entry;
}

/**
 * Parse a comma/space-separated ABI list or the keyword "all".
 * Empty / whitespace → defaultAbis.
 */
export function parseAndroidAbis(raw, { defaultAbis = ['aarch64'] } = {}) {
  const text = (raw ?? '').trim();
  if (!text) {
    return defaultAbis.map(entryForAbi);
  }
  if (text.toLowerCase() === 'all') {
    return [...ANDROID_ABI_MATRIX];
  }
  const tokens = text
    .split(/[,\s]+/)
    .map((t) => t.trim())
    .filter(Boolean);
  if (tokens.length === 0) {
    return defaultAbis.map(entryForAbi);
  }
  const seen = new Set();
  const out = [];
  for (const token of tokens) {
    const entry = entryForAbi(token);
    if (seen.has(entry.abi)) continue;
    seen.add(entry.abi);
    out.push(entry);
  }
  return out;
}

/** GitHub Actions matrix.include payload for selected ABIs. */
export function toGithubMatrixInclude(entries) {
  return entries.map((entry) => ({
    abi: entry.abi,
    rust_target: entry.rustTarget,
    gradle_abi: entry.gradleAbi,
    output_key: entry.outputKey,
  }));
}

function main() {
  const mode = process.argv[2] || 'parse';
  if (mode === 'parse') {
    const raw = process.argv[3] ?? process.env.OPENLESS_ANDROID_ABIS ?? '';
    const defaultRaw = process.env.OPENLESS_ANDROID_ABIS_DEFAULT ?? 'aarch64';
    const entries = parseAndroidAbis(raw, {
      defaultAbis: parseAndroidAbis(defaultRaw, { defaultAbis: ['aarch64'] }).map((e) => e.abi),
    });
    process.stdout.write(JSON.stringify(toGithubMatrixInclude(entries)));
    return;
  }
  if (mode === 'list-all') {
    process.stdout.write(JSON.stringify(toGithubMatrixInclude(ANDROID_ABI_MATRIX)));
    return;
  }
  if (mode === 'rust-targets') {
    const raw = process.argv[3] ?? process.env.OPENLESS_ANDROID_ABIS ?? 'all';
    const entries = parseAndroidAbis(raw, { defaultAbis: ANDROID_ABI_MATRIX.map((e) => e.abi) });
    process.stdout.write(entries.map((e) => e.rustTarget).join(','));
    return;
  }
  console.error(`Usage: node android-abi-matrix.mjs <parse|list-all|rust-targets> [abis]`);
  process.exit(1);
}

if (process.argv[1]?.replace(/\\/g, '/').endsWith('android-abi-matrix.mjs')) {
  main();
}
