## Summary

Closes #153

This draft PR is now a tracking anchor for a narrowed conclusion, not for continued patch stacking.

Current convergence:

- macOS product intent is not "just a floating panel"
- it is a **non-activating but draggable helper-window**
- Windows currently only matches the non-activating part of that intent
- Windows does **not** yet carry the draggable semantics

Therefore the next real fix should move to native window creation / message strategy, not stay in React toolbar or generic drag API workarounds.

## Current Status

- keep draft
- park here
- do not continue hard-fixing in this PR until the Windows native helper-window strategy is chosen

## Scope

- helper-window lifecycle semantics
- helper-window drag semantics
- selection ask / QA feature family

Out of scope:

- main window frame / radius / shadow issues
- generic Windows UI appearance fixes
- large QA renderer refactors

## Key Finding

```text
Windows QA panel lacks a native draggable contract equivalent to
the original macOS helper-window behavior.
```

## Evidence

- Windows local result:
  - `Ctrl+Shift+;` works
  - QA flow works
  - QA panel still does not drag
- runtime tracing shows drag APIs still reduce to ordinary caption-drag semantics
- this is enough to stop guessing and treat the remaining problem as a native helper-window semantics gap

## Next Repair Direction

Future repair should move toward:

- native window creation attributes
- native message / hit-test contract
- helper-window specific drag-region semantics

## Validation Plan

- [x] Manual verification: QA hotkey path remains healthy
- [x] Manual verification: panel drag still fails on Windows
- [ ] Future verification after native strategy:
  - QA panel opens
  - QA panel drags
  - helper-window remains non-disruptive
