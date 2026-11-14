# Native Realtime dictation

OpenLess streams microphone audio to a generative Realtime model and receives
text using the existing dictation style and context. No helper process is needed.
A WebSocket URL alone does not establish protocol compatibility.

## Choose a protocol in settings

In **Settings → AI services and models**, choose the **Multimodal** pipeline and
select the matching provider. Enter its endpoint, model and authentication,
then use **Validate** before trying a real recording.

| Provider ID | Protocol and candidate models | Audio sent |
| --- | --- | --- |
| `openai-realtime` | OpenAI Realtime GA; `gpt-realtime-2.1`, `gpt-realtime-2.1-mini`, or an explicitly compatible gateway alias | PCM16 mono, 24 kHz |
| `dashscope-realtime` | Current Qwen Omni Realtime; `qwen3.5-omni-flash-realtime`, `qwen3.5-omni-plus-realtime`, `qwen3.8-omni-flash-realtime` | PCM16 mono, 16 kHz |
| `dashscope-realtime-legacy` | Legacy Qwen Omni; `qwen3-omni-flash-realtime-2025-09-15`, using its flat audio-format fields | PCM16 mono, 16 kHz |

The profile is explicit, not inferred from a model name. Existing HTTP
**OpenAI**, **Qwen Omni** and **Custom** providers retain their HTTP behavior.
OpenAI's old beta interface has been [retired](https://developers.openai.com/api/docs/deprecations).

