# Native OpenAI Realtime dictation

See [Native Realtime setup and protocol compatibility](native-realtime.md) for
the GUI controls, gateway authentication, Qwen profiles and tested boundaries.

Choose **Multimodal → OpenAI Realtime**, enter an OpenAI API key and select a
Realtime model. The default API root is `https://api.openai.com/v1`. This provider
uses the Realtime WebSocket protocol; the existing **OpenAI** provider continues
to use HTTP Chat Completions. Selecting a Realtime model in the HTTP provider is
not equivalent to selecting this provider.

## Recording and output

OpenLess queues PCM immediately, opens the session in the background and sends
audio while recording. Stopping recording flushes the final sample, commits the
audio and requests one text response. Server turn detection is disabled. There
is no generated speech and no separate ASR service or second polishing request.
The default **Auto** reasoning setting uses `low` for recognized 2.x models and
omits the parameter for older models. The Realtime-specific effort dropdown is
independent of the text-LLM thinking toggle. **Omit** lets the server choose;
**none** is experimental and can fall back once to `low` only when the initial
session setup explicitly rejects that effort value. Authentication, quota and
network failures do not trigger this fallback.

The existing style, vocabulary, working languages, Chinese-script preference,
cursor context and explicit translation mode form the prompt. Cursor capture is
the normal host capture and respects its existing switch and failure behavior;
this provider does not inspect windows itself. When conversation context is
enabled, at most two already-filtered final texts are included, each bounded to
2,000 Unicode characters. Context is reference data, not content to repeat.

Short confirmations and fragments are kept concise. Questions and action
requests spoken in ordinary dictation are transcribed rather than answered.
Long dictation retains details and corrections instead of being summarized.
The final result follows the existing preview, script conversion, insertion and
history pipeline. Auxiliary text operations also use this transport. Current
multimodal history entries do not expose audio retranscription; retranscribing
older traditional entries still uses the separately selected ASR channel. Keep
that channel usable if removing an old ASR bridge. Complete-file PCM playback
through the Realtime engine includes connection setup time.

The dictation prompt allows clear, committed self-corrections only to content
already identified within the current recording. Consecutive corrections keep
the final value and preserve the other facts; merely quoted, reported, compared,
rejected or undecided edits remain dictated content. This does not edit cursor
documents or historical turns. If an earlier-state label is retained, it stays
paired with its old value, not the replacement; the selected style and explicit
translation then apply to the revised content.

## Boundaries

- The queue accepts up to ten minutes of PCM16 mono 16 kHz audio and converts it
  to 24 kHz without resetting resampling at recorder chunk boundaries.
- Connection setup has a 15-second deadline. Final generation has a 30-second
  deadline and a 4,096-token output limit. Incomplete responses are errors,
  rather than successful shortened dictations.
- Cancellation, dropped sessions and application shutdown close the session.
  No helper process, local listener or separate startup entry is required.
- Credentials remain in the existing secure Omni credential store. Credentials,
  recording bytes and document context are not included in transport errors or
  logs. The normal application history and audio-retention preferences apply.
- The **Validate** action checks the WebSocket, model access and session schema.
  It does not record, send audio or create a response, so it cannot establish
  recognition quality. Actual usage and credit eligibility depend on the API
  organization, project and model, just as for other direct OpenAI API clients.

An existing external bridge is not needed for this provider. Providers that
depend on unofficial web endpoints are separate integrations and are unaffected.
