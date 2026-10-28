// Contract test for scripts/ci-changed-areas.sh (and the ci-impact.mjs graph
// walk behind it).
//
// The CI jobs that verify each platform are skipped for change sets that cannot
// reach them. That decision must never be made on a guess: this test drives the
// script against real git diffs and asserts both directions - out-of-scope
// change sets skip, everything else runs. The Rust half of the answer comes from
// `cargo metadata`, which is why the scratch repository below has no Cargo files
// of its own: the classifier reads the package graph from this checkout and the
// diff from the scratch repository.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const scriptPath = resolve(dirname(fileURLToPath(import.meta.url)), 'ci-changed-areas.sh');
const git = (cwd, args) =>
  execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });

const scratch = mkdtempSync(join(tmpdir(), 'openless-changed-areas-'));
const repo = join(scratch, 'repo');
mkdirSync(repo, { recursive: true });

git(repo, ['init', '--quiet', '--initial-branch=main']);
git(repo, ['config', 'user.email', 'ci@example.invalid']);
git(repo, ['config', 'user.name', 'CI Contract']);
git(repo, ['config', 'commit.gpgsign', 'false']);
writeFileSync(join(repo, 'README.md'), 'base\n');
git(repo, ['add', '.']);
git(repo, ['commit', '--quiet', '-m', 'base']);
const base = git(repo, ['rev-parse', 'HEAD']).trim();

let failures = 0;
const check = (label, actual, expected) => {
  const ok = actual === expected;
  if (!ok) failures += 1;
  console.log(`${ok ? 'ok' : 'FAIL'}  ${label} -> ${actual} (expected ${expected})`);
};

const commit = (files, message) => {
  for (const [path, body] of Object.entries(files)) {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), body);
  }
  git(repo, ['add', '-A']);
  git(repo, ['commit', '--quiet', '-m', message]);
};

const decide = (area, revision = base) => {
  const args = [scriptPath, area];
  if (revision !== null) args.push(revision);
  return execFileSync('bash', args, { cwd: repo, encoding: 'utf8' }).trim();
};

const reset = () => {
  git(repo, ['reset', '--hard', '--quiet', base]);
  git(repo, ['clean', '-fdq']);
};

