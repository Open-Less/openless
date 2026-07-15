import { spawnSync } from 'node:child_process';

if (process.platform === 'win32') {
  // The full Tauri lib-test binary is compile-only on clean Windows runners because an
  // optional native runtime DLL is unavailable there. CI executes this behavioral gate on
  // macOS/Linux and still compiles the same Rust test on Windows via `cargo test --no-run`.
  console.log('Hotkey injection runtime gate skipped on Windows (covered by lib-test compilation).');
  process.exit(0);
}

const result = spawnSync(
  'cargo',
  ['test', '--manifest-path', 'src-tauri/Cargo.toml', 'hotkey_injection_gate_logs_pressed_and_cancels', '--', '--nocapture'],
  {
    env: { ...process.env, OPENLESS_HOTKEY_INJECTION_DRY_RUN: '1' },
    encoding: 'utf8',
  },
);

const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
process.stdout.write(result.stdout ?? '');
process.stderr.write(result.stderr ?? '');

if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

if (!output.includes('[coord] hotkey pressed')) {
  console.error("Hotkey injection gate did not emit '[coord] hotkey pressed'.");
  process.exit(1);
}

console.log('Hotkey injection gate passed.');
