import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { load } from 'js-yaml';

const workflow = (name) =>
  load(readFileSync(new URL(`../../../.github/workflows/${name}`, import.meta.url), 'utf8'));
const ci = workflow('ci.yml');
const desktop = workflow('release-tauri.yml');
const browserGate = ci.jobs['ui-motion'].steps[0].run;
const windowsGate = ci.jobs['windows-checks'].steps[0].run;
const runGate = (script, env) =>
  spawnSync('bash', ['-e', '-c', script], {
    env: { ...process.env, ...env },
    encoding: 'utf8',
    timeout: 5000,
  }).status;

// Exercise the actual shell gates: failure, cancellation and unexpected skips
// must not turn an existing required check green after parallelising its work.
for (const outcome of ['success', 'failure', 'cancelled', 'skipped']) {
  assert.equal(
    runGate(browserGate, {
      CHANGES_RESULT: 'success',
      FRONTEND: 'true',
      MACOS_ONLY: 'false',
      BROWSERS_RESULT: outcome,
    }),
    outcome === 'success' ? 0 : 1,
    `browser gate: ${outcome}`,
  );
  for (const compiler of ['STABLE_RESULT', 'MSRV_RESULT']) {
    assert.equal(
      runGate(windowsGate, {
        CHANGES_RESULT: 'success',
        TAURI: 'true',
        MSRV: 'true',
        STABLE_RESULT: 'success',
        MSRV_RESULT: 'success',
        [compiler]: outcome,
      }),
      outcome === 'success' ? 0 : 1,
      `${compiler}: ${outcome}`,
    );
  }
}
assert.equal(
  runGate(browserGate, {
    CHANGES_RESULT: 'success',
    FRONTEND: 'false',
    MACOS_ONLY: 'false',
    BROWSERS_RESULT: 'skipped',
  }),
  0,
);
assert.equal(
  runGate(browserGate, {
    CHANGES_RESULT: 'failure',
    FRONTEND: '',
    MACOS_ONLY: 'false',
    BROWSERS_RESULT: 'skipped',
  }),
  1,
);
assert.equal(
  runGate(windowsGate, {
    CHANGES_RESULT: 'success',
    TAURI: 'false',
    MSRV: 'false',
    STABLE_RESULT: 'skipped',
    MSRV_RESULT: 'skipped',
  }),
  0,
);
assert.equal(
  runGate(windowsGate, {
    CHANGES_RESULT: 'success',
    TAURI: 'false',
    MSRV: 'true',
    STABLE_RESULT: 'skipped',
    MSRV_RESULT: 'success',
  }),
  0,
);

// Exercise the actual CI Cargo commands against two tiny offline packages.
// Switching the diagnostics compile back to `check` or giving backend tests
// their own target directory must lose the dependency reuse this change needs.
const cargoScratch = mkdtempSync(join(tmpdir(), 'openless-cargo-reuse-'));
try {
  for (const path of ['host/src', 'shared/src', 'backend/src']) {
    mkdirSync(join(cargoScratch, path), { recursive: true });
  }
  writeFileSync(
    join(cargoScratch, 'host/Cargo.toml'),
    `[package]
name = "ci-profile-host"
version = "0.1.0"
edition = "2021"
[features]
issue-1266-diagnostics = []
[dependencies]
ci-shared = { path = "../shared" }
`,
  );
  writeFileSync(
    join(cargoScratch, 'shared/Cargo.toml'),
    '[package]\nname = "ci-shared"\nversion = "0.1.0"\nedition = "2021"\n',
  );
  writeFileSync(join(cargoScratch, 'shared/src/lib.rs'), 'pub fn value() -> u32 { 42 }');
  writeFileSync(
    join(cargoScratch, 'host/src/lib.rs'),
    'pub fn run() -> u32 { ci_shared::value() }',
  );
  writeFileSync(
    join(cargoScratch, 'host/src/main.rs'),
    'fn main() { assert_eq!(ci_profile_host::run(), 42); }',
  );
  writeFileSync(
    join(cargoScratch, 'backend/Cargo.toml'),
    '[package]\nname = "ci-backend-tests"\nversion = "0.1.0"\nedition = "2021"\n' +
      '[dependencies]\nci-shared = { path = "../shared" }\n',
  );
  writeFileSync(
    join(cargoScratch, 'backend/src/lib.rs'),
    '#[test] fn shared_contract() { assert_eq!(ci_shared::value(), 42); }',
  );
  const cargoEnv = { ...process.env, CARGO_PROFILE_TEST_DEBUG: '0' };
  delete cargoEnv.CARGO_TARGET_DIR;
  const cargo = (args) =>
    spawnSync('cargo', [...args, '--offline'], {
      cwd: cargoScratch,
      env: cargoEnv,
      encoding: 'utf8',
      timeout: 30_000,
    });
  const expectSuccess = (result) => {
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    return result;
  };
  expectSuccess(
    cargo(['test', '--manifest-path', 'host/Cargo.toml', '--lib', '--bins', '--no-run']),
  );
  expectSuccess(cargo(['generate-lockfile', '--manifest-path', 'backend/Cargo.toml']));
  const ciArgs = (stepName) =>
    ci.jobs['cross-platform'].steps
      .find((step) => step.name === stepName)
      .run.trim()
      .split(/\s+/)
      .slice(1)
      .map((arg) =>
        arg === 'src-tauri/Cargo.toml'
          ? join(cargoScratch, 'host/Cargo.toml')
          : arg === 'src-tauri/backend-tests/Cargo.toml'
            ? join(cargoScratch, 'backend/Cargo.toml')
            : arg === 'src-tauri/target'
              ? join(cargoScratch, 'host/target')
              : arg,
      );
  const diagnosticsArgs = ciArgs('Compile opt-in Windows hang diagnostics host');
  for (const args of [diagnosticsArgs, ciArgs('Run Rust-only backend unit tests (Windows)')]) {
    const result = expectSuccess(cargo([...args, '--message-format=json']));
    const artifacts = result.stdout
      .split('\n')
      .filter((line) => line.startsWith('{'))
      .map((line) => JSON.parse(line))
      .filter((record) => record.reason === 'compiler-artifact');
    assert.equal(
      artifacts.find((record) => record.target.name === 'ci_shared')?.fresh,
      true,
      `${args.join(' ')} must reuse the previously compiled dependency`,
    );
    if (args === diagnosticsArgs) {
      assert.ok(
        artifacts.some((record) => record.target.kind.includes('bin') && !record.profile.test),
        'diagnostics must link the production binary, not only a test harness',
      );
    }
  }
  writeFileSync(
    join(cargoScratch, 'host/src/lib.rs'),
    '#[cfg(all(feature = "issue-1266-diagnostics", not(test)))]\n' +
      'compile_error!("production diagnostics checked");\n' +
      'pub fn run() -> u32 { ci_shared::value() }',
  );
  const broken = cargo(diagnosticsArgs);
  assert.ifError(broken.error);
  assert.notEqual(broken.status, 0);
  assert.match(broken.stderr, /production diagnostics checked/);
} finally {
  rmSync(cargoScratch, { recursive: true, force: true });
}

