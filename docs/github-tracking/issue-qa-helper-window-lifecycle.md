## Symptom

QA panel is not yet backed by one cross-platform helper-window contract.

What macOS currently expresses as a natural product behavior is:

- non-activating helper window
- does not steal the source app context
- draggable by the toolbar/background area
- dismiss means non-participating, not only visually hidden

Windows currently only matches part of that behavior. Latest local evidence on Windows:

- `Ctrl+Shift+;` works
- selection + follow-up QA flow works
- QA panel is still **not draggable**

This narrows the problem from a broad helper-window risk into a concrete gap:

```text
Windows does not yet carry the draggable semantics of the original macOS helper-window design.
```

## Evidence

- [openless-all/app/src-tauri/tauri.conf.json](/D:/Users/cooper/Practice-Project/202604/openless/openless-all/app/src-tauri/tauri.conf.json)
  - `qa` uses `transparent + alwaysOnTop + focus:false + visible:false + skipTaskbar`
- [openless-all/app/src-tauri/src/lib.rs](/D:/Users/cooper/Practice-Project/202604/openless/openless-all/app/src-tauri/src/lib.rs)
  - macOS path uses `orderFrontRegardless`
  - Windows path uses non-activating show/hide combinations
- [openless-all/app/src/pages/QaPanel.tsx](/D:/Users/cooper/Practice-Project/202604/openless/openless-all/app/src/pages/QaPanel.tsx)
  - toolbar drag path was attempted in frontend and via runtime drag APIs
- Tauri/Wry/Tao runtime reading:
  - `start_dragging()` still maps to a classic `drag_window()` / caption-drag path on Windows
- Windows local testing:
  - hotkey path OK
  - panel drag still fails

## Root Cause Convergence

This is no longer best explained as a missing React handler or a missing JS API call.

Current best root-cause statement:

```text
The Windows QA panel is shown as a non-activating helper window,
but it does not gain a native draggable contract equivalent to
macOS movableByWindowBackground semantics at window-creation /
message-processing level.
```

In other words:

- macOS product intent: non-activating **and** draggable
- Windows implementation today: partially non-activating, not truly draggable

## 5 Whys

1. Why is the QA panel not draggable on Windows?
   - Because it lacks an OS-level draggable contract.
2. Why were frontend `startDragging()` and backend `start_dragging()` not enough?
   - Because they still collapse back to ordinary caption-drag semantics.
3. Why is that insufficient here?
   - Because QA is not a normal activated desktop window; it is a transparent, topmost, skip-taskbar helper window.
4. Why does this diverge from the original macOS intent?
   - Because macOS naturally supports "non-activating but draggable" for this window pattern, while Windows does not get that for free from the same implementation shape.
5. Why is this parked instead of force-fixed now?
   - Because the evidence says the next meaningful fix must move into native window creation / message strategy, not more local toolbar workarounds.

## Platform Scope

- Direct symptom: confirmed on Windows
- Problem layer: helper-window lifecycle contract, native drag semantics, OS-level window creation / message semantics
- Cross-platform interpretation:
  - not a Windows visual-only bug
  - a helper-window abstraction gap, with Windows as the first strong failure surface

## Related Issues

- #153 main issue anchor
- #139 helper-window lifecycle precedent on Capsule
- #158 governance issue for helper-window / native-window contract

## Impact

- QA feature family is still missing one core helper-window behavior on Windows
- If left vague, future work will keep wasting time on toolbar-level workarounds instead of fixing the OS contract
- This blocks a clean "QA helper-window semantics aligned" conclusion

## Proposed Acceptance Criteria

- [ ] Decide whether Windows QA panel must fully match the macOS draggable contract
- [ ] If yes, implement the fix at native window creation / message layer
- [ ] Keep a real verification that proves both:
  - `Ctrl+Shift+;` works
  - QA panel is draggable
- [ ] Keep this issue out of the main window radius / frame appearance family

## Parked Note

Current recommendation: keep draft and park the repair here until the Windows native window/message strategy is explicitly chosen.
