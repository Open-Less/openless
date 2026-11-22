# OpenLess User Guide

[English](USAGE.md) | [简体中文](USAGE.zh.md)

## Installation

### macOS

1. Download `OpenLess_<version>_aarch64.dmg` from [Releases](https://github.com/Open-Less/openless/releases/latest).
2. Open the dmg and drag OpenLess.app into the Applications folder.
3. Double-click to launch.

### Windows

1. Download `OpenLess_<version>_x64-setup.exe` from [Releases](https://github.com/Open-Less/openless/releases/latest).
2. Run the installer and follow the prompts.
3. Launch OpenLess from the Start menu.

### Linux

Linux runs on a native egui host and does not depend on Tauri or WebKitGTK.

1. No package is shipped with Releases yet: Linux packaging stays paused until the L01–L12 on-device acceptance is complete, so build from source for now (see the build sections in the [README](README.md)).
2. Requires glibc ≥ 2.39 (Ubuntu 24.04 or an equivalent distribution).
3. The global hotkey is provided by the fcitx5 plugin: enable the OpenLess addon in fcitx5 and bind a key. There is no in-app updater and no AppImage.

### Android

1. Download the APK for your phone, `OpenLess_<version>_arm64-v8a.apk`, from [Releases](https://github.com/Open-Less/openless/releases/latest) (use `armeabi-v7a` for older 32-bit devices).
2. Allow "install unknown apps" and install it.
3. Grant microphone permission on the first recording. To insert text with the keyboard, enable the OpenLess keyboard in system settings and set it as the current input method.
4. In-app updates live under Settings → About & updates.

### iOS

1. There is no downloadable IPA. Build it from the [`openless-all/ios/`](openless-all/ios/README.md) sources with Xcode and sign it with a personal team.
2. See the [iOS guide](openless-all/ios/README.md) for the full build, install, and keyboard-enable steps.

### HarmonyOS (adapting)

1. Download the unsigned `OpenLess_harmony_arm64.hap` from the same Release.
2. Sign it for your own device before installing. A phone or tablet rejects an unsigned HAP: the debug certificate has to match the bundle name `openless.hm.com`, and the debug Profile has to include the target device — a Profile issued for another bundle name cannot be reused. DevEco Studio's automatic signing (Huawei account) does both for you; when building from the CLI, put the certificate and Profile in the local signing directory (default `~/.local/share/harmony/signing/local-test`, override with `OPENLESS_HARMONY_SIGNING_DIR`) and the build emits `entry-default-signed.hap`.
3. Install the signed HAP with DevEco Studio or `hdc`; AppGallery packages are submitted manually.
4. Open the main app once to finish provider configuration before the keyboard can reuse the voice settings. There is no update flow yet. See [`harmony/README.md`](openless-all/app/harmony/README.md) for build and signing.

---

## First-time setup

### macOS permissions

After the first launch, grant the following in System Settings → Privacy & Security:

1. **Microphone** — lets OpenLess record.
2. **Accessibility** — lets OpenLess read and write the focused text field. After granting it you must **quit OpenLess completely and relaunch** before the hotkey works.

### Windows permissions

1. Grant **microphone** permission when prompted.
2. Open OpenLess → Settings → Permissions & data and confirm the global hotkey listener shows as running.

### Android permissions

1. **Microphone** — granted on the first recording.
2. **Input method** — enable the OpenLess keyboard in System Settings → Language & input and set it as the current IME before it can type into other apps.
3. **Optional: accessibility** — "learn from corrections" reads text in supported editors; without it you can confirm words manually with "Remember a word" in the history detail view.

### iOS permissions

1. Grant **microphone** and **speech recognition** when prompted.
2. **Keyboard** — Settings → General → Keyboard → Keyboards → Add New Keyboard → OpenLess.
3. **Full Access** — enable it for shared settings and voice control; basic typing does not need it.

### HarmonyOS permissions

1. **Microphone** and **network** permissions use the system declarations.
2. **Input method** — enable "OpenLess InputMethod" in system input-method settings, then switch to it in a text field; voice results are written through the system-assigned input session, without accessibility.
3. When the IME is not enabled, copy and paste manually as a fallback.

### Entering credentials

If you are new to Volcengine ASR, start with the illustrated walkthrough:  
[OpenLess Volcengine ASR setup](docs/volcengine-setup.md)

For iFlytek real-time transcription, see [OpenLess iFlytek ASR setup](docs/xfyun-asr.md) (AppID + API Key, Settings → AI & models → Speech recognition channels → iFlytek real-time transcription).

Open OpenLess → **Settings** and fill in:

| Field | Description |
| --- | --- |
| Volcengine App ID | Application ID of the speech recognition service |
| Volcengine Access Token | Access credential for speech recognition |
| Volcengine Resource ID | Speech recognition resource ID |
| Ark API Key | API key for text polishing |
| Ark Model ID | Model ID to use (for example `doubao-pro-32k`) |
| Ark Endpoint | Endpoint URL, default `https://ark.cn-beijing.volces.com/api/v3/chat/completions` |

Settings take effect immediately after saving; no restart is needed.

Volcengine ASR + Ark is the shortest path. Under Settings → AI & models → Speech recognition channels / Text processing channels you can also pick iFlytek real-time ASR, Tencent Hunyuan, Alibaba Bailian, StepFun, Zhipu GLM, Xiaomi MiMo, OpenAI-compatible endpoints, and nearly thirty other ASR providers plus more than twenty polish providers (the full list is in the [README](README.md) "feature status" section). Credentials live in the OS credential vault (service = `com.openless.app`): macOS Keychain, Windows Credential Manager, Linux keyring. They are never written to disk in plaintext.

---

## Basic usage

### Start recording

Press the global hotkey (desktop defaults: right Option on macOS, right Control on Windows; on Linux the hotkey comes from the fcitx5 plugin and must be configured there; Android, iOS, and HarmonyOS trigger it in-app or from the keyboard).  
A status capsule appears at the screen edge showing "recording".

### Stop recording

Press the same hotkey again. OpenLess will:

1. Stop recording and send the audio for transcription.
2. Polish the transcript according to the current output mode.
3. Insert the polished text into the focused field (falling back to the clipboard if insertion fails).

### Cancel recording

Press `Esc` while recording: the current take is discarded and nothing is inserted.

---

## Output modes

Switch the mode from the capsule in the main window or under Settings:

| Mode | Description |
| --- | --- |
| Raw | Outputs the transcript verbatim, with no changes |
| Light polish | Fixes filler words, punctuation, and obvious typos while keeping the original meaning |
| Structured (AI prompt mode) | Turns speech into a structured, constrained, context-rich prompt ready for ChatGPT / Claude / Cursor |
| Formal | Converts speech into formal written prose |

---

## Dictionary

The dictionary improves recognition of specific words (product names, people, jargon).

1. Open the main window → **Dictionary**.
2. Click "New" and fill in the correct spelling, category, and note.
3. Once enabled, entries are injected as hot words into ASR and help semantic decisions during polishing.

---

## History

Main window → **History** lists every recording, including the raw transcript and the polished result.

---

## Changing the hotkey

Main window → **Settings → Shortcuts**, then pick the trigger key.  
macOS supports the right-side modifiers (Option / Control / Command / Shift); Windows supports left and right modifiers plus mouse side buttons (Mouse4 / Mouse5).

Linux handles the global hotkey through the fcitx5 plugin and its configuration; Android, iOS, and HarmonyOS do not register system-wide hotkeys and use the in-app or keyboard voice button instead.

---

## FAQ

**Q: The hotkey does nothing?**  
macOS: make sure Accessibility is granted and that you relaunched OpenLess afterwards.  
Windows: check the listener status under Settings → Permissions & data.

**Q: The result is empty or placeholder text?**  
Check that the Volcengine ASR credentials are correct. Recognition only works once they are.

**Q: The text was only copied to the clipboard instead of being inserted?**  
When the target field does not support accessibility writes (some hardened apps), OpenLess falls back to copying to the clipboard; paste it manually.

**Q: While playing Minecraft or another fullscreen game on Windows, the OpenLess capsule does not appear / characters are not typed?**  
This is a **Windows-level limitation** that OpenLess cannot work around (see [issue #457](https://github.com/Open-Less/openless/issues/457)):

- **Exclusive fullscreen**: standard app windows (including the OpenLess capsule) are **not drawn over exclusive-fullscreen DirectX/OpenGL apps**. Switch the game to **borderless windowed fullscreen**. In Minecraft: Video Settings → turn Fullscreen off (keeping the window maximized is fine).
- **Mismatched admin rights (UIPI)**: if the game runs elevated and OpenLess does not, Windows blocks OpenLess from receiving the game's foreground keystrokes and the hotkey never fires. Align the two (run both elevated or both normally).
- **Game chat not open**: characters are typed by simulating keyboard events. In Minecraft you must press `T` to open chat before OpenLess's input lands there.

macOS has no exclusive fullscreen (every "fullscreen" is a bordered window on a Space), so the limitation does not apply.

**Q: The polished result is not what I expected?**  
Try another output mode, or add the relevant proper nouns to the dictionary.

**Q: Is there auto-update on Linux?**  
No. Linux gets deb/rpm packages (after on-device acceptance) and source builds; there is no AppImage or in-app updater, so upgrades mean installing a new package manually.

**Q: How do I update HarmonyOS or iOS?**  
HarmonyOS has no update flow yet — reinstall a new HAP. iOS means rebuilding from source and re-signing. Neither shows in-app update prompts.

**Q: How do I use local recognition so audio is not uploaded?**  
Local models never take over the channel you already use: you still have to make the local channel the first enabled one. While that local channel is serving requests, audio stays on the machine; if a cloud channel is first, it is uploaded as usual.

On Windows, open Settings → AI & models → Local ASR models, download a model, then add and enable the matching local channel from the speech recognition channel list: Local Whisper (Foundry Local), Local sherpa-onnx (Experimental), or Local Whisper (batch). On macOS, open Settings → Experiments & extensions → Experimental local speech models and enable TextStream, SenseVoice Small, or Embedding Gemma; models are downloaded on demand and are not bundled with the installer, and the app then offers to open the channel list.

In both cases finish in Settings → AI & models → Speech recognition channels: enable the channel and drag it to the top. Requests use the first enabled channel, so until the local one is first, audio still goes to your existing cloud channel. The macOS experimental models run in the traditional pipeline (ASR + LLM); if the pipeline mode is multimodal, switch back first.

---

## Community and support

Join the QQ user groups to report issues or exchange tips:

**QQ Group 1: 1078960553 (full)**  
**QQ Group 2: 1095794054**
