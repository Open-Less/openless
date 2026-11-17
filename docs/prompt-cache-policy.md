# Provider prompt cache contract

The client optimizes reusable input prefixes. It does not cache model outputs or skip model calls. Cache availability, minimum lengths, expiry and routing remain properties of the selected service and model. A token-weighted hit rate is not a request success rate; unique input and cold requests remain in the denominator when reported.

## Request boundaries

Fixed style, translation and question-answering instructions precede changing application and cursor context. Existing context escaping, text envelopes, trailing injection defenses and output constraints remain intact. Custom style templates and vocabulary order are not rewritten. Ordinary dictation does not acquire unrelated conversation history to inflate the shared prefix.

Provider-specific parameters are selected from the actual HTTPS origin, API path, request format and known model capabilities, rather than the channel's display name or template. Unknown gateways retain the shared prefix improvement without unverified vendor parameters. User-supplied routing headers and cache settings take precedence.

Routing keys are bounded opaque hashes of service, model and reusable system prefix. They contain no credentials, raw prompt, generated application context or per-request UUID. Cache breakpoints divide an existing system string into contiguous slices: concatenating the slices must reproduce the original text exactly. Only the static slice receives a system cache marker. For growing Anthropic conversations, a second marker may cover the last completed assistant turn; the current user input is not marked. Multi-turn OpenAI requests retain implicit history caching alongside the static explicit breakpoint.

## Service strategies

| Service and format | Strategy |
| --- | --- |
| DeepSeek official APIs | Stable prefixes; provider-managed implicit caching. No OpenAI cache parameters are forwarded. |
| OpenAI Chat Completions / Responses | Stable routing key for related prefixes. Known GPT-5.6 and GPT-6 Responses models additionally mark static developer input with an explicit breakpoint. Older models retain implicit caching. |
| Anthropic Messages | Mark the stable system content with the default ephemeral cache policy for Claude models. |
| OpenRouter Chat Completions / Messages | Stable `session_id` for cache-aware routing; explicit stable-system markers for supported Anthropic models. Existing route selection is preserved. |
| Ark Chat / Responses | Stable prefixes for all models. Only documented Seed 2 series receive `X-Prompt-Cache-Id`; DeepSeek models do not receive this Seed-specific option. |
| DashScope | Stable prefixes; explicit markers only for verified standard API/model combinations. Coding-plan and unsupported model endpoints retain implicit behavior. |
| Native Gemini, Codex and other gateways | Shared stable prompt composition. No generic endpoint's cache flags are assumed compatible with their transport. |

The policy does not create separate remote cache objects, extend retention, enable response storage, send paid warm-up requests or change provider/model selection. An explicit marker can incur a provider's normal cache-write charge; its benefit depends on reuse before expiry.

## Accounting and validation

Usage records retain the service and protocol snapshot and the raw returned quantities. Canonical Chat Completions, Responses and Gemini input totals include cached input; Messages input is additive with cache reads and writes. Ambiguous or missing fields retain unknown semantics. Old records are not retroactively assigned the current service or a guessed denominator.

The displayed rate is `sum(cache reads) / sum(comparable total input)` for the same reported records. The UI exposes the number of comparable reports and groups by service, channel, model and protocol. This remains a partial ledger: native Gemini/Codex recording, every request attempt and unreported usage require separate work. Neither a missing report nor a short uncached request proves a client failure.

Acceptance checks cover origin/model/format boundaries, exact text preservation, stable keys across application changes, caller overrides, inclusive/additive usage, unknown values, overflow and streaming report merging. Service-side improvement must be measured from subsequent real responses; local request tests cannot establish a universal 97–98% cache rate.

## Official references

Verified on 2026-11-09; capability tables must be checked when adding a model or service:

- [DeepSeek context caching](https://api-docs.deepseek.com/guides/kv_cache/)
- [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching)
- [Anthropic prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
- [Ark implicit and explicit caching](https://docs.volcengine.com/docs/ark/context-cache?lang=zh)
- [Ark routing key and supported models](https://docs.volcengine.com/docs/ark/prompt-cache-key?lang=zh)
- [DashScope context caching](https://help.aliyun.com/zh/model-studio/context-cache)
- [OpenRouter cache-aware routing](https://openrouter.ai/blog/tutorials/prompt-caching-sticky-routing/)
- [Gemini context caching](https://ai.google.dev/gemini-api/docs/caching)
