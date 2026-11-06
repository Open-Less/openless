import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import yaml from 'js-yaml';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const helper = read('../src-tauri/src/issue1266_debug.rs');
const lib = read('../src-tauri/src/lib.rs');
const recorder = read('../src-tauri/src/recorder.rs');
const loops = read('../src-tauri/src/coordinator/hotkey_loops.rs');
const coordinator = read('../src-tauri/src/coordinator.rs');
const host = read('../src-tauri/src/tauri_coordinator_host.rs');
const ime = read('../src-tauri/src/windows_ime_ipc.rs');
const ci = yaml.load(read('../../../.github/workflows/ci.yml'));
const release = yaml.load(read('../../../.github/workflows/release-tauri.yml'));

assert.match(read('../src-tauri/Cargo.toml'), /issue-1266-diagnostics = \[\]/);
assert.match(helper, /all\(target_os = "windows", feature = "issue-1266-diagnostics"\)/);
assert.match(helper, /cfg\(any\(test, all/);
assert.match(helper, /A1-owner-thread-target/);
assert.match(helper, /C1-windows-target-storage/);
assert.match(helper, /HandleCloneEntered/);
assert.match(helper, /HandleCloneReturned/);
assert.match(helper, /sync_channel\(512\)/);
assert.match(helper, /\.try_send\(record\)/);
assert.doesNotMatch(helper, /log::|thread::sleep|recv_timeout|truncate\(|\.send\(record\)/);
assert.match(helper, /openless-issue-1266-2e2c7a47-session\.ndjson/);
assert.equal((helper.match(/OpenOptions::new\(\)/g) ?? []).length, 1);
assert.match(helper, /create\(true\)\.append\(true\)/);
assert.match(helper, /drop\(self\.guard\.take\(\)\);\s*self\.operation\.record\(Event::Released/);
assert.match(helper, /"schema": "diagnosing-bugs.v1"/);
assert.match(helper, /"parent_operation_id"/);
assert.match(helper, /"event_seq"/);
assert.match(helper, /pub\(crate\) fn health/);
for (const field of [
  'dropped',
  'queue_closed',
  'legacy_unmapped',
  'spawn_failed',
  'io_failed',
  'written',
  'flushed',
]) {
  assert.match(helper, new RegExp(`${field}:`));
}
const probes = helper.match(/pub\(crate\) enum Probe \{([\s\S]*?)\n\}/)[1];
assert.deepEqual(
  [...probes.matchAll(/^\s+(P\d),/gm)].map((m) => m[1]),
  ['P1', 'P2', 'P3', 'P4', 'P5', 'P6'],
);
assert.match(coordinator, /issue1266_debug::lock_host\(&self\.inner\.settings_host_gate/);
assert.doesNotMatch(coordinator + loops, /settings_host_gate\.lock\(\)/);
assert.equal((loops.match(/lock_host\(&inner\.settings_host_gate/g) ?? []).length, 8);
// Both adapters target the same native queue; neither whole-file token presence nor
// the old host-only dispatcher is sufficient coverage of the reported capsule task.
const capsuleHost = host.slice(
  host.indexOf('impl TauriCapsuleWindow {'),
  host.indexOf('pub(crate) struct TauriCoordinatorHost'),
);
const coordinatorHost = host.slice(host.indexOf('pub(crate) struct TauriCoordinatorHost'));
assert.match(
  capsuleHost,
  /run_on_main_thread_observed\(crate::issue1266_debug::Kind::CapsuleDispatch, task\)/,
);
assert.match(capsuleHost, /MainDispatch::new\(kind\)/);
assert.match(
  capsuleHost,
  /self\.app\.run_on_main_thread\(dispatch\.wrap\(move \|\| task\(capsule\)\)\)/,
);
assert.match(capsuleHost, /Kind::MirrorDispatch/);
assert.match(capsuleHost, /Kind::LayoutDispatch/);
assert.match(coordinatorHost, /MainDispatch::new\(Kind::HostDispatch\)/);
assert.match(coordinatorHost, /app\.run_on_main_thread\(dispatch\.wrap\(task\)\)/);
// beta moved the Windows timer onto a handle-free host. Preserve that ownership
// boundary while retaining P2 LayoutDispatch provenance on the real queued task.
const layoutWatch = capsuleHost.slice(
  capsuleHost.indexOf('fn start_layout_watch('),
  capsuleHost.indexOf('fn refresh_watched_layout('),
);
const windowsLayout = layoutWatch.match(
  /#\[cfg\(target_os = "windows"\)\]\s*\{([\s\S]*?)\n                \}/,
)[1];
assert.match(
  windowsLayout,
  /host\.on_main_thread_observed\(crate::issue1266_debug::Kind::LayoutDispatch/,
);
assert.ok(windowsLayout.indexOf('move ||') < windowsLayout.indexOf('capsule_window()'));
assert.doesNotMatch(windowsLayout, /self\.clone\(|capsule\.clone\(|self\.app/);
assert.match(
  layoutWatch,
  /#\[cfg\(not\(target_os = "windows"\)\)\]\s*let capsule = self\.clone\(\)/,
);
// Bound this assertion by the method's own closing indentation, not the name
// of an unrelated next method (beta may rename/remove that neighbor).
function rustMethod(source, signature) {
  const start = source.indexOf(signature);
  assert.ok(start >= 0, `missing Rust method: ${signature}`);
  const end = source.indexOf('\n    }', start);
  assert.ok(end >= 0, `missing Rust method boundary: ${signature}`);
  return source.slice(start, end + '\n    }'.length);
}
const extractionFixture = `impl Host {
    fn observed() {
        if ready { send(); }
    }
    fn unrelated() { app.clone(); }
}`;
assert.equal(
  rustMethod(extractionFixture, 'fn observed('),
  'fn observed() {\n        if ready { send(); }\n    }',
);
assert.throws(() => rustMethod(extractionFixture, 'fn missing('), /missing Rust method:/);
assert.throws(
  () => rustMethod('    fn unclosed() {', 'fn unclosed('),
  /missing Rust method boundary:/,
);
const borrowedDispatch = rustMethod(coordinatorHost, 'fn on_main_thread_observed<');
assert.match(borrowedDispatch, /MainDispatch::new\(kind\)/);
assert.match(borrowedDispatch, /let task = dispatch\.wrap\(task\)/);
assert.match(borrowedDispatch, /self\.app\.lock\(\)\.as_ref\(\)/);
assert.match(borrowedDispatch, /app\.run_on_main_thread\(task\)\.is_ok\(\)/);
assert.match(borrowedDispatch, /dispatch\.dispatched\(queued\)/);
assert.doesNotMatch(borrowedDispatch, /clone_handle|self\.app\(\)|app\.clone\(/);
assert.match(helper, /EXECUTING\.with\(Cell::get\)/);
assert.match(helper, /current\.replace\(Some\(self\.operation\)\)/);
assert.match(helper, /current\.set\(self\.previous\)/);
assert.match(
  helper,
  /self\.record\(Event::Entered, Outcome::Observed\);\s*task\(\);\s*self\.record\(Event::Returned, Outcome::Returned\)/,
);
assert.match(helper, /thread_label: main_dispatch::thread_label\(\)/);
assert.match(helper, /"thread_label": record\.thread_label/);
assert.doesNotMatch(helper, /GetCurrentThreadId|std::thread::current|std::thread::ThreadId/);
assert.match(loops, /DebugEvent::Consumed/);
assert.match(loops, /handle_pressed_edge[\s\S]*?DebugEvent::HandlerReturned/);
assert.match(loops, /DebugEvent::AckTimedOut/);
assert.match(ime, /spawn_blocking\(move \|\| \{[\s\S]*?operation\.mark\(Event::Entered\)/);
assert.match(ime, /Event::SubmitAck/);
assert.match(ime, /error\.is_outcome_unknown\(\) => Outcome::Unknown/);
assert.match(recorder, /if let Some\(handle\) = join_handle\.lock\(\)\.take\(\) \{/);
assert.match(recorder, /if let Some\(handle\) = self\.join_handle\.lock\(\)\.take\(\) \{/);
assert.match(recorder, /not\(feature = "issue-1266-diagnostics"\), not\(test\)/);
assert.match(recorder, /issue1266_debug::legacy\(event, outcome\)/);
assert.ok(
  lib.indexOf('issue1266_debug::initialize();') <
    lib.indexOf('tls_crypto::install_tls_crypto_provider();'),
);
assert.match(
  lib,
  /feature = "issue-1266-diagnostics"\)\)\]\s*#\[tauri::command\]\s*fn issue1266_health/,
);
assert.match(lib, /observe_issue1266_lifecycle\(&event\);\s*match event/);

const ciSteps = Object.values(ci.jobs).flatMap((job) => job.steps ?? []);
assert.ok(ciSteps.some((step) => /test-windows-hang-diagnostics\.py/.test(step.run ?? '')));
assert.ok(
  ciSteps.some((step) => /cargo check.*--features issue-1266-diagnostics/.test(step.run ?? '')),
);
const steps = release.jobs.build.steps;
assert.equal(release.on.workflow_dispatch.inputs.windows_hang_diagnostics.default, false);
assert.equal(
  steps.find((step) => step.uses === 'actions/checkout@v4').with.ref,
  '${{ inputs.release_tag || github.sha }}',
);
const gate = release.jobs.build.env.OPENLESS_HANG_DIAGNOSTICS;
for (const fragment of [
  'workflow_dispatch',
  'windows-latest',
  'refs/heads/',
  "inputs.release_tag == ''",
])
  assert.ok(gate.includes(fragment));
const build = steps.find((step) => step.name === 'Build (Windows)').run;
assert.match(build, /diagnostic_args=\(--features issue-1266-diagnostics\)/);
assert.equal((build.match(/build "\$\{diagnostic_args\[@\]\}" --bundles/g) ?? []).length, 4);
const verify = steps.find((step) => step.name === 'Verify Windows diagnostic symbols');
assert.match(verify.run, /windows-symbol-pair\.py/);
const symbolTooling = steps.find((step) => step.name === 'Fetch symbol-pair verification tooling');
assert.equal(symbolTooling.env.TOOLING_SHA, '${{ github.workflow_sha }}');
assert.match(symbolTooling.run, /--method GET -f "ref=\$env:TOOLING_SHA"/);
assert.match(symbolTooling.run, /LASTEXITCODE -ne 0/);
assert.match(symbolTooling.run, /bytes\.Length -eq 0/);
assert.match(
  verify.run,
  /python -I release-tooling\/openless-all\/app\/scripts\/windows-symbol-pair\.py/,
);
assert.ok(steps.indexOf(symbolTooling) < steps.indexOf(verify));

assert.match(verify.run, /LASTEXITCODE -ne 0/);
const upload = steps.find((step) => step.name === 'Upload Windows artifacts');
assert.match(upload.with.name, /issue-1266-diagnostics/);
assert.match(upload.with.path, /openless-symbol-pair\.json/);
assert.match(
  steps.find((step) => step.name === 'Write Windows artifact provenance').run,
  /issue_1266_diagnostics/,
);
