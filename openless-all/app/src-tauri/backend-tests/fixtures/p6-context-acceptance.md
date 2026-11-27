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

## Current desktop safety and explicit vocabulary

1. Dictate a test sentence whose ASR output is `请使用 Codox 完成这个任务`.
   Correct only `Codox` to `Codex` inside that insertion. Ordinary AX and Windows
   have no integrated trusted edit-span/revision source: there must be NO
   automatic body read, word candidate or expression evidence, including after
   a quiet pause or an equal-length correction. This is a safety fallback,
   not acceptance of automatic learning.
2. Add `Codex` through the existing explicit manual vocabulary action. Existing
   pending-candidate accept/reject APIs also remain intact; they must not write
   before explicit acceptance. The trusted-owner contract regression exercises
   this confirmation boundary, but cannot produce a native candidate in Dev.
3. Begin another recording after explicit addition/confirmation and say the same name. Inspect
   the exact request's hotword data without recording the surrounding field,
   then record the actual ASR/model output as test evidence. A hotword
   appearing in a request does not prove the model recognized it correctly.
4. Replace the whole inserted sentence with a different test paragraph.
   It must not create a word candidate or an expression sample.
5. Edit an identical word elsewhere outside the inserted range. Switch to
   another field or window, then return; also move the caret outside the known
   insertion and return. No automatic watcher may be armed or post a candidate.
6. Cancel/start parallel recordings. No old receipt may authorize reading a
   new input or post a candidate into another session.

## Expression and modes

- A future trusted owner can submit punctuation/list/closed courtesy and
  concise-form edits as abstract evidence only. This contract is tested with
  synthetic callbacks; current AX/Windows edits must produce NO evidence.
  One session contributes once; fewer than 10 sessions,
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

Record exact SHA, editor/version, permission state, input and actual output.
Synthetic range tests do not prove native editor support or real model quality.
Startup context is separate from post-insertion observation; verifying the
former does not establish any tracked-range capability for the latter.

This candidate does not create an alternative Windows observer: automatic
desktop learning stays off there until an owned UIA/native insertion-range
receipt and continuous edit revisions are available. Ordinary Windows insertion, manual vocabulary APIs and
other legacy hosts keep their existing contracts. Browser input-kind labels
never authorize page/URL reads, and these steps do not exercise P5's page reader.

Plain AX value/selection notifications do not identify an edit span or revision,
so fixed positions, unchanged totals, repeated anchors and caret positions never
authorize subsequent body reads. Both split notifications that restore total
length and one coalesced equal-length batch must be rejected. Reproduce with
64 `！`, `今天先完成这个工作然后再讨论！`, 64 `！`, and `TAIL`: insert `！`
before the field, delete the last `L`, keeping the caret numerically in the old
range. No borrowed punctuation may become expression evidence. Repeat the
far-append case after 64 boundary newlines; there must be no sample or learning.

The Core trusted-owner positive fixture proves only its contract: same
session/owner/insertion, every exact local edit span, revision-scoped selection
and bounded body read. No production desktop adapter currently supplies it.
Do not count disabled observation as completion of #4.
