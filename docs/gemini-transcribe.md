# Gemini dedicated transcription

OpenLess supports both dedicated Gemini transcription models in Traditional and Multimodal modes:

| Model | During recording | After recording stops |
| --- | --- | --- |
| `gemini-3.5-transcribe` | Audio is buffered locally. | Recorded audio is uploaded and transcribed. |
| `gemini-3.5-transcribe-live` | Audio is streamed and a draft transcript appears while you speak. | The final transcript replaces the draft. Multimodal mode applies Smart cleanup. |

Live recordings can last up to 10 minutes. The draft may change as the model receives more audio; only the finalized result is used for subsequent processing.

When the global stable transcription setting is enabled, even the Live model connects only after recording stops and transcribes the buffered audio without recording-time previews.

## Traditional: verbatim ASR + optional LLM

1. Select Traditional mode.
2. Add a speech recognition channel and choose Google Gemini Transcribe.
3. Enter your Gemini API key in that channel.
4. Choose `gemini-3.5-transcribe-live` to see results while speaking, or `gemini-3.5-transcribe` to transcribe after recording stops.
5. Use `https://generativelanguage.googleapis.com/v1beta` as the Base URL. The complete `https://generativelanguage.googleapis.com/v1beta/interactions` endpoint is also accepted. Live also accepts a complete WebSocket endpoint, such as `wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent`.
6. Click Verify and enable the channel. Configure the language model channel separately if you want polishing.

This channel returns a verbatim transcript. The separately configured LLM can apply your style prompt or translate the result. Credentials remain in the ASR channel; the LLM has its own credentials.

## Multimodal: Smart transcription

1. Select Multimodal mode.
2. Choose Google Gemini in the multimodal model settings.
3. Enter `gemini-3.5-transcribe-live` or `gemini-3.5-transcribe` and your Gemini API key.
4. Use the same Base URL described above, then click Verify.

This route enables the API's `smart` mode. It removes filler words, resolves spoken self-corrections, and formats the final transcript without a second LLM call. With Live, a draft appears during recording and the cleaned final result arrives after recording stops. Working languages and dictionary terms become structured transcription hints. An empty working-language list enables automatic detection, including mixed-language speech.

Multimodal credentials are independent of the Traditional ASR channel. Other Gemini models keep the existing general-purpose Gemini route.

The dedicated models reject system instructions. Smart transcription therefore does not apply OpenLess style prompts, conversation context, or output-language preferences. Translation requests, text-only polishing and Q&A are rejected instead of returning a transcript as if it were a translation or answer. Use Traditional mode with an LLM for these tasks.

## Request and verification behavior

For `gemini-3.5-transcribe`, OpenLess encodes recorded PCM as a 16 kHz mono WAV, uploads it through the Files API, and passes the returned file URI and `audio/wav` MIME type to the Interactions API. Traditional and Multimodal modes share this transport. Temporary uploaded files are deleted after the request, including on transcription failure. Cancelling a Multimodal call also cancels the interaction and lets the worker finish cleanup.

For `gemini-3.5-transcribe-live`, OpenLess sends 16 kHz mono PCM through the Live WebSocket API. Recording start and stop explicitly delimit the speech activity, so pauses do not finish the recording. The connection is closed on completion or cancellation; no Files API upload is needed. Base URLs and explicit WebSocket endpoints use the application's configured proxy behavior.

Verify exercises the selected transcription mode using 500 milliseconds of silence. A completed response without text is a valid result for silence. This tests connectivity, not recognition quality; confirm that with a short recording. HTTP failures retain their status for the settings error display; provider response bodies and credentials are not logged.

## Real API smoke test

The following example reads `GEMINI_API_KEY` from the environment and uses in-memory credentials. It runs the real settings Verify service, then transcribes the supplied audio with the production ASR client or Multimodal engine. It makes billable API requests when applicable.

From `openless-all/app`, supply a raw signed 16-bit little-endian, 16 kHz mono PCM file containing speech:

```sh
cargo run -p openless-core --locked --example gemini_transcribe_smoke -- speech.pcm
cargo run -p openless-core --locked --example gemini_transcribe_smoke -- speech.pcm https://generativelanguage.googleapis.com/v1beta/interactions
cargo run -p openless-core --locked --example gemini_transcribe_smoke -- --smart speech.pcm
cargo run -p openless-core --locked --example gemini_transcribe_smoke -- --smart speech.pcm https://generativelanguage.googleapis.com/v1beta/interactions
cargo run -p openless-core --locked --example gemini_transcribe_smoke -- --live speech.pcm
cargo run -p openless-core --locked --example gemini_transcribe_smoke -- --live --smart speech.pcm
```

For example, FFmpeg can convert an existing recording with `ffmpeg -i speech.wav -ar 16000 -ac 1 -f s16le speech.pcm`.

Offline regression tests require no API key:

```sh
cargo test -p openless-core --locked --lib asr::gemini
```

## Official references

- [Audio transcription guide](https://ai.google.dev/gemini-api/docs/transcribe)
- [Interactions API reference](https://ai.google.dev/api/interactions-api)
- [Interactions OpenAPI schema](https://ai.google.dev/static/api/interactions.openapi.json)
- [Live transcription guide](https://ai.google.dev/gemini-api/docs/live-api/live-transcribe)
