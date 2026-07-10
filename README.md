<p align="center">
  <img src="openless-all/app/src-tauri/icons/128x128@2x.png" alt="OpenLess" width="128" height="128" />
</p>

<h1 align="center">OpenLess</h1>

<p align="center">
  <strong>Open-source voice input for macOS &amp; Windows</strong>
</p>

<p align="center">
  Hold a hotkey, speak, and watch AI-polished text stream straight to your cursor —<br/>
  in the writing style <em>you</em> choose.
</p>

<p align="center">
  <a href="https://openless.top"><strong>Website</strong></a>
  &nbsp;·&nbsp;
  <a href="https://github.com/appergb/openless/releases/latest"><strong>Download</strong></a>
  &nbsp;·&nbsp;
  <a href="README.md">English</a>
  &nbsp;/&nbsp;
  <a href="README.zh.md">中文</a>
</p>

<p align="center">
  <a href="https://github.com/appergb/openless/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/appergb/openless?style=flat-square&color=2c5282" /></a>
  <a href="https://github.com/appergb/openless/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/github/license/appergb/openless?style=flat-square&color=2f855a" /></a>
  <a href="https://github.com/appergb/openless/stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/appergb/openless?style=flat-square&color=805ad5" /></a>
  <a href="https://discord.gg/vTZHTFGFm"><img alt="Discord" src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat-square&logo=discord&logoColor=white" /></a>
</p>

<p align="center">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-12%2B-1f425f?style=flat-square&logo=apple&logoColor=white" />
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-0078d4?style=flat-square&logo=windows&logoColor=white" />
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24c8db?style=flat-square&logo=tauri&logoColor=white" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-ce422b?style=flat-square&logo=rust&logoColor=white" />
</p>

<p align="center">
  💬 &nbsp;<a href="https://discord.gg/vTZHTFGFm"><strong>Join our Discord</strong></a> &nbsp;·&nbsp; QQ Group&nbsp; <strong>1078960553</strong>
</p>

<br/>

<h2 align="center">Sponsors</h2>

<p align="center">
  <em>OpenLess is sustained by the generous support of its sponsors.</em>
</p>

<p align="center">
  <a href="https://jiangmuran.com/" target="_blank" rel="noopener">
    <img src="assets/people/jiangmuran.png" width="120" height="120" alt="jiangmuran" />
  </a>
</p>

<p align="center">
  <a href="https://jiangmuran.com/" target="_blank" rel="noopener"><strong>jiangmuran</strong></a><br/>
  <sub>⭐ Featured Sponsor · jiangmuran.com</sub>
</p>

<p align="center">
  Special thanks to <strong>jiangmuran</strong> for the sustained support that keeps OpenLess moving forward.
</p>

<p align="center">
  <sub>— with thanks to —</sub>
</p>

<p align="center">
  <a href="https://www.knin.net" target="_blank" rel="noopener">
    <img alt="悠雾云数据 (Youwu Cloud Data)" src="https://www.knin.net/upload/logo.png" height="52" />
  </a>
  <br/>
  <a href="https://www.knin.net" target="_blank" rel="noopener">悠雾云数据 · Youwu Cloud Data (knin.net)</a>
</p>

<h2 align="center">Developers</h2>

<table align="center">
  <tr>
    <td align="center" width="170">
      <a href="https://tripmc.top/" target="_blank" rel="noopener">
        <img src="assets/people/tripmc.png" width="80" height="80" alt="TRIP" /><br/>
        <strong>TRIP</strong>
      </a><br/>
      <sub>tripmc.top</sub>
    </td>
    <td align="center" width="170">
      <a href="https://chris233.qzz.io" target="_blank" rel="noopener">
        <img src="assets/people/Chris233.png" width="80" height="80" alt="Chris233" /><br/>
        <strong>Chris233</strong>
      </a><br/>
      <sub>chris233.qzz.io</sub>
    </td>
    <td align="center" width="170">
      <a href="https://github.com/Cooper-X-Oak" target="_blank" rel="noopener">
        <img src="assets/people/cooper.png" width="80" height="80" alt="Cooper" /><br/>
        <strong>Cooper</strong>
      </a><br/>
      <sub>github.com/Cooper-X-Oak</sub>
    </td>
  </tr>
</table>

---

