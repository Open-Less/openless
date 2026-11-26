# OpenLess User Guide

[English](USAGE.md) | [简体中文](USAGE.zh.md)

> Online documentation is continuously updated at **<https://openless.top/en/docs/>** (Chinese: **<https://openless.top/zh/docs/>**). Use this guide as the in-repo reference for installation, permissions, daily dictation, and troubleshooting.

## Installation

See also the website download & installation guide: <https://openless.top/en/docs/installation/>.

### macOS (14 Sonoma+)

1. Download `OpenLess_<version>_aarch64.dmg` (Apple Silicon) or `OpenLess_<version>_x64.dmg` (Intel) from [GitHub Releases](https://github.com/Open-Less/openless/releases/latest).
2. Open the DMG and drag **OpenLess.app** into **Applications**.
3. Launch OpenLess from Applications.

You can also install the Stable release via Homebrew (optional update reminder setup in [docs/guides/homebrew.md](docs/guides/homebrew.md)):

```bash
brew tap Open-Less/openless https://github.com/Open-Less/openless
brew install --cask openless
brew update && brew upgrade openless
```

Published releases follow the signing and notarization checks in [RELEASING.md](RELEASING.md). Separately distributed dev builds may use ad-hoc signing.

### Windows (10+)

1. Download `OpenLess_<version>_x64-setup.exe` or `OpenLess_<version>_x64_en-US.msi` from [GitHub Releases](https://github.com/Open-Less/openless/releases/latest).
2. Run the installer and follow the prompts.
3. Launch OpenLess from the Start menu.

### Linux

Linux runs on a native `egui` host with no Tauri or WebKitGTK dependency.

1. Linux release packaging is currently paused pending on-device acceptance; build from source using [openless-all/README.md#shared-backend-and-linux-host](openless-all/README.md#shared-backend-and-linux-host).
2. Requires glibc ≥ 2.39 (Ubuntu 24.04 or equivalent).
3. Global hotkeys are handled by the `fcitx5` plugin — enable the OpenLess addon in `fcitx5` and bind a trigger key. Linux has no AppImage and no in-app updater.

### Android (8.0+)

1. Download `OpenLess_<version>_arm64-v8a.apk` from [GitHub Releases](https://github.com/Open-Less/openless/releases/latest) (use `armeabi-v7a` for older 32-bit devices).
2. Allow installing unknown apps and install the APK.
3. Grant microphone permission on first recording. To dictate into other apps, enable the OpenLess keyboard in system input-method settings and switch to it.
4. Check for updates under **Settings → About & updates**.

### iOS (16+)

1. Pre-signed IPAs are not distributed. Build from `openless-all/ios/` with Xcode and sign with your personal Apple team.
2. Follow [openless-all/ios/README.md](openless-all/ios/README.md) for build, signing, and keyboard setup.

### HarmonyOS (adapting)

1. Download the unsigned `OpenLess_harmony_arm64.hap` from the release assets.
2. Sign it for your device before installing (devices reject unsigned HAPs; debug certificate must match bundle `openless.hm.com` and profile must include your device). DevEco Studio automatic signing handles this, or place signing files in `~/.local/share/harmony/signing/local-test` (`OPENLESS_HARMONY_SIGNING_DIR`).
3. Install with DevEco Studio or `hdc`. Open the main app once to configure speech services before using the input method. See [openless-all/app/harmony/README.md](openless-all/app/harmony/README.md).

---

## First-Time Setup

### System Permissions

- **macOS**: Open **System Settings → Privacy & Security** and grant **Microphone** and **Accessibility**. After granting Accessibility, **quit OpenLess completely and relaunch** so global hotkeys take effect.
- **Windows**: Grant **Microphone** permission when prompted, then check **Settings → Permissions & data** to confirm the global hotkey listener is active.
- **Android**: Grant **Microphone** on first use and enable **OpenLess Keyboard** under System Settings → Language & input. Accessibility is optional and only needed if you enable local vocabulary learning inside supported editors.
- **iOS**: Grant **Microphone** (and Speech Recognition if used). Enable **Settings → General → Keyboard → Keyboards → Add New Keyboard → OpenLess** and turn on **Allow Full Access** for shared settings and voice control.
- **HarmonyOS**: Grant **Microphone** and **Network**, then enable **OpenLess InputMethod** in system input-method settings (no accessibility permission required).

### Configure Speech & Model Services

Open **Settings → AI Services & Models** and choose one of three dictation pipelines. Complete step-by-step provider instructions live on the official docs:

- **Official Setup Guide**: <https://openless.top/en/docs/setup/> (Chinese: <https://openless.top/zh/docs/setup/>)
- **Quick Reference**: [docs/guides/speech-services.md](docs/guides/speech-services.md)

The three pipelines are:
1. **Traditional (ASR + optional LLM)**: Transcribe via an ASR provider (Volcengine, Alibaba Cloud Bailian, iFlytek RTASR, Tencent Hunyuan, OpenAI-compatible, or local models), then optionally polish/translate with an LLM provider.
2. **Multimodal (single model)**: Send audio directly to an audio-capable model (Google Gemini, OpenAI Audio, Qwen-Omni) for one-step transcription and rewriting.
3. **DeepSeek Suite**: Use an official DeepSeek API key for voice dictation, Ask, web search, and spoken readback.

All API keys are stored in the OS credential vault (service = `com.openless.app`: macOS Keychain, Windows Credential Manager, Linux Secret Service, iOS Keychain) and are never saved in plaintext config files.

---

## Daily Usage

See also <https://openless.top/en/docs/voice-input/> for live examples and polish tips.

### Recording & Continuous Dictation

- **Start / Stop**: Press the global hotkey (defaults: macOS right Option, Windows right Control; Linux via `fcitx5`; mobile via app or keyboard). Both **toggle** and **push-to-talk** modes are supported, plus `MediaPlayPause` for wired headset buttons.
- **Continuous background dictation**: As soon as you stop recording one segment, it moves to an asynchronous background queue while freezing its target app and cursor context — you can start the next segment immediately, and finished results are inserted in order.
- **Cancel**: Press `Esc` at any time (recording, polishing, or insertion) to discard the current segment cleanly.
- **Safe insertion**: Text inserts straight into the focused input field; if an app blocks direct insertion, OpenLess automatically copies the result to your clipboard so nothing is lost.

### Output Modes, Style Packs & Marketplace

Switch modes in the main window, capsule, or Settings:

| Mode | Behavior |
| --- | --- |
| **Raw** | Outputs the ASR transcript directly without LLM rewriting |
| **Light Polish** | Removes filler words, fixes punctuation and self-corrections, keeps your original tone |
| **Structured (AI Prompt)** | Organizes spoken thoughts into structured context, constraints, and requests ready for ChatGPT / Claude / Cursor |
| **Formal** | Rewrites spoken input into clean, professional written prose |

You can also assign **Style Packs** (custom system prompts, hotkey switching, per-app automatic routing) and browse or install community packs from the in-app **Marketplace**. See reference polish examples in [docs/references/polish-reference-examples.sample.jsonl](docs/references/polish-reference-examples.sample.jsonl).

### Ask & Selection Rewrite

Desktop hosts provide a standalone **Ask** window and selected-text rewrite workflow. Generated replies stay inside the Ask window until the original target input can be safely verified for writeback, or can be copied manually.

### Dictionary & Optional Cursor Context

- **Personal Dictionary**: Add names, domain terms, and acronyms under **Dictionary**. Enabled entries are sent as ASR hotwords (where supported) and injected into the polish prompt.
- **Learn from Corrections (experimental, opt-in)**: Under **Settings → Experiments & Extensions**, OpenLess can observe post-insertion edits in supported editors and suggest new terms for explicit confirmation (never silently added, never uploaded).
- **Cursor Context (opt-in, off by default)**: Under **Settings → Privacy → Data storage**, OpenLess can read a few hundred characters around the caret (macOS / Windows UIA / Android IME `InputConnection`) to disambiguate homophones. Password fields, Secure Input, password managers, and terminals are always excluded.

### History, Remote Input & Encrypted Cloud Sync

- **History**: Inspect every recording's raw transcript and polished result in **History**.
- **Mobile Remote Input**: Turn on Remote Input on desktop, verify the local HTTPS certificate SHA-256 fingerprint on your phone, enter the pairing code, and dictate from your phone straight to your computer's cursor. See [docs/guides/remote-input.md](docs/guides/remote-input.md).
- **Encrypted Cloud Sync**: Sign in with GitHub under **Settings → Cloud Services**, review the scope disclosure, and set a sync passphrase. Settings, registered service keys, and history text are end-to-end encrypted locally before upload; keep your sync passphrase safe.

### Changing Hotkeys

Open **Settings → Shortcuts**. macOS supports right-side modifier keys (Option / Control / Command / Shift); Windows supports left/right modifiers and mouse side buttons (Mouse4 / Mouse5); Linux binds hotkeys inside `fcitx5`.

---

## Troubleshooting

**Q: Hotkey does nothing?**  
macOS: Confirm Accessibility permission is enabled, then **quit and relaunch** OpenLess.  
Windows: Check **Settings → Permissions & data** to confirm the listener is running.

**Q: Transcript is empty or placeholder text?**  
Verify your selected ASR / Multimodal / DeepSeek credentials in **Settings → AI Services & Models** and run a test check (see <https://openless.top/en/docs/setup/>).

**Q: Text was copied to the clipboard instead of inserted?**  
When a focused field restricts accessibility or IME injection, OpenLess falls back to the clipboard automatically — press `Cmd+V` / `Ctrl+V` to paste.

**Q: In Windows fullscreen games (e.g. Minecraft), the capsule does not appear or text is not typed?**  
This is a Windows OS limitation ([issue #457](https://github.com/Open-Less/openless/issues/457)):
- **Exclusive fullscreen**: Standard windows cannot render over exclusive fullscreen DirectX/OpenGL apps. Switch the game to **Borderless Windowed Fullscreen**.
- **Privilege mismatch (UIPI)**: If the game runs as Administrator while OpenLess does not, Windows blocks keyboard events. Run both at the same privilege level.
- **Chat box closed**: Open the in-game chat box first (e.g. press `T` in Minecraft) before dictating.

**Q: How do I use local ASR so audio never leaves my machine?**  
Download a local model (Windows: **Settings → AI & Models → Local ASR Models**; macOS: **Settings → Experiments & Extensions → Experimental Local Speech Models**), then enable the local provider in **ASR Providers** and **drag it to the very top** of the list. Requests always use the first enabled provider (switch back to Traditional mode if you were in Multimodal mode).

**Q: Do Linux, iOS, or HarmonyOS support auto-updates?**  
No. Linux is updated via rebuilt source or manually installed packages; iOS is rebuilt and signed from source; HarmonyOS currently requires installing a new signed HAP.

---

## Community

- **QQ Group 1**: `1078960553` (full)
- **QQ Group 2**: `1095794054`
