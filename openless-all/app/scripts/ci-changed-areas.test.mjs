// Contract test for scripts/ci-changed-areas.sh.
//
// The CI jobs that verify the desktop Tauri builds and the Rust 1.88 MSRV are
// skipped for change sets that cannot reach them. That decision must never be
// made on a guess: this test drives the script against real git diffs and
// asserts both directions - out-of-scope change sets skip, everything else runs.
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
  // Documentation cannot reach either check.
  commit({ 'docs/architecture.md': 'docs\n' }, 'docs only');
  check('docs only: tauri', decide('tauri'), 'false');
  check('docs only: msrv', decide('msrv'), 'false');
  check('docs only: linux', decide('linux'), 'false');
  check('docs only: harmony', decide('harmony'), 'false');
  reset();

  // The Linux host is a separate crate: only its own Rust sources stay in.
  commit({ 'openless-all/app/linux-egui/src/ui/theme.rs': 'fn x() {}\n' }, 'linux host source');
  check('linux host source: tauri', decide('tauri'), 'false');
  check('linux host source: msrv', decide('msrv'), 'true');
  check('linux host source: linux', decide('linux'), 'true');
  check('linux host source: harmony', decide('harmony'), 'false');
  reset();

  // HarmonyOS work touches the shared bundle and the Rust host, never the Linux
  // package chain or the Tauri desktop build.
  commit(
    { 'openless-all/app/harmony/entry/src/main/ets/pages/Index.ets': 'export {};\n' },
    'arkts',
  );
  check('arkts source: harmony', decide('harmony'), 'true');
  check('arkts source: tauri', decide('tauri'), 'true');
  reset();

  // ...but its manifest moves the shared workspace dependency graph.
  commit(
    { 'openless-all/app/linux-egui/Cargo.toml': '[package]\nname = "x"\n' },
    'linux host manifest',
  );
  check('linux host manifest: tauri', decide('tauri'), 'true');
  check('linux host manifest: msrv', decide('msrv'), 'true');
  reset();

  // Frontend-only work still needs the Tauri job (it runs the frontend tests)
  // but cannot change what the Rust compiler has to accept.
  commit({ 'openless-all/app/src/pages/Style.tsx': 'export const x = 1;\n' }, 'frontend only');
  check('frontend only: tauri', decide('tauri'), 'true');
  check('frontend only: msrv', decide('msrv'), 'false');
  check('frontend only: linux', decide('linux'), 'true');
  check('frontend only: harmony', decide('harmony'), 'true');
  reset();

  // Native code keeps every check on.
  commit({ 'openless-all/app/src-tauri/src/lib.rs': 'pub fn x() {}\n' }, 'native source');
  check('native source: tauri', decide('tauri'), 'true');
  check('native source: msrv', decide('msrv'), 'true');
  reset();

  // A dependency bump moves both the Tauri build and the MSRV promise.
  commit({ 'openless-all/app/Cargo.lock': '# lock\n' }, 'lockfile');
  check('lockfile: tauri', decide('tauri'), 'true');
  check('lockfile: msrv', decide('msrv'), 'true');
  reset();

  // Workflow changes always verify everything.
  commit({ '.github/workflows/ci.yml': 'name: CI\n' }, 'workflow');
  check('workflow change: tauri', decide('tauri'), 'true');
  check('workflow change: msrv', decide('msrv'), 'true');
  check('workflow change: linux', decide('linux'), 'true');
  check('workflow change: harmony', decide('harmony'), 'true');
  reset();

  // Unreadable or empty change sets never skip a platform check.
  check('no base revision: tauri', decide('tauri', null), 'true');
  check('no base revision: msrv', decide('msrv', null), 'true');
  check('no base revision: linux', decide('linux', null), 'true');
  check('no base revision: harmony', decide('harmony', null), 'true');
  check('empty diff: tauri', decide('tauri'), 'true');
  check('empty diff: msrv', decide('msrv'), 'true');
  check('empty diff: linux', decide('linux'), 'true');
  check('empty diff: harmony', decide('harmony'), 'true');
  check('unknown revision: tauri', decide('tauri', 'deadbeefdeadbeef'), 'true');
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
const areas = ['tauri', 'msrv', 'linux', 'harmony'];
check(
  'workflow publishes every area',
  areas.every((area) => workflow.includes(area + ': ${{ steps.detect.outputs.' + area + ' }}')),
  true,
);
check(
  'workflow computes every area in the detect step',
  /for area in tauri msrv linux harmony; do/.test(workflow),
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
  /inputs\.platform != 'macos'\) && needs\.changes\.outputs\.tauri != 'false'/.test(workflow),
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

const windowsJob = workflow.match(/\n  windows-frontend:[\s\S]*?(?=\n  [a-z-]+:)/)?.[0] ?? '';
const capsuleJob = workflow.match(/\n  native-capsule:[\s\S]*?(?=\n  [a-z-]+:)/)?.[0] ?? '';
const windowsGate = windowsJob.match(/\n    if: (.+)/)?.[1];
const capsuleMatrix = capsuleJob.match(/\n        os: \$\{\{ (.+) \}\}/)?.[1];
for (const [event, platform, tauri, expectedWindows, expectedPlatforms] of [
  ['workflow_dispatch', 'macos', 'true', false, ['macos-latest']],
  ['workflow_dispatch', 'all', 'true', true, ['macos-latest', 'windows-latest']],
  ['pull_request', '', 'true', true, ['macos-latest', 'windows-latest']],
  ['push', '', undefined, true, ['macos-latest', 'windows-latest']],
  ['pull_request', '', 'false', false, ['macos-latest', 'windows-latest']],
]) {
  const context = {
    github: { event_name: event },
    inputs: { platform },
    needs: { changes: { outputs: { tauri } } },
    fromJSON: JSON.parse,
  };
  check(
    event + '/' + platform + ': Windows frontend gate',
    Boolean(vm.runInNewContext(windowsGate ?? 'false', context)),
    expectedWindows,
  );
  check(
    event + '/' + platform + ': capsule platforms',
    JSON.stringify(vm.runInNewContext(capsuleMatrix ?? '[]', context)),
    JSON.stringify(expectedPlatforms),
  );
}

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
