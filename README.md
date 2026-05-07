<p align="center">
  <img src="openless-all/app/src-tauri/icons/128x128@2x.png" alt="OpenLess" width="160" />
</p>

<h1 align="center">OpenLess</h1>

<p align="center">
  <strong>Open-source voice input for macOS &amp; Windows.</strong><br/>
  Press a hotkey, speak, get AI-polished text at your cursor.
</p>

<p align="center">
  <a href="https://openless.top"><strong>🌐 Official site — openless.top</strong></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh.md">中文</a>
</p>

<p align="center">
  <a href="https://github.com/appergb/openless/releases/latest"><img alt="release" src="https://img.shields.io/github/v/release/appergb/openless?style=flat-square&color=2c5282" /></a>
  <a href="https://github.com/appergb/openless/blob/main/LICENSE"><img alt="license" src="https://img.shields.io/github/license/appergb/openless?style=flat-square&color=2f855a" /></a>
  <img alt="macOS" src="https://img.shields.io/badge/macOS-12%2B-1f425f?style=flat-square" />
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-0078d4?style=flat-square" />
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24c8db?style=flat-square" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-ce422b?style=flat-square" />
  <img alt="Stars" src="https://img.shields.io/github/stars/appergb/openless?style=flat-square&color=805ad5" />
</p>

<p align="center">
  <strong>Join our QQ Group: 1078960553</strong>
</p>

<p align="center">
  <strong>Sponsors</strong>
</p>

<p align="center">
  <a href="https://www.knin.net" target="_blank" rel="noopener">
    <img alt="悠雾云数据 (Youwu Cloud Data)" src="https://www.knin.net/upload/logo.png" height="48" />
  </a>
  &nbsp;&nbsp;
  <a href="https://jiangmuran.com/" target="_blank" rel="noopener">
    <img alt="jiangmuran" src="assets/people/jiangmuran.png" width="48" height="48" />
  </a>
  <br/>
  <a href="https://www.knin.net" target="_blank" rel="noopener">悠雾云数据 — www.knin.net</a>
  &nbsp;·&nbsp;
  <a href="https://jiangmuran.com/" target="_blank" rel="noopener">jiangmuran — jiangmuran.com</a>
</p>

<p align="center">
  <strong>Developers</strong>
</p>

<p align="center">
  <a href="https://tripmc.top/" target="_blank" rel="noopener">
    <img alt="TRIP" src="assets/people/tripmc.png" width="80" height="80" />
  </a>
  &nbsp;&nbsp;
  <a href="https://chris233.qzz.io" target="_blank" rel="noopener">
    <img alt="Chris233" src="assets/people/Chris233.png" width="80" height="80" />
  </a>
  &nbsp;&nbsp;
  <a href="https://github.com/Cooper-X-Oak" target="_blank" rel="noopener">
    <img alt="Cooper" src="https://github.com/Cooper-X-Oak.png?size=160" width="80" height="80" />
  </a>
  <br/>
  <a href="https://tripmc.top/" target="_blank" rel="noopener">TRIP — tripmc.top</a>
  &nbsp;·&nbsp;
  <a href="https://chris233.qzz.io" target="_blank" rel="noopener">Chris233 — chris233.qzz.io</a>
  &nbsp;·&nbsp;
  <a href="https://github.com/Cooper-X-Oak" target="_blank" rel="noopener">Cooper — github.com/Cooper-X-Oak</a>
</p>

---