The [OpenAI GA reference](https://developers.openai.com/api/reference/resources/realtime/client-events)
uses `output_modalities: ["text"]` and nested `audio.input.turn_detection`.
[Qwen Omni](https://help.aliyun.com/zh/model-studio/client-events) uses
`modalities: ["text"]` and top-level `turn_detection: null`. Current Qwen uses
nested `audio.input.format` with `sample_rate: 16000`; legacy Qwen uses
`input_audio_format: "pcm"`. Neither Qwen profile sends OpenAI reasoning fields.
This distinction also follows the
[official DashScope SDK serializer](https://github.com/dashscope/dashscope-sdk-python/blob/2cb05b1ec87bdfa104ce3732f6b65db685469e2a/dashscope/resources/audio/realtime.py#L392).

## Set the endpoint and authentication

Use an HTTP(S) API root, or a WS(S) URL with the complete WebSocket path:

| Input | Resulting route |
| --- | --- |
| OpenAI: `https://api.openai.com/v1` | `/v1/realtime?model=…` |
| Qwen: `https://dashscope.aliyuncs.com` | `/api-ws/v1/realtime?model=…` |
| HTTP gateway API root: `https://gateway.example/team/v1` | `/team/v1/realtime?model=…` |
| Complete WS gateway endpoint: `wss://gateway.example/team/socket` | `/team/socket?model=…` |

Custom gateway prefixes are retained. A non-root WS(S) path is complete, without
another `/realtime` suffix. Qwen maps
HTTP `/v1` and `/compatible-mode/v1` to its Realtime route only on official
DashScope/workspace hosts. For a nonstandard gateway route, provide its complete
WS(S) URL. Existing routing queries survive; the model is encoded once, URL
userinfo is rejected, and credential query parameters and fragments are removed.
Put credentials in headers, not the URL.

For Qwen 3.8, replace the shared host with the workspace host for the API key's
region, such as `{WorkspaceId}.cn-beijing.maas.aliyuncs.com` or
`{WorkspaceId}.ap-southeast-1.maas.aliyuncs.com`. Qwen 3.8 requires this dedicated
host; older models can continue using the shared Beijing or Singapore host.
See [official Qwen endpoint requirements](https://help.aliyun.com/zh/model-studio/omni-realtime-websocket-access).

The GUI exposes three authentication modes:

- **Bearer**: the main API-key field becomes `Authorization: Bearer …`.
- **api-key**: the main field becomes an `api-key` header for gateways that
  require it. This does not change the selected wire protocol.
- **Custom headers only**: supply the gateway's authentication in Extra headers
  as a JSON object of string values. The main API-key field is optional and is
  not used in this mode.

Bearer and api-key modes control their own authentication headers; conflicting
Extra headers do not override them. Framing headers are rejected:
`Host`, `Connection`, `Upgrade`, `Content-Length`, `Transfer-Encoding`,
`Proxy-Authorization` and every `Sec-WebSocket-*` header. Invalid header names,
duplicate names with different casing, and CR/LF values are rejected too.

## Choose OpenAI reasoning effort

The GUI offers `auto`, `omit`, `none`, `minimal`, `low`, `medium`, `high` and
`xhigh` for the OpenAI GA profile. This is separate from the text LLM's thinking
toggle.

- **auto**, the default, sends `low` for recognized `gpt-realtime-2.x` names and
  omits the field for other names. It is the conservative starting point for
  normal dictation.
- **omit** leaves reasoning to the server; it does not mean reasoning is off.
- **minimal** through **xhigh** request an explicit level. `minimal` is below
  `low`; `low` is not the lowest documented level. Known legacy OpenAI model
  names omit unsupported reasoning fields even if an explicit level is saved.
  Custom aliases require the gateway to support the requested setting.
- **none is experimental**. It is not in the public Realtime effort enum, even
  though selected endpoints have accepted it. It is not guaranteed for every
  model or gateway. See the [official Realtime effort schema](https://developers.openai.com/api/reference/resources/realtime/client-events)
  and [reasoning guidance](https://developers.openai.com/api/docs/guides/voice-prompting?voice-api=realtime).

With fallback enabled, an initial setup error explicitly rejecting experimental
`none` at `reasoning.effort` causes one setup retry with `low`, before audio is
sent. Disable fallback to surface that rejection directly. Authentication,
quota, rate-limit, connection and unrelated errors never trigger this fallback;
a second rejection fails.

## Recording, context and auxiliary actions

Microphone PCM is queued during connection setup, then streamed while recording.
Stopping flushes audio, commits the turn and sends `response.create`; Qwen uses
a bare event rather than the GA response payload. Server VAD is disabled.
The result is text, without generated speech or a second polishing call.

All profiles reuse native style, vocabulary, working-language, Chinese-script
and translation settings. Cursor context uses the existing host snapshot and
switch, with no second window reader. Enabled conversation context includes up
to two filtered final texts, at most 2,000 Unicode characters each. References
resolve terms without becoming dictated content or executable instructions.

Dictation transcribes questions and requests rather than answering them. Clear,
committed self-corrections affect only this recording; quoted or undecided edits
remain content. Explicit application translation takes precedence. Results use
normal script conversion, preview, insertion and history. Failed, cancelled,
empty or incomplete responses are not accepted as successful partial dictations.

For the **Qwen profiles**, the selection assistant's initial audio-to-instruction
step is available, but subsequent text editing and text Q&A use the configured
text LLM. Keep that LLM configured for those actions. Direct audio-attachment Q&A
through these Qwen profiles is explicitly unsupported; choose text Q&A or an
audio-Q&A provider. The OpenAI GA profile also supports native text requests.

The ordinary dictation/selection/voice-Q&A pipeline is separate from the history
page's **Retranscribe** operation. Current multimodal entries do not offer that
operation; older traditional entries and applicable quick notes still use the
selected ASR channel. When retiring an external ASR bridge, configure a usable
native ASR channel for these older recordings instead of leaving it pointed at
the stopped localhost service.

## Model candidates, validation and other WebSocket services

The model catalog supplies candidates, not proof of access. Custom model names
must implement the selected protocol. **Validate** performs a real handshake,
waits for session configuration acknowledgement and closes. It neither opens
the microphone, sends audio nor calls `response.create`: no billable model
response is generated, and dictation quality is not tested.

Existing WebSocket speech-recognition channels and Gemini Live Smart/ASR support
remain separate. Use those channels for ASR; a shared WS transport does not make
an ASR service a generative Omni provider.

Other services are not automatically supported by these three profiles:

| Service | Relevant boundary |
| --- | --- |
| [Gemini Live](https://ai.google.dev/gemini-api/docs/live-api/capabilities) | Uses Google's BidiGenerateContent protocol. Current native-audio models require AUDIO output; an output transcription is not text-only generation. |
| [xAI Voice Agent](https://docs.x.ai/developers/rest-api-reference/inference/voice) | Has similar event names, but no xAI generative profile is implemented here. A text delta event alone does not establish the full text-only session contract. |
| [Doubao Seeduplex](https://docs.volcengine.com/docs/doubao-voice/end-to-end-real-time-voice-full-duplex-version?lang=zh) | Uses its own session creation, audio formats and keep-alive rules, with a separate older binary protocol as well. Neither is implemented by these profiles. |
| [StepAudio Realtime](https://platform.stepfun.ai/docs/en/api-reference/realtime/chat) | Its public Realtime schema fixes output to text plus audio. This is not a text-only Omni adapter; existing StepFun ASR is a different service. |

## OpenAI subscription API credits

Eligible paid ChatGPT Pro subscribers can activate API credits by linking a
same-email API organization they own; the subscription must be at least seven
days old. Allowances depend on the plan. Grants recur on eligible subscription
renewals while the benefit is offered; the first is prorated, and unused credits
expire at the billing-cycle boundary without rollover. OpenAI explicitly covers
Realtime API usage billed to that linked organization. Use its project API key,
check the current balance and model access, and do not assume permanent free
usage or coverage for third-party gateways. See the current
[official eligibility and activation conditions](https://developers.openai.com/api/docs/guides/chatgpt-api-credits).
