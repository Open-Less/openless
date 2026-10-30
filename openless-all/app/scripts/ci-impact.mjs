#!/usr/bin/env node
// Platform reachability for a change set, derived from Cargo's own package graph.
//
//   node scripts/ci-impact.mjs <area> [base-sha]   → prints "true" | "false"
//
// Why this exists: CI used to decide "can this change reach platform X" with a
// hand-written list of path globs, which errs in both directions — an iOS-only
// change still spun up the whole desktop matrix, and a new crate under an
// unlisted path silently fell through to "run everything". The Rust half of the
// answer is not a guess: `cargo metadata` reports every workspace package, its
// directory, its intra-repo dependencies and the `cfg(...)` that guards each
// one. Walking that graph from each platform's root package gives the exact set
// of crates that platform compiles, so a file is in scope for a platform iff
// the package that owns it is reachable from that platform's root.
//
// Anything the graph cannot answer — no base revision, a git failure, an empty
// diff, a file outside every package and rule, a metadata error — prints "true".
// Skipping a check must be the result of a provable "cannot reach", never of a
// failure to classify.
//
// Areas:
//   tauri        macOS / Windows desktop shells and the Android build that
//                shares their dependency graph.
//   msrv         The pinned Rust 1.99 compile.
//   linux        The Linux egui host, its tests and the deb/rpm chain.
//   harmony      The HarmonyOS host (ArkTS contracts, Rust host, HAP).
//   frontend     Browser-rendered surface: the Chromium/WebKit motion suite.
//   ios-device   The Xcode/Rime project layer and the shared Rust crates that
//                are compiled for the real iOS device target.

import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, '../../..');

/** Everything runs. */
const ALL = Symbol('all');

const AREAS = ['tauri', 'msrv', 'linux', 'harmony', 'frontend', 'android', 'capsule', 'ios-device'];

/** Cargo workspaces that describe this repository's own packages. */
const WORKSPACES = ['openless-all/app/Cargo.toml', 'openless-all/app/src-tauri/Cargo.toml'];

/**
 * Platform roots: the package each platform actually builds. The closure is
 * walked from here, so a crate only counts if the platform really links it.
 *
 * `openless-native-capsule` is not linked by the app binary — it is compiled
 * into the desktop shells — so it is listed explicitly, on desktop only.
 * iOS copies `openless-core` into its own tree (see ios/scripts/prepare-core.py)
 * and never sees the Tauri crate.
 */
const PLATFORM_ROOTS = {
  macos: ['openless', 'openless-native-capsule'],
  windows: ['openless', 'openless-native-capsule'],
  android: ['openless'],
  // The Linux reusable workflow also owns Harmony Rust unit tests.
  linux: ['openless-linux-egui', 'openless-harmony-host'],
  harmony: ['openless-harmony-host'],
  ios: ['openless-core'],
};

/**
 * Capsule crates: compiled into the desktop shells and covered by their own
 * macOS + Windows runners, which only earn their keep when these change.
 */
const CAPSULE_PACKAGES = new Set(['openless-capsule', 'openless-native-capsule']);

/** Rust target_os names used when evaluating `cfg(...)` guards. */
const PLATFORM_TARGET_OS = {
  macos: 'macos',
  windows: 'windows',
  android: 'android',
  linux: 'linux',
  harmony: 'ohos',
  ios: 'ios',
};

const rust = () => {
  try {
    execFileSync('cargo', ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
};

function cargoMetadata(manifestRelativePath) {
  const manifestPath = join(REPO_ROOT, manifestRelativePath);
  if (!existsSync(manifestPath)) return null;
  const out = execFileSync(
    'cargo',
    [
      'metadata',
      '--no-deps',
      '--offline',
      '--format-version',
      '1',
      '--manifest-path',
      manifestPath,
    ],
    { cwd: REPO_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 64 << 20 },
  );
  return JSON.parse(out);
}

/** Minimal `cfg(...)` evaluator — enough for the guards Cargo reports. */
function evaluateCfg(expression, targetOs) {
  const text = String(expression ?? '').trim();
  if (!text) return true;
  const body = text.startsWith('cfg(') && text.endsWith(')') ? text.slice(4, -1) : text;
  return evaluatePredicate(body, targetOs);
}

function evaluatePredicate(body, targetOs) {
  const source = body.trim();
  const call = /^(any|all|not)\(([\s\S]*)\)$/.exec(source);
  if (call) {
    const [, name, inner] = call;
    const terms = splitTopLevel(inner);
    if (name === 'any') return terms.some((term) => evaluatePredicate(term, targetOs));
    if (name === 'all') return terms.every((term) => evaluatePredicate(term, targetOs));
    return !evaluatePredicate(terms[0] ?? '', targetOs);
  }
  const targetOsMatch = /^target_os\s*=\s*"([^"]+)"$/.exec(source);
  if (targetOsMatch) return targetOsMatch[1] === targetOs;
  // Unknown predicate (feature = ..., target_arch = ..., a newer cfg key): never
  // skip on it.
  return true;
}

