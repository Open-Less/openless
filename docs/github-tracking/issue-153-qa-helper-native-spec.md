## Issue #153 Native Spec

Scope: Windows-native helper-window implementation for QA panel

Status:

- This document is the implementation-spec layer for `#153`
- It replaces "keep trying shared drag carriers" with a platform-layered plan
- Product goal stays shared with macOS; implementation becomes Windows-native

### Shared Product Contract

The QA panel must satisfy the same product goal across platforms:

1. Non-disruptive to the source app context
2. Clickable controls:
   - close
   - pin
3. Draggable from toolbar/background drag affordance
4. Safe multi-turn follow-up usage
5. Dismiss means non-participating, not merely visually hidden

### Windows Native Contract

Windows should not reuse macOS's implementation shape as the primary carrier.

Instead, the Windows-specific helper-window must explicitly define:

1. Window creation attributes
   - topmost helper window
   - transparent window is allowed only if it does not break hit-test semantics
   - no taskbar entry
   - activation behavior must be explicitly chosen, not inherited accidentally

2. Native drag semantics
   - drag region must be recognized at the native window/message layer
   - do not depend on shared async drag helpers as the primary mechanism
   - toolbar drag and control-hit regions must be natively separated

3. Native click semantics
   - close and pin controls must remain clickable
   - drag region must not swallow control clicks
   - hit-test mapping must distinguish:
     - drag region
     - control buttons
     - normal client area

4. Source-app context relation
   - normal show path should avoid stealing the user's upstream context unnecessarily
   - if Windows drag requires temporary focus/activation semantics, this must be treated as an explicit transition, not an accident
   - after drag/dismiss, helper-window semantics must be restored

### Current Baseline Findings

Upstream-based repro branch confirms:

- QA feature exists
- hotkey path can be made healthy
- close path can be healthy
- drag is the remaining isolated interaction gap

This means the implementation target is now narrow:

```text
Fix Windows-native draggable semantics for QA helper-window
without reopening unrelated QA / UI / hotkey scope.
```

### Implementation Boundaries

In scope:

- Windows QA helper-window creation/runtime behavior
- native hit-test / message routing for drag region
- preserving clickable controls while enabling drag

Out of scope:

- main window frame/radius/shadow issues
- Capsule geometry / lifecycle work from other families
- unrelated provider / insertion / polish changes

### Acceptance Criteria

- [ ] `Ctrl+Shift+;` still opens and closes QA panel
- [ ] close button remains clickable
- [ ] pin remains clickable
- [ ] toolbar drag works on Windows
- [ ] no regression in follow-up QA flow
- [ ] implementation remains scoped to Windows-native helper-window semantics only

### Notes

The key design rule is:

```text
Same product goal, different OS carriers.
Windows must own its native helper-window carrier instead of inheriting
macOS-shaped interaction assumptions.
```