OpenLess is a cross-platform (macOS & Windows) voice-input app — a **fully open-source** alternative to commercial tools like [Typeless](https://www.typeless.com/), [Wispr Flow](https://wisprflow.ai), [Lazy](https://heylazy.com), and Superwhisper. Official site: [openless.top](https://openless.top).

Put your cursor in any text field — ChatGPT, Claude, Cursor, Notion, an email draft, a chat box — press one global hotkey and talk. OpenLess records, transcribes, polishes the text in the mode you picked, and inserts the result at the cursor. If insertion is blocked it copies to the clipboard, so the words you spoke don't get lost.

Unlike voice typing tools that just dump a word-for-word transcript, OpenLess's headline mode is **AI-prompt mode**: you ramble, it adds structure, lists constraints, and produces a context-rich prompt you can paste straight into ChatGPT / Claude / Cursor.

## A concrete example

Hold the hotkey, say to OpenLess:

> uh… so… I want ChatGPT to write me a SQL query, from the orders table get last month's orders, group by customer, sort by amount desc, top ten

Release the hotkey. A second later your input box reads:

```text
Please write a SQL query that:

- Pulls orders from last month from the `orders` table.
- Groups by customer.
- Sorts by total amount, descending.
- Returns the top 10 rows only.
```

No edits needed. Hit Enter and ask GPT. That's the whole pitch: **write prompts with your mouth, faster and cleaner than typing them.**

## Why OpenLess is open source

The closest tools are subscription SaaS: monthly bill, no bring-your-own model, your audio uploaded to the vendor, your dictionary and habits living in their account.

OpenLess goes for the same end-user experience but:

- **Fully open source, local-first.** Code is in this repo; all your data stays on your machine.
- **Bring your own cloud credentials.** Volcengine streaming ASR + Ark / DeepSeek-compatible chat-completions. No vendor lock-in.
- **Tuned for AI prompts.** The "Structured" mode reshapes loose speech into a prompt with context, constraints, and asks — paste straight into ChatGPT, Claude, or Cursor.
- **Won't answer for you.** The model only cleans up your text. If you say "what features does this app still need?", it returns that as a clean question — it does not hand you a feature list. Ask the real AI for that.

## Use cases

- Writing prompts for ChatGPT / Claude / Cursor / Gemini: dictate a request, OpenLess turns it into a structured, detailed prompt.
- Drafting emails, specs, long Slack/WeChat messages: removes filler, fixes punctuation, organizes paragraphs.
- Code comments, commit messages, PR descriptions: dump what's in your head straight to the cursor.
- Any "I don't want to type but I have to produce written text" situation.

## Project direction

OpenLess does one thing: **turn speech into usable written text (especially AI prompts), at the current cursor.**

- It does not answer questions, run tasks, or analyze your project.
- It does not accumulate conversation context — every dictation is an independent cleanup request.
- Speech → transcript → cleanup → insert at cursor. Clipboard fallback on failure.
- Everything else (modes, dictionary, history, menu bar, home report) supports that one path.

## Comparison

| Tool | Form | How OpenLess differs |
| --- | --- | --- |
| [Typeless](https://www.typeless.com/) | Closed-source macOS / Windows / iOS, subscription | Open source; explicit AI-prompt mode; bring-your-own ASR + LLM; data and dictionary stay on your machine |
| [Wispr Flow](https://wisprflow.ai) | Closed-source macOS / Windows, subscription | Open source; bring-your-own ASR + LLM; transparent prompt-handling rules |
| [Lazy](https://heylazy.com) | Closed-source notes / capture tool | Not a notes container — inserts straight into any input field |
| [Superwhisper](https://superwhisper.com) | Closed-source macOS, subscription | Open source; cloud ASR today, local ASR on the roadmap |

## Status (v1.2)

- Tauri 2 + Rust backend + React/TS frontend. macOS 12+, Windows 10+.
- **Toggle and push-to-talk** recording modes. `Esc` cancels at any phase, including polish/insert.
- **Cloud ASR**: Volcengine streaming ASR, OpenAI Whisper-compatible batch ASR, Apple Speech (macOS).
- **Local ASR**: bundled Qwen3-ASR (0.6B / 1.7B) via vendored `antirez/qwen-asr`; Windows Foundry Local Whisper variants.
- **Polish providers**: Ark / DeepSeek / OpenAI / Doubao / Anthropic-compatible chat-completions, plus any OpenAI-compatible endpoint you bring.
- 4 output modes: raw, light polish, structured (**AI prompt mode**), formal. Plus a **translation hotkey** that converts speech directly into the configured target language ([#43](../../issues/43)).
- **Selection-ask QA panel** — separate hotkey opens a floating panel that runs voice Q&A against the highlighted text in any app ([#118](../../issues/118)).
- Main window: Overview / History / Vocab / Style / Settings. Persistent tray icon. Mini status capsule floating on screen.
- **Multilingual UI** — Settings → Language switches between 简体中文 / 繁體中文 / English / 日本語 / 한국어 (auto-detects on first launch).
- **In-app auto-update** — Settings → About → Check button; signed updater artifacts via Tauri updater plugin.
- **Beta channel (opt-in)** — Settings → About → Join Beta channel exposes the latest pre-release build for manual download; Beta releases never reach Stable users automatically (see [Contributing workflow](#contributing-workflow)).
- **Distribution channels** — direct DMG/EXE from [Releases](../../releases), Homebrew Cask (`brew install --cask openless`), Windows installer.
- **Single-instance lock** — prevents two OpenLess processes from racing the same hotkey edge.
- Dictionary entries injected as Volcengine ASR `context.hotwords` and as semantic hints during polish; hits accumulate per session.
- Platform-native global hotkey: CGEventTap on macOS, low-level keyboard hook (`WH_KEYBOARD_LL`) on Windows.

## Download & install (end users)

Go to [Releases](../../releases) and download:
- **macOS**: `OpenLess_<version>_aarch64.dmg` — open, drag to `/Applications`
- **Windows**: `OpenLess_<version>_x64-setup.exe` — run the installer
- **macOS(brew install)**:
```bash
brew tap appergb/openless https://github.com/appergb/openless
brew install --cask openless
xattr -cr /Applications/OpenLess.app

# Upgrade to the latest version
brew update && brew upgrade openless
```

On first launch, grant the permissions the app requests:

**macOS:**
1. Grant Microphone access.
2. Grant Accessibility access.
3. **Quit and reopen the app** — Accessibility only takes effect after a restart.
4. Open Settings → fill in Volcengine ASR + Ark credentials.

**Windows:**
1. Grant Microphone access when prompted.
2. Open Settings → Permissions to verify the global hotkey listener is active.
3. Fill in Volcengine ASR + Ark credentials in Settings.

Full end-user walkthrough: [USAGE.md](USAGE.md).

## Build from source (developers)

The active codebase is in `openless-all/app/` (Tauri 2 + Rust + React/TS). The macOS build links a vendored C ASR engine ([`antirez/qwen-asr`](https://github.com/antirez/qwen-asr)) pulled in as a git submodule under `src-tauri/vendor/qwen-asr/`, so initialize submodules on first clone.

```bash
# First clone only — pull in vendored submodules
git submodule update --init --recursive

cd "openless-all/app"
npm ci

# Dev: Vite at :1420 + Tauri shell
npm run tauri dev

# macOS release build (signs, installs, resets TCC)
./scripts/build-mac.sh
INSTALL=0 ./scripts/build-mac.sh   # build only, skip install

# Rust type-check without full compile
cargo check --manifest-path src-tauri/Cargo.toml

# Frontend TS check
npm run build
```

Logs: `~/Library/Logs/OpenLess/openless.log` (macOS) / `%LOCALAPPDATA%\OpenLess\Logs\openless.log` (Windows).

**Windows build** — see [`openless-all/README.md`](openless-all/README.md) for MSVC vs GNU/MinGW routes.

## Contributing workflow

OpenLess uses a two-channel branching model.

- **`beta`** — the **Beta channel**. Default branch and integration buffer; all in-progress development lands here. Beta builds may exist but are **not pushed to regular users** — they only reach people who explicitly opt into the Beta channel.
- **`main`** — the **Stable channel (正式版)**. Always-releasable. The build everyone gets by default.

```text
your fork / topic branch
        │  (test locally on your target platform first)
        ▼
   PR → beta  ← AI review (one pass, advisory only)
        │     ← maintainer lightweight glance (scope, cross-module impact)
        ▼
       merged into beta
        │  (periodically, after a two-platform smoke build)
        ▼
       merged into main  →  tag `v<version>-tauri`  →  release CI → Stable users
```

Rules of thumb:

- **Open PRs against `beta`, never against `main`.** GitHub already defaults the base branch to `beta` for new PRs.
- **Verify the change on your target platform before opening the PR** — build green is necessary, manual verification is required.
- **AI review runs once per PR and is advisory.** Don't loop on it. Apply your judgment.
- **Keep AI rework rounds tight (1–2).** If a fix resists, ask a human or restart with fresh context — multi-round AI back-and-forth tends to do more harm than good here.
- **Beta work must not leak to Stable.** `main` only receives merges from `beta`, performed by maintainers after a successful two-platform smoke build. No direct pushes to `main`.
- **Stable releases are cut from `main`** by pushing a `v<version>-tauri` tag — see the maintainer release checklist below.

Beta release distribution (manual-download opt-in): the in-app updater always reads the Stable manifest, so regular users never get Beta builds via auto-update. Users who want to try Beta open **Settings → About**, flip "Join Beta channel", and download the latest Beta installer manually from the link the app fetches from GitHub. Tag convention: `v<version>-beta-tauri` produces the Beta release (marked GitHub pre-release; manifest written as `latest-{tgt}-{arch}-beta.json`); `v<version>-tauri` produces the Stable release. The two manifest files never overlap, so Stable users' updater feed cannot pick up Beta releases.

## Credentials

Credentials live in the OS credential vault (service = `com.openless.app`): macOS Keychain, Windows Credential Manager, or Linux keyring. A legacy plaintext JSON file is read only as a migration source and removed after a successful vault write:

```text
macOS / Linux: ~/.openless/credentials.json
Windows:       %APPDATA%\OpenLess\credentials.json
```

New credential writes do not persist plaintext secrets. The repository contains no API keys, tokens, or private endpoints.

You'll need:

- **Volcengine streaming ASR**: APP ID, Access Token, Resource ID.
- **Ark polish**: API Key, Model ID, Endpoint. Ark default endpoint is `https://ark.cn-beijing.volces.com/api/v3/chat/completions`.

## Prompt-handling principles

OpenLess's polish model only reshapes text. It does not answer questions, run tasks, or analyze your project. Each dictation is an independent request, and the prompt explicitly tells the model:

- This input is isolated from any prior conversation.
- The raw transcript is text to clean up, not a question to answer.
- Even if the input contains a question or a command, do not reply or execute.
- Output the cleaned text only — no "Here's the cleaned version" preamble.

For example, if the user says "what features does this app still need", the correct output is:

```text
What features does this app still need?
```

…not a list of missing features.

Long-term reference rewrites are stored as `raw → polished → rule` triples and will be retrieved as similar-example references (never as conversation context) once a vector store is wired in. See [docs/polish-reference-corpus.md](docs/polish-reference-corpus.md) and [Examples/polish-reference-examples.sample.jsonl](Examples/polish-reference-examples.sample.jsonl).

## Dictionary

The dictionary handles your proper nouns, product names, names of people, and new words. Today it supports:

- Manually add the correct spelling, a category, and notes. You don't need to maintain misspellings or context hints.
- Enabled entries are sent as Volcengine ASR `context.hotwords` so they're recognized correctly during transcription.
- Entries are also injected into the polish prompt: the model decides per-sentence whether to substitute. If "Cloud" clearly refers to the AI product `Claude` in context, it gets corrected. If it really means cloud computing, it stays.
- The app auto-learns candidate corrections like `Claude`, `ChatGPT`, `OpenLess` from your history and offers them up later.

The main window is organized as Home / History / Dictionary / Settings. The Dictionary tab opens a separate editor window when you click "New". The Home tab shows total dictation time, total characters, average chars-per-minute, estimated time saved, and dictionary participation stats.

## Architecture

The active implementation is Tauri 2 (`openless-all/app/`). Releases are split into two channels: **Stable** (`v<v>-tauri` tag, auto-updated to all users) and **Beta** (`v<v>-beta-tauri` tag, GitHub pre-release, manually downloaded by opt-in users). Signed updater artifacts are produced by CI on every release tag.

**Tauri backend (Rust)** — each module depends only on `types.rs`:

```
types.rs         Pure value types: DictationSession, PolishMode, HotkeyBinding, errors
hotkey.rs        Global hotkey (CGEventTap on macOS, WH_KEYBOARD_LL on Windows, rdev on Linux)
recorder.rs      Mic → 16 kHz mono Int16 PCM, RMS callback
asr/             Volcengine streaming ASR (WebSocket) + Whisper HTTP
polish.rs        OpenAI-compatible chat-completions (Ark / DeepSeek / etc.)
insertion.rs     AX focused-element → clipboard + Cmd+V → copy-only fallback
persistence.rs   History / preferences / vocab JSON + platform credential vault
permissions.rs   TCC checks (Accessibility / Microphone)
coordinator.rs   State machine: Idle → Starting → Listening → Processing
commands.rs      Tauri IPC surface
```

**React frontend (`src/`)** — state via Recoil atoms (`pages/_atoms.tsx`); hotkey capability/binding via `HotkeySettingsContext`; all backend calls go through `lib/ipc.ts`.

The dictation pipeline: `hotkey edge → Recorder.start + ASR.openSession → [audio frames] → hotkey edge → Recorder.stop + ASR.sendLastFrame → Polish → Insert → History.save`.

See [CLAUDE.md](CLAUDE.md) for invariants and module-wiring rules.

## Roadmap

Planned but not yet shipped:

- Dictation translation mode: hold a separate hotkey, speak in your language, insert in target language ([#43](../../issues/43)).
- Cross-session style memory: polish learns user's tone over time ([#46](../../issues/46)).
- Snippets (no UI / trigger logic yet).
- History enhancements: copy button, search, re-polish, re-insert.
- "Paste last result" hotkey.
- Multi-monitor capsule placement on the focused screen.

## Maintainer release checklist

OpenLess ships two release channels. Branch name = channel name (see [Contributing workflow](#contributing-workflow)).

### Common prep (both channels)

- Bump version in **all five** files: `package.json`, `package-lock.json` (root + nested entry under `packages.""`), `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `Cargo.lock` (look for the `name = "openless"` block). CI's `Verify version sync` step will fail the build otherwise.
- Run `INSTALL=0 ./scripts/build-mac.sh` and confirm the `.app` launches.
- Smoke-test on a clean machine: permission flow, hotkey, recording, ASR, polish, insertion, clipboard fallback.
- Confirm `TAURI_SIGNING_PRIVATE_KEY` and (for macOS) the Apple signing/notarization secrets are set on the repo.

### Beta channel — `v<v>-beta-tauri`

1. Land changes onto the `beta` branch via PR review.
2. Push tag **on `beta`**: `git tag v<v>-beta-tauri && git push origin v<v>-beta-tauri`.
3. CI tags the GitHub Release as `Pre-release` and uploads only `latest-{tgt}-{arch}-beta.json` updater manifests. Stable users' `releases/latest` redirect is unaffected.
4. Announce in the appropriate channel (issue thread, QQ group) that opt-in Beta users can grab it from Settings → About → Join Beta channel.

### Stable channel — `v<v>-tauri`

1. Merge `beta → main` after the Beta release has soaked enough (or run a final two-platform smoke build directly).
2. Push tag **on `main`**: `git tag v<v>-tauri && git push origin v<v>-tauri`.
3. CI publishes a normal GitHub Release and uploads `latest-{tgt}-{arch}.json` (no `-beta` suffix). All Stable users get the update through the in-app updater.

### Post-release verification (always run)

Run the 5-step checklist in [`CLAUDE.md` → Branch & release-channel workflow → Channel distribution](CLAUDE.md): page status (pre-release flag), asset filename channel-correctness, Stable user flow, Beta opt-in flow, raw endpoint sanity.

## Acknowledgements

OpenLess sincerely thanks our sponsors, developers/contributors, and the broader LinuxDo community.

We appreciate sponsors for making sustained project work possible, and we thank developers and contributors for building, reviewing, and improving OpenLess.

OpenLess also recognizes and appreciates the LinuxDo community for its open, practical, and developer-friendly atmosphere. Many ideas, discussions, and early feedback around OpenLess were inspired by the broader open-source spirit represented by LinuxDo.

This acknowledgement does not imply official endorsement or affiliation.

## License

MIT