OpenLess is a cross-platform (macOS and Windows) voice-input application — a fully open-source alternative to commercial tools such as [Typeless](https://www.typeless.com/), [Wispr Flow](https://wisprflow.ai), [Lazy](https://heylazy.com), and Superwhisper. Official site: [openless.top](https://openless.top).

Place your cursor in any text field — ChatGPT, Claude, Cursor, Notion, an email draft, a chat box — press a single global hotkey, and speak. OpenLess records the audio, transcribes it, polishes the text in the mode you selected, and inserts the result at the cursor. If insertion is blocked, the text is copied to the clipboard instead, so nothing you said is lost.

Unlike dictation tools that simply emit a word-for-word transcript, OpenLess's headline capability is its **AI-prompt mode**: you speak freely, and it adds structure, captures your constraints, and produces a context-rich prompt that you can paste directly into ChatGPT, Claude, or Cursor.

## From tool to infrastructure

Advanced systems all do the same thing: **they turn today's communication into tomorrow's default rules.** The Internet won not because HTTP was faster, but because no one negotiates how data is transmitted anymore. Git won not because the code was better, but because teams stopped coordinating versions every day. Stripe won not because payments were faster, but because developers stopped coordinating with banks.

What OpenLess does is not "faster dictation" — it **eliminates every act of coordination between a thought and clean text.** Turning what's in your head into usable text used to mean deciding, over and over: what tone for this one? how is that name spelled? which input field, copy or paste? where does the model's key live, and who has access? OpenLess **sediments** those repeated negotiations, layer by layer, into defaults:

- **Tone** sediments into a **style pack** — named, switchable, live at a keystroke, never re-decided.
- **Proper nouns** sediment into **dictionary hotwords** — recognized by the ASR, hinted to the polish model, never re-corrected.
- **Where text lands** sediments into a **single insertion protocol** — hold → speak → it appears at the cursor, with an automatic clipboard fallback.
- **Model and credentials** sediment into the **OS vault** — authorized once at launch, never renegotiated.

That is what **authorizing the infrastructure once, at launch** means: on first run you grant the microphone, accessibility, and cloud credentials exactly once; from that moment, "speak → clean text at the cursor" is no longer a negotiation you run each time, but a default protocol you can forget is even there. Judge OpenLess not by how much faster it is, but by how much **coordination it removes.**

## ✨ What's new

Two capabilities that sediment yet more of the coordination you used to repeat every day into defaults:

- 🎨 **Style Pack Marketplace.** OpenLess no longer ships a single fixed "polish" voice. Build your own **style packs** with custom system prompts, switch between them with a hotkey, and **install community packs in one click** — or publish your own to share. When a style is tuned to your exact task (cold emails, commit messages, 小红书 posts, formal reports, your team's tone), the output is not merely cleaner — it is *noticeably better*, because the model is finally writing the way you intend.
- ⚡ **Streaming insertion.** Text now flows to your cursor **character by character** as it is polished, rather than making you wait for the complete result. Perceived latency drops sharply, so dictation feels nearly as fast as thinking — and it automatically falls back to a one-shot paste when an application cannot accept streamed keystrokes.

## A concrete example

Hold the hotkey and say:

> uh… so… I need to reply to the client about the proposal they sent last time, we talked it over internally and the overall direction is fine but a few details need changing, first the delivery timeline is too tight we'd like to push it back two weeks, second on budget their quote came in about twenty percent higher than we expected can we negotiate that, and overall we're positive about working together so reassure them, keep the tone polite not too blunt

Release the hotkey. A moment later, your input box reads:

```text
Reply to the client about the proposal they sent. After internal discussion, the overall direction is fine, but a few details need adjusting:

1. Delivery timeline
   (a) The current timeline is too tight; we'd like to extend it by two weeks.
2. Budget
   (a) Their quote is about 20% higher than we expected — can we renegotiate?
Keep the overall message positive about the partnership, and keep the tone polite rather than blunt.
```

No edits are required — paste it into ChatGPT or Claude and let it draft the email. That is the core idea: **compose prompts by speaking, faster and more cleanly than typing them.**

## Why OpenLess is open source

The closest alternatives are subscription SaaS products: a monthly fee, no option to bring your own model, your audio uploaded to the vendor, and your dictionary and habits stored in their account.

OpenLess targets the same end-user experience, but:

- **Fully open source, local-first.** The code lives in this repository, and all of your data stays on your machine.
- **Bring your own cloud credentials.** Volcengine streaming ASR with Ark / DeepSeek-compatible chat completions. No vendor lock-in.
- **Tuned for AI prompts.** The structured mode reshapes loose speech into a prompt complete with context, constraints, and requests — ready to paste into ChatGPT, Claude, or Cursor.
- **It will not answer for you.** The model only cleans up your text. If you say "what features does this app still need?", it returns that as a clean question — it does not hand you a feature list. For that, ask the AI itself.

## Use cases

- **Writing prompts for ChatGPT / Claude / Cursor / Gemini** — dictate a request and OpenLess turns it into a structured, detailed prompt.
- **Drafting emails, specifications, and long Slack or WeChat messages** — filler is removed, punctuation is corrected, and paragraphs are organized.
- **Code comments, commit messages, and PR descriptions** — capture your thoughts directly at the cursor.
- **Any situation** where you must produce written text but would rather not type.

## Style packs & the Marketplace

A **style pack** is a named output style with its own system prompt. Rather than being constrained to one built-in "polish" voice, you can shape exactly how your speech is rendered — and the closer the style matches your real task, the better the result.

- **Create and customize.** On the **Style** page, add a pack and write a custom system prompt (for example, "terse engineering commit messages", "warm customer-support replies", or "小红书 copy with emoji"). Switch the active pack with a hotkey.
- **Install from the community.** Open the **Marketplace** to browse, search, and install packs others have shared in one click, and like the ones that work for you.
- **Publish your own.** Sign in with a GitHub identity (Settings → Marketplace), then select **Publish to Marketplace** from the Style page. Uploads are moderated before they appear publicly.

The marketplace is served by OpenLess's own moderated backend, so the catalog remains curated rather than a free-for-all.

## Project direction

OpenLess does one thing: it **turns speech into usable written text — AI prompts in particular — at the current cursor.**

- It does not answer questions, run tasks, or analyze your project.
- It does not accumulate conversation context; every dictation is an independent cleanup request.
- The pipeline is: speech → transcript → cleanup → insertion at the cursor, with a clipboard fallback on failure.
- Everything else (modes, dictionary, history, menu bar, home report) exists to support that single path.

## Comparison

| Tool | Form | How OpenLess differs |
| --- | --- | --- |
| [Typeless](https://www.typeless.com/) | Closed-source macOS / Windows / iOS, subscription | Open source; explicit AI-prompt mode; bring-your-own ASR + LLM; data and dictionary stay on your machine |
| [Wispr Flow](https://wisprflow.ai) | Closed-source macOS / Windows, subscription | Open source; bring-your-own ASR + LLM; transparent prompt-handling rules |
| [Lazy](https://heylazy.com) | Closed-source notes / capture tool | Not a notes container — inserts straight into any input field |
| [Superwhisper](https://superwhisper.com) | Closed-source macOS, subscription | Open source; cloud ASR today, local ASR on the roadmap |

## Status (v1.3.6)

Every item below is one more layer sedimented into a default — a capability you authorize once and then never manage again. This is the infrastructure you stand on after launch:

- Tauri 2 backend in Rust with a React/TypeScript frontend. macOS 12+, Windows 10+.
- 🎨 **Style Pack Marketplace** — browse, install, and like community **style packs** from the in-app Marketplace, and publish your own (custom system prompt per pack, switchable by hotkey). Backed by a moderated marketplace backend; uploads are reviewed before they go public.
- ⚡ **Streaming insertion** — polished text is written to the cursor character by character to reduce perceived latency, with an automatic one-shot-paste fallback. Toggle in Settings → Recording.
- **Toggle and push-to-talk** recording modes, plus a **MediaPlayPause trigger** so wired-earbud inline controls can start and stop recording. `Esc` cancels at any phase, including polish and insertion.
- **Cloud ASR**: Volcengine streaming ASR, OpenAI Whisper-compatible batch ASR, Apple Speech (macOS).
- **Local ASR**: bundled Qwen3-ASR (0.6B / 1.7B) via vendored `Open-Less/qwen-asr`; Windows Foundry Local Whisper variants.
- **Polish providers**: Ark / DeepSeek / OpenAI / Doubao / Anthropic-compatible chat completions, plus any OpenAI-compatible endpoint you bring.
- **Four output modes**: raw, light polish, structured (**AI-prompt mode**), and formal. Plus a **translation hotkey** that converts speech directly into the configured target language ([#43](../../issues/43)).
- **Selection-ask QA panel** — a separate hotkey opens a floating panel that runs voice Q&A against the highlighted text in any app ([#118](../../issues/118)).
- **Main window**: Overview / History / Vocab / Style / Marketplace / Settings. Persistent tray icon, plus a mini status capsule that floats on screen and follows the display you are typing on (multi-monitor).
- **Local model management** — manage on-disk local-ASR model storage from Settings.
- **Multilingual UI** — Settings → Language switches between 简体中文 / 繁體中文 / English / 日本語 / 한국어 (auto-detected on first launch).
- **In-app auto-update** — Settings → About → Check; signed updater artifacts via the Tauri updater plugin.
- **Beta channel (opt-in)** — Settings → About → Join Beta channel exposes the latest pre-release build for manual download. Beta releases never reach Stable users automatically (see [Contributing workflow](#contributing-workflow)).
- **Distribution channels** — direct DMG/EXE from [Releases](../../releases), Homebrew Cask (`brew install --cask openless`), and a Windows installer.
- **Single-instance lock** — prevents two OpenLess processes from racing the same hotkey edge.
- Dictionary entries are injected as Volcengine ASR `context.hotwords` and as semantic hints during polish; hits accumulate per session.
- Platform-native global hotkey: CGEventTap on macOS, low-level keyboard hook (`WH_KEYBOARD_LL`) on Windows.

## Download & install (end users)

Go to [Releases](../../releases) and download:

- **macOS**: `OpenLess_<version>_aarch64.dmg` (Apple Silicon) or `OpenLess_<version>_x64.dmg` (Intel). Open it, drag the app to `/Applications`, **then run the following once in Terminal to bypass the Gatekeeper "damaged" warning** (the build is ad-hoc signed, not Apple-notarized):
  ```bash
  xattr -cr /Applications/OpenLess.app
  ```
- **Windows**: `OpenLess_<version>_x64-setup.exe` — run the installer.
- **Android** (release APKs on GitHub Release; debug APKs via Actions artifacts):
  - **Phones (recommended)**: `OpenLess_<version>_arm64-v8a.apk`
  - **Older 32-bit ARM**: `OpenLess_<version>_armeabi-v7a.apk`
  - **64-bit emulator**: `OpenLess_<version>_x86_64.apk`
  - **32-bit emulator**: `OpenLess_<version>_x86.apk`
  - In-app updates (Settings → About) use `latest-android-{arch}.json` manifests; Beta users join Beta in Advanced settings.
  - Debug smoke builds: `OpenLess-android-debug-{abi}-*.apk` from workflow_dispatch artifacts.
  - If unsure, run `adb shell getprop ro.product.cpu.abi` and pick the matching APK.
- **macOS (Homebrew)**:
  ```bash
  brew tap appergb/openless https://github.com/appergb/openless
  brew install --cask openless
  xattr -cr /Applications/OpenLess.app

  # Upgrade to the latest version
  brew update && brew upgrade openless
  ```

On first launch, grant the permissions the app requests.

**macOS:**
1. Grant Microphone access.
2. Grant Accessibility access.
3. **Quit and reopen the app** — Accessibility only takes effect after a restart.
4. Open Settings and fill in your Volcengine ASR + Ark credentials.

**Windows:**
1. Grant Microphone access when prompted.
2. Open Settings → Permissions to verify the global hotkey listener is active.
3. Fill in your Volcengine ASR + Ark credentials in Settings.

For the full end-user walkthrough, see [USAGE.md](USAGE.md).

## Build from source (developers)

The active codebase lives in `openless-all/app/` (Tauri 2 + Rust + React/TS). The macOS build links a vendored C ASR engine ([`Open-Less/qwen-asr`](https://github.com/Open-Less/qwen-asr), forked from `antirez/qwen-asr`) pulled in as a git submodule under `src-tauri/vendor/qwen-asr/`, so initialize submodules on first clone.

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

# Rust type-check without a full compile
cargo check --manifest-path src-tauri/Cargo.toml

# Frontend TS check
npm run build
```

Logs: `~/Library/Logs/OpenLess/openless.log` (macOS) / `%LOCALAPPDATA%\OpenLess\Logs\openless.log` (Windows).

**Windows build** — see [`openless-all/README.md`](openless-all/README.md) for the MSVC and GNU/MinGW routes.

## Contributing workflow

OpenLess uses a two-channel branching model.

- **`beta`** — the **Beta channel**. The default branch and integration buffer; all in-progress development lands here. Beta builds may exist but are **not pushed to regular users** — they reach only those who explicitly opt into the Beta channel.
- **`main`** — the **Stable channel (正式版)**. Always releasable; the build everyone receives by default.

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
- **Verify the change on your target platform before opening the PR** — a green build is necessary, and manual verification is required.
- **AI review runs once per PR and is advisory.** Do not loop on it; apply your own judgment.
- **Keep AI rework rounds tight (1–2).** If a fix resists, ask a human or restart with fresh context — multi-round AI back-and-forth tends to do more harm than good here.
- **Beta work must not leak to Stable.** `main` receives merges only from `beta`, performed by maintainers after a successful two-platform smoke build. No direct pushes to `main`.
- **Stable releases are cut from `main`** by pushing a `v<version>-tauri` tag — see the maintainer release checklist below.

Beta release distribution (manual-download opt-in): the in-app updater always reads the Stable manifest, so regular users never receive Beta builds via auto-update. Users who want to try Beta open **Settings → About**, enable "Join Beta channel", and download the latest Beta installer manually from the link the app fetches from GitHub. Tag convention: `v<version>-beta-tauri` produces the Beta release (marked as a GitHub pre-release; manifest written as `latest-{tgt}-{arch}-beta.json`), while `v<version>-tauri` produces the Stable release. The two manifest files never overlap, so Stable users' updater feed cannot pick up Beta releases.

## Credentials

Credentials live in the OS credential vault (service = `com.openless.app`): macOS Keychain, Windows Credential Manager, or the Linux keyring. A legacy plaintext JSON file is read only as a migration source and removed after a successful vault write:

```text
macOS / Linux: ~/.openless/credentials.json
Windows:       %APPDATA%\OpenLess\credentials.json
```

New credential writes do not persist plaintext secrets. The repository contains no API keys, tokens, or private endpoints.

You will need:

- **Volcengine streaming ASR**: APP ID, Access Token, Resource ID.
- **Ark polish**: API Key, Model ID, Endpoint. The Ark default endpoint is `https://ark.cn-beijing.volces.com/api/v3/chat/completions`.

## Prompt-handling principles

OpenLess's polish model only reshapes text. It does not answer questions, run tasks, or analyze your project. Each dictation is an independent request, and the prompt explicitly instructs the model:

- This input is isolated from any prior conversation.
- The raw transcript is text to clean up, not a question to answer.
- Even if the input contains a question or a command, do not reply or execute it.
- Output the cleaned text only — no "Here's the cleaned version" preamble.

For example, if the user says "what features does this app still need", the correct output is:

```text
What features does this app still need?
```

…not a list of missing features.

Long-term reference rewrites are stored as `raw → polished → rule` triples and will be retrieved as similar-example references (never as conversation context) once a vector store is wired in. See [docs/polish-reference-corpus.md](docs/polish-reference-corpus.md) and [Examples/polish-reference-examples.sample.jsonl](Examples/polish-reference-examples.sample.jsonl).

## Dictionary

The dictionary handles your proper nouns, product names, names of people, and new words. Today it supports:

- Manually adding the correct spelling, a category, and notes. You do not need to maintain misspellings or context hints.
- Enabled entries are sent as Volcengine ASR `context.hotwords` so they are recognized correctly during transcription.
- Entries are also injected into the polish prompt: the model decides per sentence whether to substitute. If "Cloud" clearly refers to the AI product `Claude` in context, it is corrected; if it genuinely means cloud computing, it is left as is.
- The app auto-learns candidate corrections such as `Claude`, `ChatGPT`, and `OpenLess` from your history and offers them later.

The main window is organized as Home / History / Dictionary / Settings. The Dictionary tab opens a separate editor window when you click "New". The Home tab shows total dictation time, total characters, average characters per minute, estimated time saved, and dictionary participation statistics.

## Architecture

The active implementation is Tauri 2 (`openless-all/app/`). Releases are split into two channels: **Stable** (`v<v>-tauri` tag, auto-updated for all users) and **Beta** (`v<v>-beta-tauri` tag, GitHub pre-release, manually downloaded by opt-in users). Signed updater artifacts are produced by CI on every release tag.

**Tauri backend (Rust)** — each module depends only on `types.rs`:

```
types.rs         Pure value types: DictationSession, PolishMode, HotkeyBinding, errors
hotkey.rs        Global hotkey (CGEventTap on macOS, WH_KEYBOARD_LL on Windows, rdev on Linux)
recorder.rs      Mic → 16 kHz mono Int16 PCM, RMS callback
asr/             Volcengine streaming ASR (WebSocket) + Whisper HTTP
polish.rs        OpenAI-compatible chat completions (Ark / DeepSeek / etc.)
insertion.rs     AX focused-element → clipboard + Cmd+V → copy-only fallback
persistence.rs   History / preferences / vocab JSON + platform credential vault
permissions.rs   TCC checks (Accessibility / Microphone)
coordinator.rs   State machine: Idle → Starting → Listening → Processing
commands.rs      Tauri IPC surface
```

**React frontend (`src/`)** — state via Recoil atoms (`pages/_atoms.tsx`); hotkey capability and binding via `HotkeySettingsContext`; all backend calls go through `lib/ipc.ts`.

The dictation pipeline: `hotkey edge → Recorder.start + ASR.openSession → [audio frames] → hotkey edge → Recorder.stop + ASR.sendLastFrame → Polish → Insert → History.save`.

See [CLAUDE.md](CLAUDE.md) for invariants and module-wiring rules.

## Roadmap

Planned but not yet shipped:

- Dictation translation mode: hold a separate hotkey, speak in your language, insert in the target language ([#43](../../issues/43)).
- Cross-session style memory: polish learns the user's tone over time ([#46](../../issues/46)).
- Snippets (no UI or trigger logic yet).
- History enhancements: copy button, search, re-polish, re-insert.
- A "Paste last result" hotkey.

## Maintainer release checklist

OpenLess ships two release channels. The branch name equals the channel name (see [Contributing workflow](#contributing-workflow)).

### Common prep (both channels)

- Bump the version in **all five** files: `package.json`, `package-lock.json` (root + nested entry under `packages.""`), `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `Cargo.lock` (look for the `name = "openless"` block). CI's `Verify version sync` step will otherwise fail the build.
- Run `INSTALL=0 ./scripts/build-mac.sh` and confirm the `.app` launches.
- Smoke-test on a clean machine: permission flow, hotkey, recording, ASR, polish, insertion, and clipboard fallback.
- Confirm that `TAURI_SIGNING_PRIVATE_KEY` and (for macOS) the Apple signing/notarization secrets are set on the repo.

### Beta channel — `v<v>-beta-tauri`

1. Land changes onto the `beta` branch via PR review.
2. Push the tag **on `beta`**: `git tag v<v>-beta-tauri && git push origin v<v>-beta-tauri`.
3. CI tags the GitHub Release as `Pre-release` and uploads only `latest-{tgt}-{arch}-beta.json` updater manifests. The Stable users' `releases/latest` redirect is unaffected.
4. Announce in the appropriate channel (issue thread, QQ group) that opt-in Beta users can grab it from Settings → About → Join Beta channel.

### Stable channel — `v<v>-tauri`

1. Merge `beta → main` after the Beta release has soaked sufficiently (or run a final two-platform smoke build directly).
2. Push the tag **on `main`**: `git tag v<v>-tauri && git push origin v<v>-tauri`.
3. CI publishes a normal GitHub Release and uploads `latest-{tgt}-{arch}.json` (no `-beta` suffix). All Stable users receive the update through the in-app updater.

### Post-release verification (always run)

Run the 5-step checklist in [`CLAUDE.md` → Branch & release-channel workflow → Channel distribution](CLAUDE.md): page status (pre-release flag), asset-filename channel correctness, Stable user flow, Beta opt-in flow, and raw endpoint sanity.

## Acknowledgements

OpenLess sincerely thanks its sponsors, developers and contributors, and the broader LinuxDo community.

We appreciate our sponsors for making sustained project work possible, and we thank our developers and contributors for building, reviewing, and improving OpenLess.

OpenLess also recognizes and appreciates the LinuxDo community for its open, practical, and developer-friendly atmosphere. Many of the ideas, discussions, and early feedback around OpenLess were inspired by the broader open-source spirit that LinuxDo represents.

This acknowledgement does not imply official endorsement or affiliation.

## License

OpenLess is released under the [MIT License](LICENSE).
