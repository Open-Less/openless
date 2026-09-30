import { readFile } from 'node:fs/promises';

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}: pattern ${pattern} not found`);
  }
}

// The contract function show_capsule_window_no_activate lives in the explicit Tauri Host Module.
// The contract must validate the copy that is actually compiled, otherwise tests can be green
// while production is broken.
const capsuleFocusRs = (
  await readFile(new URL('../src-tauri/src/coordinator/capsule_focus.rs', import.meta.url), 'utf-8')
).replace(/\r\n/g, '\n');
const coordinatorHostRs = (
  await readFile(new URL('../src-tauri/src/tauri_coordinator_host.rs', import.meta.url), 'utf-8')
).replace(/\r\n/g, '\n');
const functionMatch = coordinatorHostRs.match(
  /#\[cfg\(target_os = "macos"\)\]\s*(?:pub\((?:crate|super)\) )?fn show_capsule_window_no_activate[\s\S]*?\n}\n\n#\[cfg\(not\(any\(target_os = "macos", target_os = "windows"\)\)\)\]/,
);

if (!functionMatch) {
  throw new Error('macOS capsule no-activate function not found');
}

const macosNoActivateFunction = functionMatch[0];
const executableMacosNoActivateFunction = macosNoActivateFunction.replace(/\/\/.*$/gm, '');

assertMatch(
  macosNoActivateFunction,
  /CAN_JOIN_ALL_SPACES[\s\S]*?1 << 0[\s\S]*?setCollectionBehavior[\s\S]*?orderFrontRegardless/,
  'macOS capsule should join all Spaces via an absolute collectionBehavior write before showing without activation',
);

assertMatch(
  macosNoActivateFunction,
  /FULL_SCREEN_AUXILIARY[\s\S]*?1 << 8[\s\S]*?setCollectionBehavior[\s\S]*?orderFrontRegardless/,
  'macOS capsule should join fullscreen Spaces as an auxiliary window before showing without activation',
);

assertMatch(
  macosNoActivateFunction,
  /setLevel:\s*25[\s\S]*?orderFrontRegardless/,
  'macOS capsule must raise window level above the menu bar (25) so it renders over fullscreen apps, not just behind them',
);

for (const forbidden of ['window.show()', 'set_focus', 'NSApp.activate', 'makeKeyAndOrderFront']) {
  if (executableMacosNoActivateFunction.includes(forbidden)) {
    throw new Error(`macOS capsule no-activate path must not call ${forbidden}`);
  }
}

// === Contract: the capsule follows "the screen the mouse cursor is on" (multi-screen / Space) ===
// Root cause: positioning used the AX caret while the layout dedup cache used the capsule's own
// current_monitor — the two look at different screens. When the cursor moves to another screen,
// the cache misjudges "nothing changed" → skips repositioning → the capsule is locked to the
// first screen (only flashing on the other). After the fix both paths must share
// capsule_target_monitor, with the mouse cursor as the preferred signal. These invariants are
// guarded purely by source grep and cannot be covered by unit tests without multi-screen
// hardware — exactly what contract tests are for.
const libRs = (
  await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8')
).replace(/\r\n/g, '\n');

assertMatch(
  libRs,
  /fn capsule_target_monitor[\s\S]*?macos_mouse_cursor_point\(\)\s*\.or_else\(\s*macos_focused_input_anchor_point\s*\)/,
  'macOS capsule must resolve its target monitor from the mouse cursor first, AX caret only as fallback',
);

assertMatch(
  libRs,
  /follow the monitor under the mouse cursor[\s\S]*?if let Some\(mon\) = capsule_target_monitor\(window\)/,
  'macOS capsule positioning must follow capsule_target_monitor (the mouse screen), not its own current_monitor',
);

assertMatch(
  coordinatorHostRs,
  /#\[cfg\(target_os = "macos"\)\][\s\S]*?crate::capsule_target_monitor\(window\)/,
  'macOS capsule layout cache key must reuse capsule_target_monitor, or it will skip repositioning when the cursor moves to another screen',
);

// === Contract: a card that borrows the capsule window must return it fully (position!) ===
//
// The vocab suggestion card and the insert-fallback card have no windows of their own — they
// borrow the recording capsule's single "capsule" window, shrinking it to card size and moving it
// to the bottom-right while shown. On dismiss it must be returned exactly as it was.
//
// The "return the position" step was missed once; on real hardware, after using the
// add-word card once, the next recording's capsule appeared bottom-right and never went back to
// bottom-center. The miss was fatal because maybe_position_capsule_bottom_center's dedup cache
// only records "monitor + translation state" and knows nothing about the card moving the window —
// the next recording got the same monitor snapshot, judged "no change", and skipped repositioning
// outright. The window had been moved, while the only code that would move it back thought it had
// nothing to do.
//
// So both reset and cache invalidation are required, each blocking one direction; the same applies
// to passthrough state (emit_capsule relies on capsule_cursor_passthrough to skip redundant calls;
// if the cache and the window's real state diverge, a needed call gets skipped and the ✓/✕ buttons
// on the capsule stop responding).
//
// This fix was lost once in 2026-08 (it existed only on an unmerged local branch; main regrew the
// version missing the position restore) and unit tests cannot catch it — it is all Tauri window
// calls running inside main-thread closures. A contract test is the only way to pin it.
const coordinatorRs = (
  await readFile(new URL('../src-tauri/src/coordinator.rs', import.meta.url), 'utf-8')
).replace(/\r\n/g, '\n');
assertMatch(
  coordinatorHostRs,
  /fn set_cursor_passthrough[\s\S]*?window\.set_ignore_cursor_events\(passthrough\)[\s\S]*?cursor_passthrough[\s\S]*?store\(passthrough, Ordering::SeqCst\)/,
  'the narrow capsule-window capability must update the Tauri window and its host-owned passthrough cache together',
);

function extractFn(source, name) {
  const match = source.match(new RegExp(`(?:pub\\(crate\\) )?fn ${name}[\\s\\S]*?\\n}\\n`));
  if (!match) {
    throw new Error(`${name}: function not found in coordinator.rs`);
  }
  return match[0];
}

// Showing a card = moving the shared window away; the dedup cache must be invalidated on the spot.
for (const name of ['show_vocab_suggestion_card', 'show_insert_fallback_card']) {
  const body = extractFn(coordinatorRs, name);
  assertMatch(
    body,
    /capsule\.invalidate_layout\(\)/,
    `${name} moves the shared capsule window, so it must invalidate the capsule_layout dedup cache`,
  );
  assertMatch(
    body,
    /capsule\.set_cursor_passthrough\(false\)/,
    `${name} must change cursor passthrough through the host capability that keeps its cache in sync`,
  );
}

// Dismissing a card = returning the window fully: passthrough, size, position — nothing skipped.
for (const name of ['hide_vocab_suggestion_card', 'hide_insert_fallback_card']) {
  const body = extractFn(coordinatorRs, name);
  assertMatch(
    body,
    /set_cursor_passthrough\(true\)/,
    `${name} must restore cursor passthrough, or the capsule keeps blocking that strip of screen`,
  );
  assertMatch(
    body,
    /capsule_window_bounds\(false\)[\s\S]*?set_size/,
    `${name} must restore the capsule window size, or the next capsule is squeezed into a card-sized window`,
  );
  assertMatch(
    body,
    /capsule\.invalidate_layout\(\)/,
    `${name} must invalidate the capsule_layout dedup cache, or the next recording skips repositioning and the capsule stays bottom-right`,
  );
  assertMatch(
    body,
    /position_capsule_bottom_center\(false\)/,
    `${name} must move the capsule window back to bottom-center; restoring size alone leaves it in the card's bottom-right corner`,
  );
  // Ordering invariant: size and position must change together; changing them while the window is
  // still visible can composite a frame of "card stretched wide, still flying across half the
  // screen".
  const hideAt = body.indexOf('capsule.hide()');
  const resizeAt = body.indexOf('set_size');
  if (hideAt === -1 || hideAt > resizeAt) {
    throw new Error(
      `${name} must hide the window before changing its geometry, or the restore animates on screen`,
    );
  }
}