// Run the real Windows build shell with only npm stubbed. Check artifact-flow
// decisions, not the spelling of the commands: one compile, optional rebundle,
// and no MSI attempt after a failed NSIS build.
const buildWindows = desktop.jobs.build.steps.find((s) => s.name === 'Build (Windows)').run;
const scratch = mkdtempSync(join(tmpdir(), 'openless-installer-flow-'));
const shellPath = (value) =>
  process.platform === 'win32'
    ? value.replaceAll('\\', '/').replace(/^([a-z]):/i, (_, drive) => '/' + drive.toLowerCase())
    : value;
try {
  mkdirSync(join(scratch, 'src-tauri'));
  mkdirSync(join(scratch, 'bin'));
  const npm = join(scratch, 'bin/npm');
  writeFileSync(
    npm,
    `#!/usr/bin/env bash
printf '%s\\n' "$*" >> "$NPM_CALLS"
case "$*" in
  *'-- build '*) exit "\${NSIS_EXIT:-0}" ;;
  *'-- bundle '*) exit "\${MSI_EXIT:-0}" ;;
  *) exit 99 ;;
esac
`,
  );
  chmodSync(npm, 0o755);
  for (const [name, version, channel, signed, nsis, msi, expectedCalls, expectedExit] of [
    ['stable-signed', '2.0.2', 'stable', true, 0, 0, 2, 0],
    ['stable-unsigned', '2.0.2+build.123', 'stable', false, 0, 0, 2, 0],
    ['beta-channel', '2.0.2', 'beta', true, 0, 0, 1, 0],
    ['beta-version', '2.0.2-Beta.4', 'stable', false, 0, 0, 1, 0],
    ['nsis-failure', '2.0.2', 'stable', true, 23, 0, 1, 23],
    ['msi-repair', '2.0.2', 'stable', true, 0, 17, 2, 0],
  ]) {
    writeFileSync(join(scratch, 'src-tauri/tauri.conf.json'), JSON.stringify({ version }));
    const callsPath = join(scratch, `${name}.calls`);
    const envPath = join(scratch, `${name}.env`);
    const result = spawnSync('bash', ['-c', 'export PATH="$TEST_BIN:$PATH"\n' + buildWindows], {
      cwd: scratch,
      encoding: 'utf8',
      timeout: 10000,
      env: {
        ...process.env,
        TEST_BIN: shellPath(join(scratch, 'bin')),
        NPM_CALLS: shellPath(callsPath),
        GITHUB_ENV: shellPath(envPath),
        TAURI_SIGNING_PRIVATE_KEY: signed ? 'fixture' : '',
        OPENLESS_HANG_DIAGNOSTICS: 'true',
        OPENLESS_RELEASE_CHANNEL: channel,
        NSIS_EXIT: String(nsis),
        MSI_EXIT: String(msi),
      },
    });
    assert.ifError(result.error);
    assert.equal(result.status, expectedExit, name + ': ' + result.stdout + result.stderr);
    const calls = readFileSync(callsPath, 'utf8').trim().split('\n');
    assert.equal(calls.length, expectedCalls, name);
    assert.match(calls[0], /-- build --features issue-1266-diagnostics --bundles nsis/);
    assert.match(calls[0], /-- --locked --timings$/);
    if (expectedCalls === 2) {
      assert.match(calls[1], /-- bundle --features issue-1266-diagnostics --bundles msi/);
      assert.equal(
        JSON.parse(readFileSync(join(scratch, 'src-tauri/tauri.msi-version.json'))).bundle.windows
          .wix.version,
        '2.0.2.0',
      );
      assert.match(readFileSync(envPath, 'utf8'), new RegExp(`TAURI_BUILD_EXIT=${msi}`));
    }
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
console.log('CI gates, Cargo reuse and Windows installer failure propagation passed');
