# Issue #153 Tracking

Scope: QA helper-window lifecycle consistency against macOS intent

Current stage:

- This branch is a draft PR placeholder.
- No runtime fix is included yet.
- The goal is to converge the QA panel lifecycle contract before repair work starts.

Problem statement:

- macOS explicitly implements a non-activating helper-window show path.
- Windows/Linux currently rely much more on Tauri window config and plain `show()` / `hide()`.
- Dismiss semantics are not yet proven to mean fully non-participating helper-window end.

Implementation target to converge before coding:

- Define `show`, `dismiss`, `hidden`, and `non-participating` semantics for QA panel.
- Separate reasonable platform differences from contract violations.
- Reuse or align with the Capsule helper-window contract where possible.

Non-goals in this draft:

- No QA renderer rewrite
- No selection / ASR / LLM flow change
- No speculative Linux parity claim without evidence
