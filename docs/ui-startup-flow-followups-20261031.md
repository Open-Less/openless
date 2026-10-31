# OpenLess UI and startup follow-up checklist

Branch: `appergb/ui-startup-flow-followups-20261031`
Base: `beta` at `85b243abd83abb295cd99e0ec31ba483c3f5c6fe`

This checklist turns the outstanding UI and startup feedback into observable work. Each item is closed only after source changes, a focused contract or browser test, and a cloud CI result.

## P0 — visible defects and startup correctness

- [x] **Streaming transcript capsule geometry** *(source fix complete; cloud UI verification pending)*
  - Remove the left-edge clipping/sharpening source in the transcript rail and streaming pill.
  - Verify all capsule styles in light and dark themes, including the Windows rail.
- [ ] **Translation target interaction**
  - Low-memory mode was removed in PR #1296; do not recreate a separate low-memory path.
  - Verify the remaining translation target control opens a usable popup from the capsule and keeps the selected target visible after selection.
- [x] **Settings first-open flash** *(source fix complete; first-open browser verification pending)*
  - Prevent the previous settings surface or a blank/full-window frame from painting before the current settings surface is ready.
  - Verify first open, close, and second open on macOS and Windows.
- [x] **macOS first launch and missing-permission flow** *(source fix and contract test complete; macOS runtime verification pending)*
  - If microphone or Accessibility permission is missing on first launch/reinstall, show the main window even when `start_minimized` is enabled.
  - Enter the guided permission step automatically, re-check after returning from System Settings, and keep the step retryable.
  - Keep silent start for already-authorized installations.
  - Make the TCC reset/re-request behavior explicit and bounded to the authorization action.

## P1 — motion and visual system

- [x] **Page transition mask and timing** *(mask source fix complete; motion suite verification pending)*
  - Keep outgoing content covered until the next page has its first useful frame.
  - Use the normal 0.2–0.3s transition and a slower first-load path only when required.
  - Verify no flash, jump, or double paint during settings/category navigation.
- [x] **Streaming text motion** *(per-glyph blur timing tuned; long-stream verification pending)*
  - Apply per-glyph blur/opacity reveal without causing layout jumps.
  - Verify reduced-motion behavior and long transcript performance.
- [ ] **High-definition UI tokens**
  - Replace strong 1px borders with micro-contrast surfaces.
  - Reduce dark-mode weights by 40 where the current token is too heavy.
  - Lock numeric readouts to `tabular-nums`.
  - Keep snapping over-damped and positive feedback under-damped.
- [partial] **Control selection rules**
  - Theme now uses a three-option segmented control; language now uses a searchable single-select popup.
  - Use segmented buttons for two or three fixed choices.
  - Use switches for booleans.
  - Use searchable, grouped single-select popups for large lists.
  - Keep popups within the viewport and report the result after selection.
- [ ] **Toast and alert hierarchy**
  - Auto-dismiss routine setting feedback after about 0.8s.
  - Keep sync failures and maintenance notices as persistent Alerts.

## P2 — performance and cleanup

- [ ] **Memory measurement**
  - Capture a repeatable cold-start and steady-state baseline before optimizing.
  - Optimize only after identifying a measurable allocation or resident-memory source.
- [ ] **Regression coverage**
  - Add focused tests for the capsule radius, settings first-open, page transitions, permission gate, popup placement, and motion reduction.
- [ ] **Cloud validation and delivery**
  - Push a dedicated PR from this branch.
  - Require all CI and current-head Codex review to pass.
  - Build the final macOS ARM64 artifact in GitHub Actions only; do not create a GitHub Release.

## Explicitly out of scope

The retired low-memory product mode, its preference, IPC, native helper build path, and low-memory-specific translation branch remain removed. The translation popup work applies to the shared active capsule path.
