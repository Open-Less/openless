# Alibaba Bailian ASR models

OpenLess uses one Alibaba Bailian provider and selects the DashScope protocol from the model name. Configure the API key, region or workspace endpoint, and model in Settings > Providers.

| Mode | Model examples | Behavior |
| --- | --- | --- |
| Realtime WebSocket | `fun-asr-realtime`, `fun-asr-flash-8k-realtime`, `paraformer-realtime-v2`, `sensevoice-realtime-v1` | Streams text while recording. OpenLess downsamples 16 kHz recorder audio for 8 kHz models. |
| Qwen realtime | `qwen3-asr-flash-realtime`, versioned snapshots | Streams text through the Qwen Realtime WebSocket API. |
| Synchronous recording | `fun-asr-flash-*`, `qwen3-asr-flash`, `qwen-audio-3.0-asr-flash` | Sends the recording after capture and waits for one synchronous response. These models are intended for short recordings (`qwen-audio-*-streaming` variants are not supported). |
| Asynchronous file transcription | `fun-asr`, `fun-asr-mtl`, versioned snapshots, `paraformer-v2` | Uploads the recording, starts an asynchronous task, polls it, then downloads the transcript. |

Future dated snapshots that retain one of these model prefixes are routed to the same protocol, so they can be entered manually before they are added to the model picker.

## Endpoints

The default endpoint is `dashscope.aliyuncs.com`. A workspace or regional host may also be entered, for example `https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com`. OpenLess preserves the host and derives the protocol-specific path and scheme automatically.

API keys are region-specific. The endpoint and API key must belong to the same region.

## Temporary uploads

Asynchronous models require a URL. OpenLess obtains a temporary upload policy from DashScope, uploads the local WAV to the returned OSS host, and submits the resulting `oss://` URL with `X-DashScope-OssResourceResolve: enable`.

Alibaba documents this temporary storage as a development and low-concurrency facility. It is rate limited, and uploaded URLs expire after 48 hours. OpenLess uploads one file per completed recording; it does not retain or reuse the temporary URL.

For deployment or high-concurrency use, use stable Alibaba Cloud OSS storage and the official DashScope API directly.

## References

- [Fun-ASR non-real-time HTTP API](https://help.aliyun.com/en/model-studio/fun-asr-recorded-speech-recognition-http-api)
- [Qwen-ASR API reference](https://help.aliyun.com/en/model-studio/qwen-asr-api-reference)
- [DashScope temporary file upload](https://help.aliyun.com/en/model-studio/get-temporary-file-url)