function splitTopLevel(inner) {
  const terms = [];
  let depth = 0;
  let current = '';
  for (const char of inner) {
    if (char === '(') depth += 1;
    if (char === ')') depth -= 1;
    if (char === ',' && depth === 0) {
      terms.push(current);
      current = '';
      continue;
    }
    current += char;
  }
  if (current.trim()) terms.push(current);
  return terms.map((term) => term.trim()).filter(Boolean);
}

/** Package directory → package name, plus intra-repo dependency edges. */
function buildGraph() {
  const packages = new Map();
  const edges = new Map();
  for (const manifest of WORKSPACES) {
    let metadata;
    try {
      metadata = cargoMetadata(manifest);
    } catch {
      return null;
    }
    if (!metadata) continue;
    for (const pkg of metadata.packages) {
      const dir = relative(REPO_ROOT, dirname(pkg.manifest_path)).split(sep).join('/');
      packages.set(pkg.name, { dir, manifest: `${dir}/Cargo.toml` });
      const deps = (pkg.dependencies ?? [])
        .filter((dep) => dep.path)
        .map((dep) => ({ name: dep.name, target: dep.target ?? null }));
      edges.set(pkg.name, deps);
    }
  }
  return packages.size ? { packages, edges } : null;
}

function closureFor(graph, platform) {
  const targetOs = PLATFORM_TARGET_OS[platform];
  const seen = new Set();
  const queue = [...(PLATFORM_ROOTS[platform] ?? [])];
  while (queue.length) {
    const name = queue.pop();
    if (seen.has(name) || !graph.packages.has(name)) continue;
    seen.add(name);
    for (const dep of graph.edges.get(name) ?? []) {
      if (!graph.packages.has(dep.name)) continue;
      if (!evaluateCfg(dep.target, targetOs)) continue;
      queue.push(dep.name);
    }
  }
  return seen;
}

function changedFiles(base) {
  if (!base) return null;
  try {
    const out = execFileSync('git', ['diff', '--name-only', `${base}...HEAD`], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    const files = out
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean);
    return files.length ? files : null;
  } catch {
    return null;
  }
}

const prose = (file) =>
  file.endsWith('.md') ||
  file.startsWith('docs/') ||
  /(^|\/)(LICENSE|NOTICE)/.test(file) ||
  file.startsWith('.github/ISSUE_TEMPLATE/');

const workflowOrTooling = (file) =>
  file.startsWith('.github/') ||
  file === 'Cargo.toml' ||
  file.endsWith('/Cargo.toml') ||
  file.endsWith('/Cargo.lock') ||
  /(^|\/)rust-toolchain(\.toml)?$/.test(file) ||
  file.startsWith('openless-all/app/contract/') ||
  file.startsWith('scripts/');

/**
 * Contract tests are consumed by multiple platform jobs (and some run during
 * Android/Harmony prebuilds). Changing a gate test must not skip the gate it
 * describes, so classify every app script test as fail-open.
 */
const contractSuite = (file) => /^openless-all\/app\/scripts\/[^/]+\.test\.(mjs|js|ts)$/.test(file);

const nativeResource = (file) =>
  file.startsWith('openless-all/app/assets/') || file.startsWith('openless-all/app/public/');

const androidTooling = (file) =>
  /^openless-all\/app\/(package\.json|package-lock\.json)$/.test(file);