try {
  const ALL_AREAS = [
    'tauri',
    'msrv',
    'linux',
    'harmony',
    'frontend',
    'android',
    'capsule',
    'ios-device',
  ];
  const every = (verdict) => Object.fromEntries(ALL_AREAS.map((area) => [area, verdict]));
  const expect = (label, expectations) => {
    for (const [area, expected] of Object.entries(expectations)) {
      check(label + ': ' + area, decide(area), expected);
    }
  };

  // Prose cannot reach anything.
  commit({ 'docs/architecture.md': 'docs\n' }, 'docs only');
  expect('docs only', every('false'));
  reset();

  // The Linux host is a separate crate in its own target graph.
  commit({ 'openless-all/app/linux-egui/src/ui/theme.rs': 'fn x() {}\n' }, 'linux host source');
  expect('linux host source', {
    tauri: 'false',
    msrv: 'true',
    linux: 'true',
    harmony: 'false',
    frontend: 'false',
    android: 'false',
    'ios-device': 'false',
  });
  reset();

  // ArkTS lives only in the HarmonyOS host.
  commit(
    { 'openless-all/app/harmony/entry/src/main/ets/pages/Index.ets': 'export {};\n' },
    'arkts',
  );
  expect('arkts source', {
    tauri: 'false',
    harmony: 'true',
    linux: 'false',
    frontend: 'false',
    'ios-device': 'false',
  });
  reset();

  // The Linux workflow owns Harmony Rust unit tests as well as the egui host.
  commit({ 'openless-all/app/harmony/rust/src/lib.rs': 'pub fn x() {}\n' }, 'harmony rust');
  expect('harmony rust', {
    tauri: 'false',
    harmony: 'true',
    linux: 'true',
    frontend: 'false',
    'ios-device': 'false',
  });
  reset();

  // The shared Core is linked by every platform build.
  commit({ 'openless-all/app/crates/openless-core/src/lib.rs': 'pub fn x() {}\n' }, 'shared core');
  expect('shared core', {
    tauri: 'true',
    msrv: 'true',
    linux: 'true',
    harmony: 'true',
    android: 'true',
    // The device build compiles this shared Core for aarch64-apple-ios.
    'ios-device': 'true',
    frontend: 'false',
  });
  reset();

  // ...but its manifest moves the workspace dependency graph for everyone.
  commit(
    { 'openless-all/app/linux-egui/Cargo.toml': '[package]\nname = "x"\n' },
    'linux host manifest',
  );
  expect('linux host manifest', { tauri: 'true', msrv: 'true', linux: 'true', harmony: 'true' });
  reset();

  // Desktop-only crates never reach the Linux, HarmonyOS or iOS builds.
  commit({ 'openless-all/app/src-tauri/src/lib.rs': 'pub fn x() {}\n' }, 'native source');
  expect('native source', {
    tauri: 'true',
    msrv: 'true',
    linux: 'false',
    harmony: 'false',
    android: 'true',
    capsule: 'false',
    'ios-device': 'false',
  });
  reset();

  commit({ 'openless-all/app/native-capsule/src/lib.rs': 'pub fn x() {}\n' }, 'capsule source');
  expect('capsule source', {
    tauri: 'true',
    msrv: 'true',
    capsule: 'true',
    linux: 'false',
    harmony: 'false',
  });
  reset();

  // The rendered bundle is embedded by every shell that paints it; the Linux
  // host draws a native egui UI and never reads `dist/`, and the Android job only
  // compiles Rust, so neither is in scope.
  commit({ 'openless-all/app/src/pages/Style.tsx': 'export const x = 1;\n' }, 'frontend only');
  expect('frontend only', {
    tauri: 'true',
    msrv: 'false',
    linux: 'false',
    harmony: 'true',
    frontend: 'true',
    android: 'false',
    'ios-device': 'false',
  });
  reset();

  // Contract tests are consumed by Linux, Harmony and Android as well as desktop.
  commit(
    { 'openless-all/app/scripts/foo-contract.test.mjs': 'console.log(1);\n' },
    'contract suite',
  );
  expect('contract suite', every('true'));
  reset();

  // Native resources have both bundle and direct native consumers.
  commit({ 'openless-all/app/assets/vocab-presets.json': '{}\n' }, 'native asset');
  expect('native asset', every('true'));
  reset();

  // npm manifests also control Android project generation, not only the bundle.
  commit(
    {
      'openless-all/app/package.json': '{}\n',
      'openless-all/app/package-lock.json': '{}\n',
    },
    'android tooling',
  );
  expect('android tooling', {
    tauri: 'true',
    msrv: 'false',
    linux: 'false',
    harmony: 'true',
    frontend: 'true',
    android: 'true',
    capsule: 'false',
    'ios-device': 'false',
  });
  reset();

  // iOS project work is the only thing that can break the Xcode build.
  commit({ 'openless-all/ios/Packages/OpenLessShared/Sources/X.swift': 'let x = 1\n' }, 'swift');
  expect('ios project source', {
    tauri: 'false',
    msrv: 'false',
    frontend: 'false',
    'ios-device': 'true',
  });
  reset();

  // A dependency bump moves every Rust build and the MSRV promise.
  commit({ 'openless-all/app/Cargo.lock': '# lock\n' }, 'lockfile');
  expect('lockfile', { tauri: 'true', msrv: 'true', linux: 'true', harmony: 'true' });
  reset();

  // Workflow changes always verify everything.
  commit({ '.github/workflows/ci.yml': 'name: CI\n' }, 'workflow');
  expect('workflow change', every('true'));
  reset();

  // Unreadable or empty change sets never skip a platform check.
  for (const area of ALL_AREAS) check('no base revision: ' + area, decide(area, null), 'true');
  for (const area of ALL_AREAS) check('empty diff: ' + area, decide(area), 'true');
  for (const area of ALL_AREAS) {
    check('unknown revision: ' + area, decide(area, 'deadbeefdeadbeef'), 'true');
  }
  check('unknown area', decide('nonsense'), 'true');
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

// The wiring matters as much as the decision: a missing job output silently
// turns "this change set cannot reach the Linux build" into a skipped job. These
// checks read the workflow that consumes the script and fail when an area is not
// published, when a gate is not fail-open, or when prose-only runs could skip
// the Linux package chain without the reusable workflow's explicit opt-out.
const workflow = readFileSync(
  resolve(dirname(fileURLToPath(import.meta.url)), '../../..', '.github/workflows/ci.yml'),
  'utf8',
);
const reusable = readFileSync(
  resolve(
    dirname(fileURLToPath(import.meta.url)),
    '../../..',
    '.github/workflows/check-linux-egui.yml',
  ),
  'utf8',
);
const areas = ['tauri', 'msrv', 'linux', 'harmony', 'frontend', 'android', 'capsule'];
check(
  'workflow publishes every area',
  areas.every((area) => workflow.includes(area + ': ${{ steps.detect.outputs.' + area + ' }}')),
  true,
);
check(
  'workflow computes every area in the detect step',
  /for area in tauri msrv linux harmony frontend android capsule; do/.test(workflow),
  true,
);
check(
  'tauri gate is fail-open',
  workflow.includes("if: needs.changes.outputs.tauri != 'false'"),
  true,
);
check(
  'msrv gate is fail-open',
  workflow.includes("if: needs.changes.outputs.msrv != 'false'"),
  true,
);
// Native changes must exercise the pinned minimum compiler on every platform.
check(
  'macos msrv is not skipped when stable has the same version',
  workflow.includes("if: needs.changes.outputs.msrv != 'false' &&"),
  false,
);
check(
  'changes job publishes the native-source gate for pinned toolchains',
  workflow.includes('msrv_needed: ${{ steps.detect.outputs.msrv }}'),
  true,
);
// The secondary platform uses the same native-source gate.
check(
  'windows msrv steps follow the source gate',
  workflow.includes("if: runner.os == 'Windows' && needs.changes.outputs.msrv_needed == 'true'"),
  true,
);
// `cargo check` immediately followed by `cargo test --no-run` compiles the graph
// twice; the no-run pass now covers bins itself.
check(
  'windows has one codegen pass, not check plus no-run',
  !/run: cargo check --locked --manifest-path src-tauri\/Cargo\.toml$/m.test(workflow),
  true,
);
check(
  'windows codegen pass covers lib and bins',
  workflow.includes(
    'cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --bins --no-run',
  ),
  true,
);
check(
  'android gate is fail-open',
  /inputs\.platform != 'macos'\) && needs\.changes\.outputs\.android != 'false'/.test(workflow),
  true,
);
check(
  'capsule gate is fail-open',
  workflow.includes("needs.changes.outputs.capsule != 'false'"),
  true,
);
check(
  'ui motion gate follows the rendered surface',
  /inputs\.platform != 'macos'\) && needs\.changes\.outputs\.frontend != 'false'/.test(workflow),
  true,
);
check(
  'harmony gate is fail-open',
  workflow.includes("needs.changes.outputs.harmony != 'false'"),
  true,
);
check(
  'linux scope defaults to full',
  workflow.includes("needs.changes.outputs.linux == 'false' && 'none' || 'full'"),
  true,
);
check(
  'reusable workflow defaults to a full build',
  /scope:[\s\S]{0,80}default: full/.test(reusable),
  true,
);
check(
  'reusable workflow gates every build step',
  (reusable.match(/if: inputs\.scope != 'none'/g) ?? []).length >= 10,
  true,
);

