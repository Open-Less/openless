# Realtime voice typing

OpenLess streams microphone audio to a Realtime model and inserts the resulting
text using your existing style, vocabulary and cursor context.

## Setup

Open **Settings → AI services and models → Multimodal**. Choose a provider,
enter the model, endpoint and credentials, then click **Validate**.

| Provider | Models | PCM audio |
| --- | --- | --- |
| `openai-realtime` | `gpt-realtime-2.1`, `gpt-realtime-2.1-mini` | 24 kHz, mono, 16-bit |
| `dashscope-realtime` | `qwen3.5-omni-flash-realtime`, `qwen3.5-omni-plus-realtime`, `qwen3.8-omni-flash-realtime` | 16 kHz, mono, 16-bit |
| `dashscope-realtime-legacy` | `qwen3-omni-flash-realtime-2025-09-15` | 16 kHz, mono, 16-bit |

Select the protocol your service implements. Gateways can use custom model names.

## Endpoint and authentication

| Service | Endpoint |
| --- | --- |
| OpenAI | `https://api.openai.com/v1` |
| Qwen 3 / 3.5 | `https://dashscope.aliyuncs.com` or the regional shared host |
| Qwen 3.8 | Your workspace host, e.g. `https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com` |
| Gateway | Its HTTP(S) API root or complete WS(S) endpoint |

OpenAI API roots append `/realtime`; Qwen's official hosts use
`/api-ws/v1/realtime`. Gateway path prefixes and routing queries are preserved.
For a custom WebSocket route, enter its complete WS(S) URL.
Qwen 3.8 needs a region-matched workspace host; see
[Qwen endpoint setup](https://help.aliyun.com/zh/model-studio/omni-realtime-websocket-access).

Authentication options:

- **Bearer**: sends the API key as `Authorization: Bearer …`.
- **api-key**: sends the API key in the `api-key` header.
- **Custom headers only**: enter a JSON object in Extra headers; the API-key field is optional.

Put credentials in headers. Bearer and api-key modes manage their authentication
headers; the client manages WebSocket framing headers.

## OpenAI reasoning

| Setting | Behavior |
| --- | --- |
| `auto` (default) | Uses `low` for recognized `gpt-realtime-2.x` models; omits reasoning for other names |
| `omit` | Uses the server's default |
| `minimal`, `low`, `medium`, `high`, `xhigh` | Requests that effort level |
| `none` (experimental) | Requests no reasoning; availability depends on the endpoint |

Start with **gpt-realtime-2.1 / Auto**. Legacy OpenAI models omit reasoning
parameters. For custom aliases, check the gateway's supported settings.

The optional `none → low` fallback retries setup once if the server rejects
`reasoning.effort=none`, before audio upload. Other errors are reported normally.
The [public effort schema](https://developers.openai.com/api/reference/resources/realtime/client-events)
does not currently list `none`.

## Dictation and editing

Audio buffers while the connection opens, then streams during recording.
Stopping sends the remaining audio and requests the final text.
Styles, vocabulary, output language, Chinese-script preference, preview and
insertion use the existing OpenLess workflow. Enabled conversation context includes up
to two recent final texts, capped at 2,000 characters each.

Questions spoken during ordinary dictation stay as text. Clear self-corrections
revise the current recording. The app's translation command selects translation.

| Action | OpenAI Realtime | Qwen Realtime |
| --- | --- | --- |
| Dictation / selection voice recognition | Realtime model | Realtime model |
| Selection polish, voice editing, text Q&A | Realtime model | Configured text LLM |
| Audio-attachment Q&A | Supported | Choose another provider |

Keep a text LLM configured for Qwen editing and Q&A. These actions use the saved
LLM channel and model. Ordinary Qwen dictation works independently.

Old traditional history entries and applicable quick notes use the selected
**ASR channel** for Retranscribe. Configure that channel separately if you retire
an external bridge.

**Validate** checks the real connection and session settings. Test a short
recording afterward to check recognition and insertion.

## Protocol details

OpenAI GA uses nested audio settings, `output_modalities: ["text"]` and
`response.create` with GA options. Current Qwen uses nested 16 kHz audio settings,
`modalities: ["text"]` and a bare `response.create`; legacy Qwen uses the flat
`input_audio_format: "pcm"` field. Each profile disables server VAD and generates text.

References: [OpenAI GA events](https://developers.openai.com/api/reference/resources/realtime/client-events),
[Qwen events](https://help.aliyun.com/zh/model-studio/client-events),
[DashScope SDK](https://github.com/dashscope/dashscope-sdk-python/blob/2cb05b1ec87bdfa104ce3732f6b65db685469e2a/dashscope/resources/audio/realtime.py#L392).

Gemini Live, xAI Voice Agent, Doubao dialogue and StepAudio need their own
adapters. Existing HTTP Omni and WebSocket ASR channels remain available.

## OpenAI API credits

Eligible ChatGPT Pro subscribers can activate subscription API credits for an
owned API organization. Realtime usage is covered. Use that organization's
project API key and check its credit balance.
[Eligibility and activation](https://developers.openai.com/api/docs/guides/chatgpt-api-credits).
