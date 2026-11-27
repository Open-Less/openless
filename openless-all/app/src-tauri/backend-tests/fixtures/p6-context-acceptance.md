# P6 context and edit observation acceptance

Run against one exact, reviewed P1→P6 Dev SHA built by the cloud runner. No local
build, Cargo invocation, real-account content or model prompt logging is needed.
The candidate branch is not a release or a completed compatibility claim.

## Controlled editor

Use a disposable plain text field with this complete value:

```text
P6_BEFORE P6_SELECTED P6_AFTER
```

Select exactly `P6_SELECTED`. With Accessibility permission and cursor-context
enabled, the prebuilt, ignored Tauri test
`real_host_document_capture_uses_the_frozen_original_range_and_live_opt_in`
can be explicitly run by the coordinator with `OPENLESS_P6_CONTROLLED_EDITOR=1`.
It uses the production Tauri HostContext adapter and native P4 owner; it does
not insert text or contact a model. Assertions never print the actual field.
Without the controlled fixture, leave it ignored.

The three fields must be `before = "P6_BEFORE "`, `selection = "P6_SELECTED"`,
`after = " P6_AFTER"`. In a long selection repeat test Han text on both
sides of a unique middle token, retaining leading/trailing whitespace; verify
the #8 selection-edit request/history path separately. The location label must
not receive any of these strings, the page URL or native identity tokens.

Repeat with cursor-context off, Accessibility unavailable, a protected input,
another input focused before capture, and a cancelled recording. No native
document data should be returned; ordinary dictation and chosen style remain
available. Do not enter credentials or other private content to test this.

## Same name and rewrite

1. Dictate a test sentence whose ASR output is `请使用 Codox 完成这个任务`.
   Correct only `Codox` to `Codex` inside that insertion and leave the caret there.
   Pause at least five seconds. A candidate may appear only if the native range
   and all required observer subscriptions were established.
2. Before confirming, verify no new dictionary entry exists. Use the existing
   confirmation card; after confirming the complete `Codex` word should appear
   once. Do not create a source-to-target replacement rule.
3. Begin another recording after confirmation and say the same name. Inspect
   the exact request's hotword data without recording the surrounding field,
   then record the actual ASR/model output as test evidence. A hotword
   appearing in a request does not prove the model recognized it correctly.
4. Replace the whole inserted sentence with a different test paragraph.
   It must not create a word candidate or an expression sample.
5. Edit an identical word elsewhere outside the inserted range. Switch to
   another field or window, then return; also move the caret outside the known
   insertion and return. The old observation must end without a late candidate.
6. Cancel/start another recording while the first watcher is queued. Old
   callbacks must not read a new input or post a candidate into the new session.

## Expression and modes

- Repeated punctuation/list/closed courtesy and concise-form edits may produce
  abstract evidence only. One session contributes once; fewer than 10 sessions,
  a span under three days, or inconsistent directions must not affect prompts.
- Confirmed names are used by the next ASR/postprocessing snapshot. Expression
  directions are postprocessing only. Neither snapshot changes a recording that
  already started.
- Repeat ordinary style, explicit style, canonical Raw and Shift translation
  on/off. Check actual mode, request, output and history; ensure reference text
  is not repeated into the result. This is #4 scoping, not #15 completion logic.
- Keep the existing confirmation IPC/UI. The frontend owner verifies card
  feedback, long names, translations, font scaling and focus preservation.

## Evidence limits and platform fallback

Record exact SHA, editor/version, permission state, input, actual output and
whether notifications really fired. Registration success and synthetic range
tests do not prove native event delivery or real model quality. Unsupported AX
range APIs, missing required notifications, transformed paste text, stale
targets and failed delivery proof must fall back to ordinary dictation.

This candidate does not create an alternative Windows observer: automatic
desktop learning stays off there until an owned UIA/native insertion-range
receipt is available. Ordinary Windows insertion, manual vocabulary APIs and
other legacy hosts keep their existing contracts. Browser input-kind labels
never authorize page/URL reads, and these steps do not exercise P5's page reader.

Plain AX value/selection notifications do not identify the edit span. A changed
field length does not resize the insertion sample: do not read or learn that
intermediate state. Equal-length corrections at the original fixed position
remain eligible. Delete-then-type can resume only if it returns to the same
proven length/range; net insertions/deletions require a real native range owner,
which this AX path does not currently have. In particular, append once far
after 64 repeated boundary newlines while leaving the original caret unchanged:
there must be no expanded sample, vocabulary candidate or expression evidence.
