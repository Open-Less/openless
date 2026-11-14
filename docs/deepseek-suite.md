# DeepSeek suite and Q&A web search

Model services exposes **DeepSeek suite** next to multimodal models. Enabling it
selects the traditional pipeline and configures DeepSeek ASR and `deepseek-flash`
channels with the supplied key. Existing DeepSeek channels are reused; otherwise
new channels are created. The configured channels move to the first enabled
position. Other channels remain available. Connection validation uses the usual
channel controls.

## Speech

ASR uses `POST /v1/tools/asr`, multipart `file` only, with 16 kHz mono PCM WAV.
Recording completes before upload. Long recordings are split into at most
120-second segments (under the 8 MiB limit). Model, language and prompt fields
are not sent. Transcripts are joined using the existing batch ASR behavior.

The Q&A answer's read-aloud control streams DeepSeek PCM over WebSocket through
the native host, then schedules 24 kHz signed 16-bit audio in the WebView. The API
key stays in the host. `stella` and `tide` voices are available. The explicit
`finish` event must report success; early closure and provider errors fail the
playback. Stop, replacement and window/component disposal cancel playback.
Read-aloud starts on user action after an answer is complete; it does not speak
partial model output automatically. The host currently exposes playback to the
Tauri Q&A and main windows.

## Search

Advanced → Web Search offers Auto, free Bing RSS, Tavily, Brave, SearXNG and Off.
Auto enables DeepSeek's `deepseek_web_search` and `deepseek_read_url` only for
`deepseek-flash` on the exact official HTTPS endpoint using Chat Completions.
Existing models and gateways are not silently renamed. They use the general
search path. Native final-chunk citations become clickable source links;
content-filter and stream-error responses discard the partial answer.

Other models, including multimodal Q&A, first choose a short query or no search.
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

## Official contracts

- [DeepSeek November 13 announcement](https://api-docs.deepseek.com/news/news261113/)
- [DeepSeek ASR](https://api-docs.deepseek.com/zh-cn/guides/asr/)
- [DeepSeek TTS](https://api-docs.deepseek.com/zh-cn/guides/tts/)
- [DeepSeek web tools](https://api-docs.deepseek.com/zh-cn/guides/web_tools/)
- [Tavily search](https://docs.tavily.com/documentation/api-reference/endpoint/search)
- [Brave authentication](https://api-dashboard.search.brave.com/documentation/guides/authentication)
- [SearXNG search API](https://docs.searxng.org/dev/search_api.html)
