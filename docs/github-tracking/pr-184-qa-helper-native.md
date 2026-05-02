## Summary

Closes #153

This code PR is the upstream-based delivery branch for the narrowed Windows repair:

```text
Windows native drag semantics for QA helper-window
```

This PR follows a layered strategy:

- keep the same product goal as macOS
- stop assuming the same implementation carrier works on Windows
- move Windows behavior toward a native helper-window contract

## Scope

In scope:

- Windows-native QA helper-window interaction semantics
- drag-region / click-region separation
- preserving non-disruptive QA workflow

Out of scope:

- main window appearance
- Capsule family work
- unrelated QA renderer redesign

## Shared Product Goal

The PR keeps the same product target:

- non-disruptive helper window
- clickable close / pin
- draggable toolbar region
- follow-up QA flow stays healthy
- dismiss returns to non-participating state

## Windows-native Direction

This PR is not meant to keep stacking shared drag workarounds.

It exists to drive the implementation toward:

- Windows-specific helper-window carrier
- native hit-test / message ownership where needed
- explicit separation of drag surface and control surface

## Validation Target

- [x] upstream-based branch exists and builds
- [x] narrowed baseline established: hotkey + close can be healthy while drag remains broken
- [ ] Windows drag becomes healthy on this branch
- [ ] reviewer-side Windows regression confirms:
  - open
  - close
  - drag
  - follow-up QA

## Related Anchors

- #156 remains the tracking / design-convergence PR
- #158 remains the governance anchor for helper-window / native-window contract thinking
