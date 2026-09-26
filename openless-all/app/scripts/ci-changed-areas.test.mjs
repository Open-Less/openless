// Contract test for scripts/ci-changed-areas.sh.
//
// The CI jobs that verify the desktop Tauri builds and the Rust 1.88 MSRV are
// skipped for change sets that cannot reach them. That decision must never be
// made on a guess: this test drives the script against real git diffs and
// asserts both directions - out-of-scope change sets skip, everything else runs.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

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
  reset();

  // The Linux host is a separate crate: only its own Rust sources stay in.
  commit({ 'openless-all/app/linux-egui/src/ui/theme.rs': 'fn x() {}\n' }, 'linux host source');
  check('linux host source: tauri', decide('tauri'), 'false');
  check('linux host source: msrv', decide('msrv'), 'true');
  check('linux host source: linux', decide('linux'), 'true');
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
  reset();

  // Unreadable or empty change sets never skip a platform check.
  check('no base revision: tauri', decide('tauri', null), 'true');
  check('no base revision: msrv', decide('msrv', null), 'true');
  check('no base revision: linux', decide('linux', null), 'true');
  check('empty diff: tauri', decide('tauri'), 'true');
  check('empty diff: msrv', decide('msrv'), 'true');
  check('empty diff: linux', decide('linux'), 'true');
  check('unknown revision: tauri', decide('tauri', 'deadbeefdeadbeef'), 'true');
  check('unknown area', decide('nonsense'), 'true');
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

if (failures > 0) {
  console.error(`ci-changed-areas contract test failed (${failures})`);
  process.exit(1);
}
console.log('ci-changed-areas contract test passed');