// Wall-clock guards: the Windows runner is the slowest one, so the frontend
// suite must not serialise behind the Rust compile, and the Linux tests must not
// serialise in front of the deb/rpm build.
check(
  'windows frontend runs in its own job',
  /  windows-frontend:[\s\S]*?runs-on: windows-latest/.test(workflow),
  true,
);
check(
  'windows only runs the platform-sensitive contracts',
  workflow.includes('--platform-only windows'),
  true,
);
check(
  'macOS still runs the full contract suite',
  /if: runner\.os == 'macOS'\n\s+run: npm test/.test(workflow),
  true,
);

// `tauri::generate_context!()` embeds ../dist at compile time, so any job that
// compiles the Tauri lib must build the frontend first — including the matrix
// job on Windows, where the contract suite moved to its own job.
check(
  'windows builds the frontend before compiling the Tauri lib',
  /if: runner\.os == 'Windows'\n\s+run: npm run build/.test(workflow),
  true,
);

// Execute the real wiring checks with changed text, without spawning Git again.
const source = readFileSync(fileURLToPath(import.meta.url), 'utf8');
const wiringStart = source.indexOf('const areas =');
const wiringEnd = source.indexOf('// Execute the real wiring checks');
const failureStart = source.lastIndexOf('\nif (failures > 0) {');
const wiring = source.slice(wiringStart, wiringEnd) + source.slice(failureStart);
for (const [label, changed] of [
  [
    'missing Windows frontend build',
    workflow.replace(
      /if: runner\.os == 'Windows'\n\s+run: npm run build/,
      "if: runner.os == 'Never'\n        run: npm run build",
    ),
  ],
  [
    'missing separate frontend job',
    workflow.replace('\n  windows-frontend:', '\n  removed-frontend:'),
  ],
]) {
  check(label + ': mutation applied', changed !== workflow, true);
  let exitCode = 0;
  try {
    vm.runInNewContext(
      'let failures = 0; const check = (label, actual, expected) => { if (actual !== expected) failures++; }; ' +
        wiring,
      {
        workflow: changed,
        reusable,
        vm,
        check: undefined,
        console: { log() {}, error() {} },
        process: {
          exit(code) {
            exitCode = code;
            throw new Error('expected contract failure');
          },
        },
      },
    );
  } catch (error) {
    if (!exitCode) throw error;
  }
  check(label + ': contract rejects the mutation', exitCode, 1);
}

if (failures > 0) {
  console.error(`ci-changed-areas contract test failed (${failures})`);
  process.exit(1);
}
console.log('ci-changed-areas contract test passed');
