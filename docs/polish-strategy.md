# ASR polish strategy

OpenLess can optionally run a local strategy after ASR has finished and correction rules have been applied, but before the polish LLM request is sent. Enable **Use custom polish strategy** in Settings → Privacy → Data storage, then enter a Rhai function named `strategy`.

The feature is disabled by default. When it is disabled, empty, or fails, OpenLess keeps the normal polish and short-transcript rules. A strategy failure never prevents dictation.

## Input

The function receives one map. `charCount` counts non-whitespace Unicode scalar values, using the same rule as the built-in short-input setting. `audioDurationMs` is the final audio duration in milliseconds.

```rhai
text            // corrected ASR text
charCount       // integer, non-whitespace Rust char count
audioDurationMs   // integer, milliseconds
language        // string, or () when unavailable
confidence      // floating-point value, or () when unavailable
asrProvider     // provider label, or an inferred provider type
asrModel        // string, or () when unavailable
```

The function must return a map with an `action` field:

```rhai
// Do not send an LLM request.
#{ action: "skip" }

// Keep the normal polish request and configured parameters.
#{ action: "polish" }

// Polish, applying only the listed per-request overrides.
#{ action: "request", temperature: 0.4, maxTokens: 400, reasoningEffort: "low" }
```

A typical short-input policy is:

```rhai
fn strategy(input) {
    if input["charCount"] <= 3 {
        #{ action: "skip" }
    } else {
        #{ action: "polish" }
    }
}
```

The reasoning example below keeps longer English dictations cheaper while preserving ordinary polish for other languages:

```rhai
fn strategy(input) {
    if input["language"] == "en" && input["charCount"] > 160 {
        #{ action: "request", reasoningEffort: "low" }
    } else {
        #{ action: "polish" }
    }
}
```

## Request mapping

Only `temperature` (`0..=2`), `maxTokens`, and `reasoningEffort` (`none`, `low`, `medium`, or `high`) are accepted. They apply to this request only; they do not rewrite channel credentials or session preferences.

- Chat Completions uses `temperature`, `max_tokens`, and provider-specific reasoning controls; official OpenAI reasoning models use `max_completion_tokens` for a `maxTokens` override.
- Responses uses `max_output_tokens` and the provider's `reasoning.effort` mapping. Official OpenAI reasoning models also omit `temperature`, including when a `none` override removes the reasoning object.
- Messages uses `max_tokens`; its existing thinking mode and budget validation remain authoritative. If a `maxTokens` override would be no larger than a configured budget-thinking request, it is omitted to avoid an invalid request.
- Gemini maps `temperature` to `generationConfig.temperature` and `maxTokens` to `generationConfig.maxOutputTokens`; `none` disables thinking and other effort values enable it. Gemini does not expose four distinct effort levels. Providers that expose only a boolean thinking switch likewise treat `low`/`medium`/`high` as enabled and cannot preserve the exact level.
- Codex uses Responses `max_output_tokens` and maps `reasoningEffort` to Responses `reasoning.effort`.
- Existing model capability rules still win. For example, official OpenAI chat models that do not support `reasoning_effort` receive no such field, and `gpt-5-pro` remains at its required `high` tier. Unsupported overrides are omitted or reduced to the configured provider capability, not retried as a second request.

Other returned keys, malformed values, out-of-range numbers, and actions with incompatible overrides are rejected and the normal policy is used.

## Limits and privacy

The evaluator is embedded in Core and has no file, network, process, package, or credential APIs. The source is limited to 32 KiB, transcript input to 128 KiB, and execution is capped by operations, call depth, string size, and collection size. The script receives transcript metadata but never receives API keys, prompts, credentials, or document context.

The script and enable switch are portable encrypted preferences. If cloud sync is enabled, another OpenLess device may receive and execute the same user-authored script, still under these limits. Disable the feature before syncing to a device where the policy is not desired.

Missing provider metadata is represented by `()`. A script that depends on optional metadata should check for that value and return `polish` when it is unavailable.
