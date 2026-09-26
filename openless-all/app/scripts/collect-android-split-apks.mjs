/**
 * Collect split-per-ABI APKs from gen/android outputs.
 *
 * Env:
 *   OPENLESS_APK_MODE   debug|release
 *   OPENLESS_APK_LABEL  artifact label suffix
 *   OPENLESS_EXPECTED_GRADLE_ABIS  comma-separated Gradle ABI folders (required)
 *   RUNNER_TEMP         output parent (required in CI)
 *   GITHUB_OUTPUT       optional; writes paths when set
 */
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, relative } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { entryForGradleAbi } from './android-abi-matrix.mjs';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const KNOWN_ABIS = new Set(['arm64-v8a', 'armeabi-v7a', 'x86_64', 'x86']);

function walkApks(root) {
  const out = [];
  if (!existsSync(root)) return out;
  const stack = [root];
  while (stack.length) {
    const dir = stack.pop();
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      const st = statSync(path);
      if (st.isDirectory()) {
        stack.push(path);
      } else if (name.endsWith('.apk')) {
        out.push(path);
      }
    }
  }
  return out.sort();
}

function listApkAbis(apkPath) {
  // Use the same standard ZIP reader as the original workflow. Scanning for
  // central-directory magic accepts truncated archives and bytes inside entries.
  const script = `
import json, sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as apk:
    bad = apk.testzip()
    if bad is not None:
        raise ValueError("CRC failure in " + bad)
    print(json.dumps(sorted({name.split('/')[1] for name in apk.namelist()
        if name.startswith('lib/') and len(name.split('/')) >= 3})))
`;
  const commands = process.platform === 'win32' ? ['python', 'python3'] : ['python3', 'python'];
  for (const command of commands) {
    const result = spawnSync(command, ['-c', script, apkPath], {
      encoding: 'utf8',
      maxBuffer: 1024 * 1024,
    });
    if (result.error?.code === 'ENOENT') continue;
    if (result.error || result.status !== 0) {
      throw new Error(`Invalid APK ZIP ${apkPath}: ${result.error?.message || result.stderr}`);
    }
    const abis = JSON.parse(result.stdout);
    for (const abi of abis) {
      if (!KNOWN_ABIS.has(abi)) throw new Error(`Unknown ABI ${abi} in ${apkPath}`);
    }
    return abis;
  }
  throw new Error('Python 3 is required to validate APK ZIP contents');
}

export function collectSplitApks({
  mode,
  label,
  expectedGradleAbis,
  androidRoot = join(appRoot, 'src-tauri/gen/android'),
  outDir,
  version,
} = {}) {
  if (!mode || !label) {
    throw new Error('mode and label are required');
  }
  if (!expectedGradleAbis?.length) {
    throw new Error('expectedGradleAbis must be a non-empty array');
  }
  if (!outDir) {
    throw new Error('outDir is required');
  }
  if (!version) {
    throw new Error('version is required');
  }

  mkdirSync(outDir, { recursive: true });
  const candidates = walkApks(androidRoot).filter((apk) =>
    apk.replace(/\\/g, '/').includes('/outputs/'),
  );
  if (candidates.length === 0) {
    const all = walkApks(androidRoot);
    const hint = all.length ? all.map((p) => relative(androidRoot, p)).join('\n') : '(none)';
    throw new Error(`No APK found under ${androidRoot}/**/outputs/\nOther APKs:\n${hint}`);
  }

  const expected = new Set(expectedGradleAbis);
  const found = new Map();

  for (const apk of candidates) {
    const abis = listApkAbis(apk);
    if (abis.length !== 1) {
      throw new Error(
        `${apk} contains ABI directories [${abis.join(', ')}]; expected exactly one ABI per APK`,
      );
    }
    const abi = abis[0];
    if (!expected.has(abi)) {
      // Ignore extras from previous local builds; only enforce expected set.
      console.warn(`Skipping unexpected ABI ${abi} from ${apk}`);
      continue;
    }
    if (found.has(abi)) {
      throw new Error(`Duplicate APKs for ABI ${abi}: ${found.get(abi)} and ${apk}`);
    }
    const destName =
      mode === 'release'
        ? `OpenLess_${version}_${abi}.apk`
        : `OpenLess-android-debug-${abi}-${label}.apk`;
    const dest = join(outDir, destName);
    copyFileSync(apk, dest);
    found.set(abi, dest);
    console.log(`Collected ${abi}: ${apk} -> ${dest}`);
  }

  const missing = [...expected].filter((abi) => !found.has(abi)).sort();
  if (missing.length) {
    throw new Error(`Missing split APKs for ABI(s): ${missing.join(', ')}`);
  }

  const releaseFiles = [...expected].map((abi) => found.get(abi));
  const outputs = {};
  for (const abi of expected) {
    const entry = entryForGradleAbi(abi);
    outputs[`${entry.outputKey}_path`] = found.get(abi);
    outputs[`${entry.outputKey}_arch`] = entry.abi;
  }
  outputs.out_dir = outDir;
  outputs.release_files = releaseFiles.join('\n');
  // Single-ABI CI jobs consume these stable keys.
  if (expected.size === 1) {
    const onlyAbi = [...expected][0];
    const entry = entryForGradleAbi(onlyAbi);
    outputs.apk_path = found.get(onlyAbi);
    outputs.gradle_abi = onlyAbi;
    outputs.cli_abi = entry.abi;
  }
  return { found, outputs, releaseFiles };
}

function main() {
  const mode = process.env.OPENLESS_APK_MODE;
  const label = process.env.OPENLESS_APK_LABEL;
  const expectedRaw = process.env.OPENLESS_EXPECTED_GRADLE_ABIS || '';
  const expectedGradleAbis = expectedRaw
    .split(/[,\s]+/)
    .map((s) => s.trim())
    .filter(Boolean);
  const runnerTemp = process.env.RUNNER_TEMP;
  if (!runnerTemp) {
    throw new Error('RUNNER_TEMP is required');
  }
  const version = JSON.parse(readFileSync(join(appRoot, 'package.json'), 'utf8')).version;
  const outDir = join(runnerTemp, `openless-android-${mode}-split`);
  const { outputs } = collectSplitApks({
    mode,
    label,
    expectedGradleAbis,
    outDir,
    version,
  });

  const githubOutput = process.env.GITHUB_OUTPUT;
  if (githubOutput) {
    const lines = [];
    for (const [key, value] of Object.entries(outputs)) {
      if (key === 'release_files') {
        lines.push(`${key}<<EOF`, value, 'EOF');
      } else {
        lines.push(`${key}=${value}`);
      }
    }
    writeFileSync(githubOutput, `${lines.join('\n')}\n`, { flag: 'a' });
  }
}

if (process.argv[1]?.replace(/\\/g, '/').endsWith('collect-android-split-apks.mjs')) {
  main();
}
