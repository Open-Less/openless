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


// 7. The level chosen in the UI has to outlive the process, and the export has to
//    carry the rotated history - otherwise the file the user sends us is exactly
//    the one that no longer contains the incident.
const settingsSave = read('linux-egui/src/linux_app/settings_save.rs');
assert.match(
  logging,
  /pub fn write_log_level/,
  'logging.rs must persist the level chosen in the settings UI',
);
assert.match(
  logging,
  /pub fn read_log_level/,
  'logging.rs must read the persisted level at startup',
);
assert.match(
  settingsSave,
  /write_log_level\(/,
  'the settings handler must persist the log level',
);
assert.match(
  settingsSave,
  /export_error_logs\(/,
  'the export action must include the rotated logs, not only the current file',
);
assert.match(
  logging,
  /pub fn export_error_logs/,
  'logging.rs must provide the multi-file export',
);

// 8. The gaps that used to make a report unusable: no capture line on success, no
//    dictation start, no single-instance outcome.
assert.match(
  read('linux-egui/src/audio.rs'),
  /"\[audio\] capture started backend=/,
  'capture start must name the backend, device and format that actually won',
);
assert.match(
  read('crates/openless-core/src/api.rs'),
  /"\[dictation\] start session_id=/,
  'the dictation start boundary must be logged (hotkey pressed but nothing happened)',
);
assert.match(
  main,
  /"\[startup\] another instance owns the lock/,
  'a forwarded second launch must leave a trace, otherwise "clicking the icon does nothing" is invisible',
);

// 9. Per-event chatter must not sit at info.
assert.ok(
  !/log::info!\("\[transcript\] partial emitted/.test(
    read('crates/openless-core/src/dictation_engine.rs'),
  ),
  'each streaming partial must not be logged at info',
);
assert.ok(
  !/log::info!\("\[hotkey\] local edge/.test(main),
  'per-keystroke hotkey edges must stay at debug',
);

console.log('egui-logging contract test passed');
