import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import yaml from 'js-yaml';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const runtime = read('../src-tauri/vendor/tauri-runtime-wry/src/lib.rs');
const owner = read('../src-tauri/vendor/tauri-runtime-wry/src/owner_thread.rs');
const manifest = read('../src-tauri/Cargo.toml');
const lock = read('../src-tauri/Cargo.lock').replace(/\r\n/g, '\n');
const host = read('../src-tauri/src/tauri_coordinator_host.rs');
const harness = read('./test-windows-runtime-target.py');
const ci = yaml.load(read('../../../.github/workflows/ci.yml'));

// A registry-owned target fixture must fail this same shape oracle. This is a
// structural regression check, not a claim to reproduce native heap corruption.
function hasOwnerBoundTarget(source) {
  return (
    /pub window_target: owner_thread::Handle<EventLoopWindowTarget<Message<T>>>/.test(source) &&
    /window_target_owner: owner_thread::Owner<EventLoopWindowTarget<Message<T>>>/.test(source) &&
    /window_target: window_target_owner\.handle\(\)/.test(source)
  );
}
assert.equal(hasOwnerBoundTarget('pub window_target: EventLoopWindowTarget<Message<T>>,'), false);
assert.ok(hasOwnerBoundTarget(runtime));
assert.match(
  manifest,
  /\[patch\.crates-io\]\s*tauri-runtime-wry = \{ path = "vendor\/tauri-runtime-wry" \}/,
);
const locked = lock.match(
  /name = "tauri-runtime-wry"\nversion = "2\.12\.2"([\s\S]*?)(?=\n\[\[package\]\])/,
)[1];
assert.doesNotMatch(locked, /source =|checksum =/);
assert.match(owner, /PhantomData<Rc<T>>/);
assert.match(owner, /PhantomData<fn\(\) -> T>/);
assert.match(owner, /Rc<dyn Any>/);
assert.match(owner, /Ordering::Release/);
assert.match(owner, /Ordering::Acquire/);
assert.match(owner, /VALUES\.try_with/);
assert.doesNotMatch(owner, /unsafe|Mutex|thread::sleep|process::exit/);
assert.ok(
  owner.indexOf('thread::current().id() != self.metadata.owner') <
    owner.indexOf('let value = VALUES.try_with'),
);
assert.match(owner, /let removed = VALUES\.try_with[\s\S]*?;\s*drop\(removed\)/);
assert.match(
  runtime,
  /#\[cfg\(not\(windows\)\)\]\s*pub window_target: EventLoopWindowTarget<Message<T>>/,
);
assert.match(runtime, /#\[cfg\(windows\)\]\s*\{ Ok\(DisplayHandle::windows\(\)\) \}/);
assert.match(
  runtime,
  /let code = event_loop\.run_return\(event_handler\);\s*drop\(owner\);\s*std::process::exit\(code\)/,
);
assert.match(
  runtime,
  /let code = self\.event_loop\.run_return\(event_handler\);\s*#\[cfg\(windows\)\]\s*drop\(owner\)/,
);
assert.match(runtime, /window_target\.with\(dispatch\)\.ok_or\(Error::EventLoopClosed\)/);
assert.match(runtime, /if !self\.main_thread\.window_target\.is_alive\(\)/);
assert.match(
  host,
  /MainDispatch::new\(kind\);\s*let capsule = dispatch\.clone_handle\(\|\| self\.clone\(\)\)/,
);
assert.match(
  host,
  /MainDispatch::new\(Kind::HostDispatch\);\s*let app = dispatch\.clone_handle\(\|\| self\.app\(\)\)/,
);
assert.match(harness, /cargo\(root, 'test', '--lib'\)/);
assert.match(harness, /cargo\(root, 'build', '--locked'\)/);
assert.match(harness, /package\['source'\] is None/);
assert.match(harness, /'tracing' not in/);
const steps = Object.values(ci.jobs).flatMap((job) => job.steps ?? []);
assert.ok(steps.some((step) => step.run === 'python scripts/test-windows-runtime-target.py'));
assert.ok(
  steps.some(
    (step) =>
      step.run === 'python scripts/test-windows-runtime-target.py --runtime' &&
      step.if === "runner.os == 'Windows'",
  ),
);
