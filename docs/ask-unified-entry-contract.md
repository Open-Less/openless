# Ask Anything unified entry: desktop contract

This contract covers the macOS and Windows Ask window, shortcuts, native targets, submission and delivery. See the [attachment, context and tool contract](ask-standalone-contract.md) for those shared boundaries.

## Open and submit

The current recording shortcut plus Space opens or focuses the compact `ask-anything` composer. The same window expands around the composer after an explicit submission resolves to `answer` or `selectionAnswer`. `write`, `edit` and `clarify` stay compact. Expansion requires the matching acknowledged turn and current visibility generation. Replay can reconcile a live submission but cannot expand a dismissed window. Escape dismisses the composer and cancels pending voice input. An accepted turn retains its own cancellation and result lifecycle.

The recording chord can start voice input after the current composer acknowledges focus. `autoVoiceOnOpen` defaults to `true` and does not apply to opening from Settings. Dictation into Ask only fills the current draft. It does not send automatically. The recording ID, conversation, editor revision and focus generation prevent delayed transcription from editing a different draft. Repeated chords during active recording focus the composer. Each recording controls only its own capsule and audio lifecycle.

| Route | Result |
| --- | --- |
| `answer` | Answers the question in chat, independent of any incidental selection. |
| `selectionAnswer` | Answers about the relevant verified selection. An explicit reference without a verified selection requests clarification. |
| `write` | Generates new text for a verified collapsed external caret. |
| `edit` | Replaces the verified original selection. A missing selection requests clarification. |
| `clarify` | Requests input in the composer, with no external write. |

Explicit send prepares the native target and freezes the chosen mode, channel and validated attachments after admission. An identical request ID and payload reuse the original turn. A different payload with the same ID conflicts. Busy work is rejected before another target read. Once chat has expanded, later turns remain in that conversation and use the answer path. The backend reviews output before publishing or delivering it.

## Settings and routing

Machine-local `ask-entry-settings.json` stores `{ autoVoiceOnOpen, intentMode }`. A new store and explicit recovery use `{true, separateModel}`. Saved valid selections persist. Turning off the active Beta routing option returns to `semanticHybrid`. The native main window uses `ask_get_entry_settings` and `ask_set_entry_settings`, which return the committed settings or a typed failure.

- `semanticHybrid` uses local intent signals and asks the main model for a small decision when uncertain.
- `separateModel` makes an isolated classifier request that produces no transcript output.
- `mainModelTool` requires the main model to call the local `resolve_intent` tool and continues that logical conversation with the validated decision.

Classifier input is limited to the user's instruction and minimal selection or reference signals. Full selection, attachments and conversation history are turn data only where authorized. Invalid or unsupported decisions surface an error and preserve the selected strategy. Web search uses a validated public query. Its indicator follows actual turn-scoped tool activity.

## Native target and delivery

On macOS, inserting new text binds to the original eligible caret recorded when Ask opened. Reading or replacing a selection binds to the verified target captured at send. Windows uses its current external target capture. Native capture distinguishes no selection, a verified selection and unavailable evidence. AppKit or UI Automation handles, document fingerprints and write leases stay in the host. Unknown or protected controls are unavailable. Read-only selection may be usable as context and cannot authorize a write.

Before delivery, the host revalidates the original editor and expected caret or UTF-16 range. A one-shot claim immediately precedes a possible write. Outcomes distinguish a confirmed write, retained result and uncertain write. Never replay an uncertain native write or replace a different control. The full retained result remains copyable in Ask. On macOS, native write failures leave it for manual copy. The Windows path has a separate once-per-turn clipboard fallback. Cancellation before a claim prevents its effect. An already issued write is reported by observation, with no rollback claim.

## IPC and verification

`ask-window/client.ts` and the native `ask_window` command and event adapter define the wire. Events and replay are ordered per conversation. The non-chat projection exposes the accepted turn, authoritative route, delivery status and optional retained text. Conversation, turn and attachment identities gate late updates. No external write occurs for answer or clarification routes.

Check the Core routing, tool and replay tests in `qa_service/standalone/`, the browser Ask specs, and the platform-specific [native target tests](../openless-all/app/src-tauri/src/ask_window/target/README.md). Installed macOS and Windows acceptance separately covers the shortcut, composer and capsule focus, changing selection, selection answer, caret insert, selection replacement, and a changed editor during generation.
