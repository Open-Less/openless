// Contract test for the Linux egui logging backend.
//
// Logs are the only thing a user can hand us when the Linux host misbehaves, so
// the backend has to satisfy three properties that are easy to lose in a
// refactor:
//
//   1. The level filter is reachable without a rebuild (`OPENLESS_LOG`).
//   2. Periodic liveness chatter never sits at `info`: a 10 s heartbeat is 8,640
//      lines a day and pushed every real message out of the rotated files.
//   3. `error` means failure. A `stage=` progress tracer logged at `error` makes
//      every startup look broken.
//
// The Rust sources are read as text: this test runs in the Linux contract job
// and must not need a compiled tree.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (relative) => readFileSync(join(appRoot, relative), 'utf8');

const logging = read('linux-egui/src/logging.rs');
const main = read('linux-egui/src/main.rs');
const window = read('linux-egui/src/linux_app/window.rs');
const adapter = read('crates/openless-core/src/cloud_sync_e2ee/adapter.rs');

// 1. Level filter from the environment, not a constant baked in at build time.
assert.match(
  logging,
  /"OPENLESS_LOG"/,
  'logging.rs must read OPENLESS_LOG so a user can turn on debug without a rebuild',
);
assert.match(
  logging,
  /"RUST_LOG"/,
  'logging.rs must keep honouring RUST_LOG for people who expect the Rust convention',
);
assert.match(
  logging,
  /log::set_boxed_logger/,
  'logging.rs must install a logger that applies the parsed filter',
);
assert.ok(
  !/LevelFilter::Info,\s*\n\s*config\.clone\(\),\s*\n\s*TerminalMode/.test(logging),
  'the terminal sink must not be hard-wired to Info any more',
);

// 2. Rotation keeps more than a single archive, otherwise one busy day erases
//    the incident we are trying to diagnose.
assert.match(
  logging,
  /const ROTATE_KEEP: usize = [2-9]/,
  'logging.rs must keep at least two log archives',
);

// 3. The pump heartbeat is throttled: state changes at `info`, the rest at
//    `debug`, plus a low-frequency liveness line.
assert.match(
  main,
  /const PUMP_HEARTBEAT_INFO_INTERVAL[\s\S]{0,80}from_secs\(\d{2,}\)/,
  'the pump heartbeat needs an info-level interval of at least tens of seconds',
);
assert.ok(
  !/log::info!\(\s*\n?\s*"\[pump\] heartbeat/.test(main),
  'the 10 s pump heartbeat must not be logged at info unconditionally',
);
assert.match(
  main,
  /log::debug!\("\{message\}"\)/,
  'the throttled heartbeat must fall back to debug',
);

// 4. The IPC probe is a gauge, not an event.
assert.ok(
  !/log::info!\(\s*\n?\s*"\[ui-client\] ipc round-trip/.test(window),
  'the IPC round-trip probe must stay at debug',
);

// 5. No progress tracer disguised as a failure.
assert.ok(
  !/log::error!\(\s*"\[e2ee-adapter\] stage=/.test(adapter),
  'the e2ee adapter must not log its stage tracers at error level',
);
assert.ok(
  !/log::error!\(\s*\n\s*"\[e2ee-adapter\] build start/.test(adapter),
  'the e2ee adapter must not log its build start marker at error level',
);

// 6. Nothing that looks like a secret may be formatted into a log line.
const logCalls = [...`${main}\n${window}\n${adapter}`.matchAll(/log::(?:error|warn|info|debug|trace)!\(([\s\S]{0,400}?)\);/g)];
for (const [, body] of logCalls) {
  assert.ok(
    !/\b(?:api_key|apikey|access_token|refresh_token|client_secret|password|private_key)\s*=/i.test(
      body,
    ),
    `a log line formats what looks like a secret: ${body.trim().slice(0, 120)}`,
  );
}

console.log('egui-logging contract test passed');
