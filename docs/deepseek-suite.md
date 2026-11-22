# DeepSeek suite and Q&A web search

The recognition pipeline selector has three persistent choices: Traditional,
Multimodal and DeepSeek suite. Closing Settings, closing a window and restarting
the app preserve the selected pipeline. Each keeps its own configuration.

DeepSeek suite uses the dedicated application credential `deepseek_suite.config`
(`apiKey`, `voice`). First setup copies the first enabled, configured official
DeepSeek LLM channel key in channel order. Once saved, the suite credential is
independent, including an explicitly empty key. Configuration never reorders or
modifies traditional or multimodal channels. The suite resolves to a synthetic
provider with the official endpoint and `deepseek-flash`; traditional local ASR
fallback settings do not apply. The dedicated credential stays in the local vault.

## Speech

ASR uses `POST /v1/tools/asr`, multipart `file` only, with 16 kHz mono PCM WAV.
Recording completes before upload. Long recordings are split into at most
120-second segments (under the 8 MiB limit). Model, language and prompt fields
are not sent. Transcripts are joined using the existing batch ASR behavior.

Only the DeepSeek suite pipeline exposes read aloud, guarded in both the UI and
native command. The Q&A answer's read-aloud control streams DeepSeek PCM over WebSocket through
the native host, then schedules 24 kHz signed 16-bit audio in the WebView. The API
key stays in the host. `stella` and `tide` voices are available. The explicit
`finish` event must report success; early closure and provider errors fail the
playback. Stop, replacement and window/component disposal cancel playback.
Read-aloud starts on user action after an answer is complete; it does not speak
partial model output automatically. The host currently exposes playback to the
Tauri Q&A and main windows.

## Search

Traditional and multimodal Q&A use Advanced → Web Search: free Bing RSS (the
default), Tavily, Brave, SearXNG or Off. The legacy Auto value is a free-search
alias. Even a traditional DeepSeek LLM channel uses this standalone search path.
The model first chooses a short query or no search.

Only the DeepSeek suite pipeline enables `deepseek_web_search` and
`deepseek_read_url`, using its own key, `deepseek-flash`, Chat Completions and the
exact official HTTPS endpoint. It does not borrow the standalone search settings.
Native final-chunk citations become clickable source links; content-filter and
stream-error responses discard the partial answer. Every answering prompt names
the actual search backend (or states that search is off).

For free search, up to three public result pages are fetched with bounded time and size, DNS pinning and no redirects or credentials; unavailable pages retain their snippets. When requested, at most five bounded snippets are passed to the answering model
as untrusted evidence. This planning call incurs normal model usage even when no
search is needed. Missing weather location requires clarification. Free search
requires no key, but its public RSS interface can be unavailable or rate-limited.
Failure or empty results instruct the model to disclose that live facts could
not be verified. SearXNG requires a user-supplied `/search` endpoint with JSON
output enabled. Search keys are scoped to their selected fixed API origin.

Configuration is stored in the local credential store as `web_search.config`;
it does not enter provider credential sync. Search queries are sent to the chosen
search service and snippets to the answering model. Turning search off bypasses
planning and search. Native DeepSeek search and speech are paid provider services.

Conversation history retains answer text and displayed source links. Encrypted
DeepSeek tool-result signatures are not retained, so subsequent turns may search
or read links again rather than reuse earlier server-side page context.

## Conversation and presentation

Fn + Space starts a general voice question. Only an explicit nonempty native
selection is attached; the QA path never synthesizes Copy to infer selection.
The chord consumes Space down, repeats and release, while a plain Space passes
through. Prompts do not annotate the absence of a selection.

Until the user closes Q&A, subsequent questions retain its conversation and
messages. Recording and processing keep the previous answer expanded and visible;
a separate recording capsule displays the “Ask anything” badge. Focus loss does
not hide the Q&A panel. Cancelling a turn preserves the conversation; dismissing
it clears the conversation for the next question.

Each answer ends with numbered source chips on the left (site icons with a globe
fallback), Copy on the right, then suite-only Read aloud. Entrance, hover, press
and playback animations respect reduced-motion preferences.

## Official contracts

- [DeepSeek November 13 announcement](https://api-docs.deepseek.com/news/news261113/)
- [DeepSeek ASR](https://api-docs.deepseek.com/zh-cn/guides/asr/)
- [DeepSeek TTS](https://api-docs.deepseek.com/zh-cn/guides/tts/)
- [DeepSeek web tools](https://api-docs.deepseek.com/zh-cn/guides/web_tools/)
- [Tavily search](https://docs.tavily.com/documentation/api-reference/endpoint/search)
- [Brave authentication](https://api-dashboard.search.brave.com/documentation/guides/authentication)
- [SearXNG search API](https://docs.searxng.org/dev/search_api.html)