/** Browser-rendered surface: sources, styles, the bundle entry and its specs. */
const motionSurface = (file) =>
  /^openless-all\/app\/(src\/|public\/|e2e\/|assets\/)/.test(file) ||
  /^openless-all\/app\/(index\.html|package\.json|package-lock\.json|vite\.config\.[^/]+|tailwind\.[^/]+|postcss\.[^/]+|tsconfig[^/]*\.json)$/.test(
    file,
  ) ||
  /^openless-all\/app\/[^/]+\.css$/.test(file) ||
  file.startsWith('openless-all/app/playwright');

const iosProject = (file) => file.startsWith('openless-all/ios/');

/**
 * Platforms a file's change can reach. `ALL` when the file is tooling or a
 * shared resource whose native consumers cannot be proven by one package.
 */
function classify(file, context) {
  if (prose(file)) return new Set();
  if (contractSuite(file)) return ALL;
  if (workflowOrTooling(file)) return ALL;
  if (iosProject(file)) return new Set(['ios', 'ios-device']);
  if (nativeResource(file)) return ALL;
  if (androidTooling(file)) return new Set(['frontend', 'macos', 'windows', 'harmony', 'android']);

  const tags = new Set();
  if (motionSurface(file)) {
    // The bundle is embedded in every shell that renders it. The Linux host uses
    // a native egui UI and does not consume `dist/` at all, and the Android job
    // only compiles the Rust crate (`npm run build` there just satisfies
    // `generate_context!`), so neither is in scope for a bundle-only change.
    for (const tag of ['frontend', 'macos', 'windows', 'harmony']) tags.add(tag);
  }

  const owner = ownerPackage(file, context);
  if (owner) {
    if (CAPSULE_PACKAGES.has(owner)) tags.add('capsule');
    for (const [platform, packages] of context.closures) {
      if (packages.has(owner)) {
        tags.add(platform);
        if (platform === 'ios') tags.add('ios-device');
      }
    }
  }
  // Files outside every package still have to be evaluated by the shells that
  // embed them (assets, fixtures, icons).
  if (tags.size === 0) {
    if (file.startsWith('openless-all/app/harmony/')) return new Set(['harmony']);
    if (file.startsWith('openless-all/app/linux-egui/')) return new Set(['linux']);
    return ALL;
  }
  return tags;
}

function ownerPackage(file, context) {
  let best = null;
  for (const [name, pkg] of context.graph.packages) {
    if (file === pkg.manifest || file.startsWith(`${pkg.dir}/`)) {
      if (!best || pkg.dir.length > best.dir.length) best = { name, dir: pkg.dir };
    }
  }
  return best?.name ?? null;
}

function isRustSource(file) {
  return file.endsWith('.rs') || file.endsWith('Cargo.toml') || file.endsWith('Cargo.lock');
}

function decide(area, context, files) {
  const reached = new Set();
  for (const file of files) {
    const tags = classify(file, context);
    if (tags === ALL) return true;
    for (const tag of tags) reached.add(tag);
  }
  switch (area) {
    case 'msrv':
      return files.some(isRustSource) || files.some(workflowOrTooling);
    case 'tauri':
      return ['macos', 'windows', 'android'].some((tag) => reached.has(tag));
    case 'linux':
      return reached.has('linux');
    case 'harmony':
      return reached.has('harmony');
    case 'frontend':
      return reached.has('frontend');
    case 'android':
      return reached.has('android');
    case 'capsule':
      return reached.has('capsule');
    case 'ios-device':
      return reached.has('ios-device');
    default:
      return true;
  }
}

function main() {
  const [area, base = ''] = process.argv.slice(2);
  const run = () => {
    process.stdout.write('true\n');
    process.exit(0);
  };
  if (!AREAS.includes(area)) run();
  const files = changedFiles(base);
  if (!files) run();
  if (!rust()) run();
  const graph = buildGraph();
  if (!graph) run();
  const closures = new Map();
  for (const platform of Object.keys(PLATFORM_ROOTS)) {
    closures.set(platform, closureFor(graph, platform));
  }
  const verdict = decide(area, { graph, closures }, files);
  process.stdout.write(`${verdict ? 'true' : 'false'}\n`);
}

const invokedDirectly =
  Boolean(process.argv[1]) && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  try {
    main();
  } catch {
    process.stdout.write('true\n');
  }
}

export { AREAS, classify, decide, buildGraph, closureFor, evaluateCfg };
