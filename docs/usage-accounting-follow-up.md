# Service usage accounting follow-up

The desktop dashboard reads retained dictation history and the bounded local service ledger independently. History can be cleared or capped without removing the ledger. Provider labels in history do not represent retries or billing.

The background writer accepts explicitly reported token data from supported text-polisher routes and standalone Ask model calls. Ask records distinguish the response or composition call from intent decisions, tool planning and output review. The dashboard shows only saved reports. Native Gemini, Codex, SIWC, multimodal, ASR, TTS, and unsuccessful attempts still require route-specific coverage. The writer's error count is process-local. Historical token quantities cannot be reconstructed from history.

Known official Chat Completions services request streaming usage when their endpoint/model supports it and an observer is attached. An interrupted stream may not return its final usage report. Unknown gateways receive no unverified usage option. See the [provider cache contract](prompt-cache-policy.md) and its official references.

New reports retain a service/protocol snapshot and explicit inclusive or additive cache accounting derived from canonical response fields. The cache dashboard groups comparable reports by service, channel, model and protocol; it shows the count of comparable saved reports, not coverage of all actual attempts. Legacy and ambiguous reports keep unknown cache semantics. Existing raw quantities are preserved, and no current endpoint or model configuration is assigned retroactively.

Remaining work is to extend the existing ledger at the service-call boundary. Cover actual attempts across streaming, non-streaming, retries, cancellation, errors and native protocols. Record reported quantities where available, including speech-time or TTS units, and keep missing legacy data distinct from a measured zero. Preserve bounded retention, explicit deletion and the exclusion of content and credentials. Durable loss reporting, shutdown draining and authoritative server-side billing are separate from the current dashboard.
