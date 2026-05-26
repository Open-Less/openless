# Windows sherpa-onnx 本地 ASR 实施规划

> 状态：执行中；离线 batch 路径已接入，流式 ASR 代码路径已接入但待真实模型验收
> 更新日期：2026-05-26
> 范围：仅 Windows；不替换 macOS `local-qwen3`；不替换 Windows `foundry-local-whisper`

本文记录 `sherpa-onnx-local` 在 OpenLess 中的定位、当前完成度和后续工作。当前实现已经不是
M1 纯骨架：Windows 下已经接入 `sherpa-onnx` crate、`OfflineRecognizer` 和 `OnlineRecognizer`。offline 模型按“录音结束后整段识别”的 batch 模式工作；online 模型已接入 Recorder 分块解码和胶囊 partial 显示，但仍需真实模型 spike、性能数据和打包验收后才能判断是否发布。

---

## 1. 当前结论

- **已支持**：Windows 实验 provider `sherpa-onnx-local`、模型选择、模型下载、模型准备、离线 batch 转写、online streaming 模型 catalog、online recognizer 缓存、Recorder 分块驱动和胶囊 partial 显示。
- **未完成验收**：真实 online 模型 spike、Windows 真机 batch/streaming 性能数据、路径/网络/损坏模型故障注入、MSI / NSIS 安装验证。
- **当前阶段**：M1 已完成；M2 离线推理已接入；M3 模型管理和多模型已基本落地；M4 的离线自动化覆盖和运行时诊断已补强，仍需 Windows 真机、打包和真实模型稳定性验证；M5 后端/前端主链路已接入，仍需模型 spike 和发布判断。
- **用户侧定位**：继续作为 Windows 高级页里的实验选项，不提升为默认 provider。

---

## 2. 目标与边界

### 目标

- Windows 新增本地 ASR provider：`sherpa-onnx-local`。
- 复用现有听写主链路：Recorder / Coordinator / polish / insert / history。
- 中文为主，中英混合可用。
- 第一阶段 batch，第二阶段流式。
- 与 `foundry-local-whisper` 并存并可切换。

### 非目标

- 不替换 macOS `local-qwen3`。
- 不替换 Windows `foundry-local-whisper`。
- 不做 Linux 支持。
- 不做语者分离、长会议转写、字幕导出。
- 不做云端模型和模型自训。
- 不做多 provider 自动选择。

### 工程边界

- 不改 Coordinator 的 phase enum / hotkey 流程。
- 不改 polish / insertion / history 的主语义。
- `sherpa_runtime.rs` 不感知 UI / Coordinator / Recorder。
- `sherpa_provider.rs` 通过 `AudioConsumer` 接收 PCM；offline 模型缓存整段 PCM，online 模型把 chunk 交给独立 worker。
- batch API 保持不变；online API 与 offline recognizer 缓存分离，不用 OfflineRecognizer 伪造流式。

---

## 3. 当前架构

### 后端模块

```text
openless-all/app/src-tauri/src/asr/local/
  mod.rs                 # 本地 ASR 入口，导出 SherpaOnnxAsr / SherpaOnnxRuntime
  sherpa.rs              # provider id、模型 catalog、模型文件清单、事件 payload
  sherpa_runtime.rs      # sherpa-onnx OfflineRecognizer / OnlineRecognizer 加载、缓存、转写、释放
  sherpa_provider.rs     # AudioConsumer + offline PCM buffer / online worker + transcribe()
  sherpa_download.rs     # 模型远端信息、下载、取消、校验、解包
```

其他集成点：

- `openless-all/app/src-tauri/src/coordinator.rs`
  - 持有 `Arc<SherpaOnnxRuntime>`。
  - `ActiveAsr::SherpaOnnxLocal(Arc<SherpaOnnxAsr>)`。
- `openless-all/app/src-tauri/src/coordinator/dictation.rs`
  - `begin_session` 创建 `SherpaOnnxAsr` 并交给 Recorder。
  - `end_session` 调 `local.transcribe(...)`，之后进入现有 polish / insert / history 收尾。
- `openless-all/app/src-tauri/src/commands.rs`
  - 暴露 status / catalog / prepare / release / download / cancel / delete / reveal / set model / set language hint。
- `openless-all/app/src-tauri/src/lib.rs`
  - 仅 Windows 注册 sherpa Tauri commands。
- `openless-all/app/src-tauri/src/types.rs`
  - 持久化 `sherpa_onnx_model`、`sherpa_onnx_language_hint`、`sherpa_onnx_keep_loaded_secs`。
- `openless-all/app/src-tauri/Cargo.toml`
  - Windows 依赖：`sherpa-onnx = { version = "1.13.2", default-features = false, features = ["static"] }`。

### 前端模块

- `openless-all/app/src/lib/localAsr.ts`
  - sherpa 模型类型、命令封装、mock 数据。
- `openless-all/app/src/pages/LocalAsr.tsx`
  - 模型选择、准备、下载、删除、目录打开、进度事件。
- `openless-all/app/src/pages/settings/LocalModelSection.tsx`
  - 高级页本地模型开关，包含 `sherpa-onnx-local`。
- `openless-all/app/src/i18n/*.ts`
  - 多语言文案。

---

## 4. 模型策略

### 当前 catalog

| Alias | 模型 | 模式 | 用途 |
|---|---|---|---|
| `sense-voice-small-zh` | SenseVoice Small zh/en/ja/ko/yue | Offline | 默认模型，中文和常见多语言 |
| `paraformer-zh` | Paraformer zh | Offline | 中文专用 |
| `whisper-small-multi` | Whisper Small multilingual | Offline | 通用 fallback / 与 Whisper 体验对齐 |
| `qwen3-asr-0.6b-int8` | Qwen3-ASR 0.6B INT8 | Offline | 多语言和长上下文实验档 |
| `zipformer-bilingual-zh-en-streaming` | Zipformer Streaming bilingual zh/en | Online | 实验流式模型，录音中输出 partial |

offline batch 路径会显式拒绝 online Zipformer；online session API 也会拒绝 offline 模型，避免两种模型模式混用。

### 模型分发

- 模型不打进安装包。
- 默认存放路径：

```text
%APPDATA%\OpenLess\models\sherpa-onnx\<alias>\
```

- HuggingFace 模型通过 tree API 获取文件大小和 LFS SHA-256。
- Qwen3-ASR 通过 GitHub release archive 下载并解包。
- 下载进度通过 `sherpa-onnx-asr-download-progress` 事件上报。
- 准备进度通过 `sherpa-onnx-asr-prepare-progress` 事件上报。

---

## 5. Batch 转写链路

当前 sherpa 路径是完整 batch 模式：

1. `begin_session` 根据 prefs 选择模型 alias 和 language hint。
2. 创建 `SherpaOnnxAsr`。
3. Recorder 持续调用 `consume_pcm_chunk(&[u8])`，provider 只缓存 PCM。
4. `end_session` 调 `SherpaOnnxAsr::transcribe(timeout)`。
5. runtime `ensure_loaded(alias)`，必要时加载 `OfflineRecognizer`。
6. `pcm_s16le_to_f32` 转换 16kHz mono s16le PCM。
7. `stream.accept_waveform(16_000, &samples)`。
8. `OfflineRecognizer::decode(&stream)`。
9. `stream.get_result().text` 转成 `RawTranscript`。
10. 进入现有 polish / insert / history。

这个链路不会在录音过程中输出 partial，也不会把中间 token 发给前端胶囊。选择 online 模型时走独立 streaming worker，不改变上面的 batch API 语义。

### Streaming 转写链路

当前 sherpa online 路径已接入主链路，但待真实模型验收：

1. `begin_session` 根据 prefs 选择 online alias。
2. `SherpaOnnxAsr::new_for_model(...)` 判断 alias mode，online 模型创建 `SherpaOnlineSession`。
3. runtime `ensure_loaded(alias)` 加载并缓存 `OnlineRecognizer`，与 `OfflineRecognizer` 分离。
4. Recorder 持续调用 `consume_pcm_chunk(&[u8])`，provider 把 PCM 发送到 online worker。
5. worker 调 `OnlineStream::accept_waveform(16_000, chunk)`，在 `is_ready()` 时 decode。
6. partial delta 默认通过 `local-asr-token` 发给前端胶囊。
7. endpoint 时记录 final segment；停止录音后 `transcribe(timeout)` flush stream 并返回最终 `RawTranscript`。
8. final transcript 继续进入现有 polish / insert / history；ASR partial 不直接写入光标。
9. cancel 会停止当前 online worker、丢弃 partial、递增 cancel generation。

---

## 6. 命令与用户操作

当前 Windows Tauri commands：

- `sherpa_onnx_asr_status`
- `sherpa_onnx_asr_catalog`
- `sherpa_onnx_asr_fetch_remote_info`
- `sherpa_onnx_asr_download_model`
- `sherpa_onnx_asr_cancel_download`
- `sherpa_onnx_asr_set_model`
- `sherpa_onnx_asr_set_language_hint`
- `sherpa_onnx_asr_prepare`
- `sherpa_onnx_asr_cancel_prepare`
- `sherpa_onnx_asr_release`
- `sherpa_onnx_asr_model_dir`
- `sherpa_onnx_asr_delete_model`
- `sherpa_onnx_asr_reveal_model_dir`

前端已有能力：

- 查看 catalog 和 runtime 状态。
- 选择模型。
- 下载、取消下载、准备、取消准备。
- 删除模型。
- 打开模型目录。
- 设置为 active ASR provider。
- 设置 language hint。

---

## 7. 里程碑状态

### M1 Provider 骨架：已完成

- `sherpa.rs` / `sherpa_provider.rs` / `sherpa_runtime.rs` / `sherpa_download.rs` 已存在。
- `ActiveAsr::SherpaOnnxLocal` 已接入。
- Tauri commands 已注册。
- 前端 toggle、LocalAsr 管理页和 i18n 已接入。

### M2 Batch 推理可用：已接入，待真机验收

- 已接 `sherpa-onnx` crate。
- 已加载 `OfflineRecognizer`。
- 已支持 PCM -> text。
- 已支持 SenseVoice / Paraformer / Whisper / Qwen3-ASR 的 offline config。
- 待补：Windows 真机 smoke test 记录、质量对比、超时策略实测数据。

### M3 模型管理 + 多模型：基本完成，待补强

- 已有模型 catalog。
- 已有下载、取消、删除、目录打开。
- 已有 HuggingFace 镜像和 GitHub release archive 路径。
- 已有模型切换，不需要重启。
- 待补：下载失败重试体验、损坏模型恢复、校验失败文案、不同网络环境实测。

### M4 性能与稳定性：进行中

- 首次加载耗时和内存占用需要实测。
- 30s+ 长录音稳定性需要实测。
- 取消语义、缺文件、校验失败、release archive 解包失败/成功、下载取消 flag 已有离线单测。
- runtime status 已暴露最近一次 prepare / transcribe / audio 耗时和最近错误，前端 LocalAsr 页可见。
- 模型损坏、真实网络中断、路径含中文、路径含空格需要系统测试。
- Windows MSI / NSIS 打包需要完整验证。
- 代码内 “M1 骨架 / 返回空串” 的过期 sherpa 注释已清理。

### M5 流式 ASR：代码链路已接入，待真实模型验收

已接入：

- 已接入 `OnlineRecognizer`。
- 已增加 `SherpaMode::Online` 模型 catalog，当前 online alias 为 `zipformer-bilingual-zh-en-streaming`。
- 已新增 online session/provider 路径，保留 batch `transcribe()` 语义。
- 已在 Recorder 分块输入阶段驱动 online stream，并通过 `local-asr-token` 驱动胶囊 partial。
- 已让 Local ASR 页面区分 offline batch / online streaming 模型。

暂不做：

- 不把 OfflineRecognizer 包装成伪流式。
- 不为了流式重构整个 ASR trait 体系。
- 不默认开启流式；先作为实验开关，若 spike 数据不合格则隐藏或继续保持开发态。

已确认：

- `sherpa-onnx` 1.13.2 Rust crate 暴露 `OnlineRecognizer`、`OnlineRecognizerConfig`、`OnlineStream`、`is_ready()`、`decode()`、`get_result()`、`is_endpoint()`。
- 上游存在 streaming Zipformer 中文/英文模型：`csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20`。
- 已新增独立 spike example：`openless-all/app/src-tauri/examples/sherpa_online_spike.rs`。
- 待完成：真实模型下载、独立 spike 音频测试、RTF/内存记录、长句重复和取消行为验收。

### M6 发布：未开始

- 保持高级页实验入口。
- 收集真实用户反馈。
- 满足质量、稳定性、打包和性能门槛后，再决定是否提升为 Windows 默认。

---

## 8. 剩余工作执行计划

下面按实际依赖顺序推进。M4 和 M6 是把当前 batch 能力做完整、可发实验版；M5 是流式 ASR 二阶段，不阻塞 batch 实验发布。

### P0 清理基线：已完成

目标：让代码注释、文档和当前实现一致，避免后续按旧 M1 判断。

改动范围：

- `openless-all/app/src-tauri/src/asr/local/mod.rs`
- `openless-all/app/src-tauri/src/asr/local/sherpa.rs`
- `openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs`
- `openless-all/app/src-tauri/src/asr/local/sherpa_runtime.rs`
- `openless-all/app/src-tauri/src/coordinator.rs`
- `openless-all/app/src-tauri/src/coordinator/dictation.rs`
- `openless-all/app/src-tauri/src/commands.rs`
- `openless-all/app/src-tauri/src/types.rs`

任务：

1. 全局清理 `M1 骨架`、`不接 sherpa-onnx crate`、`返回空串` 等过期注释。
2. 把注释统一改成“当前支持 Windows offline batch，online streaming 代码链路已接入但待真实模型验收”。
3. 保留非 Windows stub 的说明：非 Windows 仍不提供 sherpa 推理能力。
4. 不改运行逻辑，只改注释和必要的测试名。

退出条件：

- 已完成：`rg "M1 骨架|不接 sherpa-onnx crate|M1.*返回空串" openless-all/app/src-tauri/src` 不再命中过期 sherpa 描述。
- 已完成：文档、注释、UI 文案对当前能力的说法一致：offline batch 可用，online streaming 代码链路已接入但仍需真实模型验收。

### P1 M4-1：建立 Windows batch 验收基线

目标：先确认当前实现能在开发环境稳定跑完，不急着改功能。

验证入口：

```powershell
cd openless-all/app
npm run build                                  # 已通过：2026-05-26
cargo test --manifest-path src-tauri/Cargo.toml # 已通过：2026-05-26，368 tests
cargo test --manifest-path src-tauri/Cargo.toml sherpa -- --format terse # 已通过：2026-05-26，42 tests
npm run tauri build                            # 2026-05-26：release exe 已构建；Tauri MSI wrapper 仍在 light.exe 阶段退出 1
```

Windows 真机 smoke：

1. 打开高级页，启用 `sherpa-onnx-local`。
2. 下载 `sense-voice-small-zh`。
3. prepare 模型，确认 `runtimeReady=true`、`loadedModelId=sense-voice-small-zh`。
4. 录 3-5 秒中文短句，确认能进入 polish / insert / history。
5. 切换回 `foundry-local-whisper`，再切回 sherpa，确认状态一致。

需要记录：

- Windows 版本、CPU、内存。
- 模型 alias。
- 首次下载耗时。
- prepare 耗时。
- 录音时长。
- decode 耗时。
- 是否插入成功。
- 错误日志和 UI 文案。

退出条件：

- SenseVoice 在一台 Windows 真机上完成下载、prepare、短句转写、插入。
- Foundry 与 sherpa 来回切换不破坏 active provider。
- dev build、Rust tests 已通过；Tauri release exe 已生成。WiX 下载 blocker 已解除，手动 `light.exe` 可产出 MSI；Tauri MSI wrapper 仍需定位退出 1，NSIS 下载工具包超时仍是 blocker。

### P2 M4-2：模型矩阵补齐

目标：确认 catalog 中每个 offline 模型都不是“UI 可选但实际不可用”。

模型矩阵：

| 模型 | 必测项 | 失败处理 |
|---|---|---|
| `sense-voice-small-zh` | 默认模型，中文短句 / 30s 中文 | 必须修到可用 |
| `paraformer-zh` | 中文短句 / 中英混合 | 中文必须可用；英文弱只记录 |
| `whisper-small-multi` | 英文短句 / 中英混合 | 作为 fallback，必须可 prepare + decode |
| `qwen3-asr-0.6b-int8` | 下载解包 / prepare / 中文短句 | 若资源或性能问题明显，保留实验标记并明确文案 |

任务：

1. 逐个模型跑下载、prepare、转写、release、再次 prepare。
2. 校验 `required_files_for_alias` 与实际下载/解包后的文件结构一致。
3. 校验 `catalog_snapshot()` 的 `cached`、`downloadedBytes`、`fileSizeMb` 是否可信。
4. 检查 `delete_model()` 后 runtime 是否释放已加载模型，并且 UI 状态刷新。
5. 记录每个模型的最小可用机器配置和明显限制。

退出条件：

- 所有 catalog 模型都有一条明确结论：可用、受限可用、或暂时隐藏/禁用。
- 没有“可选后必然失败且无解释”的模型。
- 文案能解释 Qwen3 包体、下载源、性能风险。

### P3 M4-3：取消、错误和恢复：自动化覆盖已补强，待真机故障注入

目标：失败时不崩溃、不写乱码、不让用户以为内容被成功识别。

需要覆盖的路径：

- 录音中取消：`SherpaOnnxAsr::cancel()` 清 buffer，session 回到 Idle。
- prepare 中取消：`request_cancel_prepare()` 能让 prepare 返回 cancelled。
- decode 中取消：`cancel_generation` 命中后丢弃 transcript。
- 下载中取消：`SherpaDownloadManager::cancel()` 停止任务并上报 cancelled。
- 模型缺文件：`ensure_required_files()` 返回可读错误。
- 模型文件损坏：prepare 或 decode 失败后 UI 展示错误，不写入空成功记录。
- 无网络下载：fetch remote info 和 download 都有明确错误。
- 删除当前 loaded 模型：runtime 释放 handle，状态刷新。

任务：

1. 已完成：给 `sherpa_runtime.rs` / `sherpa_provider.rs` 补充能离线跑的单元测试，不依赖真实模型。
2. 已完成：`sherpa_download.rs` 已覆盖 release archive 完成进度、解包后字节统计、partial archive 进度、文件大小校验失败、SHA-256 校验失败、解包缺必需文件、解包成功移动文件、下载取消 flag。
3. 人工制造缺文件、损坏文件、下载中断场景，记录 UI 和日志。
4. 根据实测补强错误文案，避免只显示底层路径或 Rust join error。

退出条件：

- 已完成：取消标记、provider 取消清 buffer 并请求 runtime cancel、缺文件错误信息、catalog 目录大小统计、下载校验失败、release archive 解包错误/成功已有离线单测。
- 待完成：真实损坏模型、真实下载中断/无网络、decode 中 native 取消、删除当前 loaded 模型仍需真机或更高层集成验证。
- 待完成：所有失败都能回到可继续使用状态的手测记录。

### P4 M4-4：性能、超时和资源释放：诊断已接入，待真实模型数据

目标：把 batch 路径从“能用”收敛到“不会明显卡死或占用异常”。

任务：

1. 已完成：runtime status 增加 `lastPrepareMs`、`lastTranscribeMs`、`lastAudioMs`、`lastError`，并在 LocalAsr 页显示最近一次诊断。
2. 已完成：prepare / transcribe 成功或失败都会写日志，包含模型、音频时长、耗时和错误。
3. 为每个模型记录首次 prepare 耗时、二次 prepare 耗时、10s 音频 RTF、30s 音频 RTF、峰值内存。
4. 检查 `sherpa_audio_transcribe_timeout_duration()` 是否覆盖低配机器，必要时按模型/音频长度调整。
5. 验证 `sherpa_onnx_keep_loaded_secs` 的延迟释放：0 秒、默认值、长时间保持加载。
6. 确认 release 后内存明显回落，重复 prepare/decode 不泄漏。
7. 检查 `spawn_blocking` 下长 decode 对 UI 响应、热键取消、窗口操作的影响。

退出条件：

- 待完成：有一张真实模型性能记录表，能支持默认模型选择和 timeout 设置。
- 低配机器上的失败模式是“超时并提示”，不是 UI 假死。
- release / delete / provider switch 后 runtime 状态一致。

### P5 M4-5：Windows 打包与安装验证

目标：确认 sherpa static dependency 不破坏 Windows 安装包。

当前记录：

- 2026-05-26 `npm run tauri build` 已完成前端 build 和 Rust release exe 编译。
- WiX 3.14 下载 blocker 已解除，`src-tauri/target/release/openless.exe` 已生成。
- 已修复 MSI WiX 片段中的 x86 IME component ICE80 blocker：x86 DLL 仍安装到 `INSTALLDIR\windows-ime\x86` 并由 `SysWOW64\regsvr32` 注册，但 MSI component 标记为 64-bit，避免 x64 `INSTALLDIR` 下混入 32-bit component。
- 手动执行 WiX `light.exe` 可从 Tauri 生成的 `main.wixobj` 和 `openless-ime.wixobj` 产出 `src-tauri/target/release/bundle/msi/manual-openless.msi`，仅剩 ICE03 / ICE40 / ICE57 / ICE61 warnings。
- `npm run tauri build` 的 Tauri MSI wrapper 仍在 `light.exe` 阶段退出 1 且未打印详细 stderr，需要继续定位 wrapper 参数或日志差异。
- `npm run tauri -- build --bundles nsis` 当前被 NSIS 3.11 工具包下载超时阻塞，下载 URL 为 `https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip`。

任务：

1. 跑 Windows Tauri build。
2. 检查 NSIS / MSI 产物体积增量。
3. 安装到默认路径，首次启动，下载模型，prepare，转写。
4. 安装到包含空格和中文的路径，重复首次启动和模型 prepare。
5. 卸载 / 重装，确认模型目录和用户数据行为符合预期。
6. 检查 CI workflow 是否需要缓存或超时调整。

退出条件：

- NSIS / MSI 两类包均可安装启动。
- 安装版能下载并加载至少 SenseVoice。
- 路径含空格 / 中文没有已知 blocker，或 blocker 已登记并有修复方案。

### P6 M6-1：实验发布收口

目标：把 batch 能力作为实验功能交给用户，而不是默认开启。

任务：

1. 保持 `sherpa-onnx-local` 在高级页，不进入新手 provider 列表。
2. UI 文案明确“Windows / 本机 / 离线批量识别 / 实验性”；online 模型明确标记为流式实验能力，batch 模型不承诺实时输出。
3. 对模型下载大小、CPU 占用和首次加载时间给出可见提示。
4. 补充 release notes：支持范围、推荐模型、已知限制、如何回退 Foundry。
5. 建立反馈字段：Windows 版本、CPU、模型、录音时长、错误日志。

退出条件：

- 用户可以理解它是实验选项。
- 出问题时能回退 `foundry-local-whisper`。
- release notes 不承诺 streaming 稳定发布或默认替换 Foundry。

### P7 M5-0：流式可行性调研：API / spike 工具已完成，真实模型数据待补

目标：先确认 Rust binding 能力和 online 模型可用性，再动主链路。

任务：

1. 已完成：`sherpa-onnx` 1.13.2 Rust API 暴露 `OnlineRecognizer`、online stream、partial result、endpoint/final segment 相关方法。
2. 已完成：找到 Windows CPU 可评估的 streaming 中文/英文模型，优先 Zipformer：`csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20`。
3. 已完成：新增独立 spike 程序 `src-tauri/examples/sherpa_online_spike.rs`，用于验证 16kHz PCM 分块输入 -> partial result -> final result。
4. 记录 online 模型文件结构、下载源、包体、RTF、内存。
5. 如果 Rust binding 不足，评估升级 crate 或 FFI 的成本；不要在主分支里半接入。

退出条件：

- 已完成：结论为“能接，不需要先升级 crate”。
- 已完成：有一个可运行的最小 online recognizer spike 工具。
- 已完成：online 模型 catalog 草案和下载文件清单已进入代码。
- 待完成：用真实 Zipformer 模型和测试音频跑出 partial/final、RTF、内存数据。

### P8 M5-1：后端流式接入：代码已完成，待真实模型验收

目标：在不破坏 batch API 的前提下增加 online 路径。

拟改文件：

- `openless-all/app/src-tauri/src/asr/local/sherpa.rs`
- `openless-all/app/src-tauri/src/asr/local/sherpa_runtime.rs`
- `openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs`
- `openless-all/app/src-tauri/src/coordinator/dictation.rs`
- `openless-all/app/src-tauri/src/coordinator.rs`
- `openless-all/app/src-tauri/src/commands.rs`

实现状态：

1. 已完成：`sherpa.rs` 增加 `SherpaMode::Online` 的 Zipformer catalog 条目、repo 和 required files。
2. 已完成：`sherpa_runtime.rs` 新增 online recognizer cache，和 offline recognizer 分离。
3. 已完成：`sherpa_provider.rs` 增加 online worker/session 路径，保留现有 `transcribe(timeout)` batch 语义。
4. 已完成：Recorder 分块输入时驱动 online stream，输出 partial delta。
5. 已完成：final transcript 仍返回 `RawTranscript`，继续走 polish / insert / history。
6. 已完成：取消时停止 PCM 输入、丢弃 partial、递增 cancel generation，并释放当前 online worker。

退出条件：

- Offline batch 模型行为不变。
- 待实测：Online 模型能在录音期间产生 partial。
- 已完成：取消、超时、空音频都有明确行为。
- 已完成：单元测试覆盖 mode 分流和 cancel generation 的关键边界。

### P9 M5-2：前端流式 UX：代码已完成，待端到端验收

目标：把 sherpa partial 显示接入现有实时胶囊，但不和 polish streaming insert 混淆。

拟改文件：

- `openless-all/app/src/lib/localAsr.ts`
- `openless-all/app/src/pages/LocalAsr.tsx`
- `openless-all/app/src/pages/settings/LocalModelSection.tsx`
- `openless-all/app/src/i18n/*.ts`
- 当前监听 `local-asr-token` 的胶囊相关组件

实现状态：

1. 已完成：事件名复用 `local-asr-token`，payload 仍为 token string。
2. 已完成：UI 用 Batch / Streaming 标签区分 online 和 offline 模型。
3. 已完成：ASR partial 只显示在胶囊，最终 raw text 再进入 polish / streaming insert / history。
4. 已完成：online 模型复用 catalog、下载、prepare、删除、状态显示。
5. 已完成：i18n 补充流式模型、实验提示、CPU 占用提示。

退出条件：

- 待实测：录音时能看到 sherpa partial。
- 待实测：停止录音后最终文本和历史记录一致。
- 待实测：polish streaming insert 开关不会导致 ASR partial 直接打进光标。

### P10 M5-3：流式验收与发布判断

目标：决定流式是否作为实验能力发布，还是继续隐藏。

验收项：

- 中文短句 partial 延迟可接受。
- 30s 中文长句不会持续重复旧文本。
- 中英混合不出现明显 session 状态错乱。
- 取消后 partial 不继续刷屏。
- online 模型切回 offline 模型后 batch 仍可用。
- 与 macOS `local-qwen3` 的实时显示体验一致或限制已写明。

发布门槛：

- 低配 CPU 不会让 UI 明显卡死。
- online 模型下载和 prepare 成功率可接受。
- 用户能一眼区分 batch 模型和 streaming 模型。

---

## 9. M4 验收清单

### 功能

- Windows 用户能启用 `sherpa-onnx-local`。
- 默认模型 SenseVoice 可下载、加载、转写。
- Paraformer / Whisper / Qwen3-ASR 至少完成一轮基本转写 smoke test。
- 失败时进入现有错误路径，不崩溃、不写入乱码。
- 切换 provider、切换模型、删除模型后状态一致。

### 性能

- 记录每个模型首次加载耗时。
- 记录每个模型 10s / 30s 中文音频 RTF。
- 记录 decode 期间 CPU 和内存峰值。
- 明确默认超时是否足够覆盖低配 Windows 机器。

### 取消与错误

- 录音中取消。
- prepare 中取消。
- decode 中取消。
- 模型缺文件。
- 模型校验失败。
- 下载中断后恢复。
- 无网络启动。
- 模型目录含中文 / 空格。

### 打包

- Windows dev build 通过。
- `cargo test` Windows 通过。
- Tauri release exe 通过。
- 手动 WiX MSI link 通过，产物：`src-tauri/target/release/bundle/msi/manual-openless.msi`。
- 待完成：Tauri MSI wrapper 退出 1 的日志定位。
- 待完成：NSIS 工具包下载成功后的 NSIS bundle 构建。
- 待完成：安装后首次启动、模型下载、卸载 / 重装验证。

---

## 10. M5 流式方案草案

### API 方向

保留当前 batch API：

```rust
pub async fn transcribe(&self, timeout: Duration) -> Result<RawTranscript>;
```

新增 online 能力目前采用等价实现：

```rust
pub async fn SherpaOnnxAsr::new_for_model(
    runtime,
    model_alias,
    language_hint,
    token_handler,
) -> Result<Self>;

pub async fn SherpaOnnxRuntime::create_online_session(alias: &str)
    -> Result<SherpaOnlineSession>;
```

### 事件方向

优先复用 macOS 本地 Qwen3 的实时显示习惯：

- partial / stable token 推到前端胶囊。
- final transcript 仍返回 `RawTranscript`，继续走 polish / insert / history。
- 如果 polish 也启用 streaming insert，需要明确 ASR streaming 和 polish streaming 的顺序，避免两套流式输出互相覆盖。

### 模型方向

- 已新增 `SherpaMode::Online` 模型条目。
- 优先评估 streaming Zipformer bilingual zh/en。
- Offline 模型继续走 batch，不混用。

---

## 11. 风险与对策

| 风险 | 对策 |
|---|---|
| sherpa-onnx static feature 打包体积或 native 链接异常 | 早跑 Windows CI 和 Tauri bundle，记录产物体积 |
| ONNX Runtime / native 依赖冲突 | 继续隔离在 Windows target dependency；不与 Foundry 共享 runtime |
| 低配 CPU 机器 decode 慢 | 默认 SenseVoice small int8；按实测调整 timeout 和文案 |
| 模型下载失败或校验失败 | 保留镜像、断点文件、明确错误文案和重试入口 |
| 首次加载耗时长 | prepare 事件持续上报，UI 显示准备状态 |
| decode 中取消不够及时 | 保留 `cancel_generation`，M4 实测后决定是否需要更细粒度中断 |
| 路径含中文 / 空格加载失败 | 加入专项手测；必要时统一 path normalize |
| 流式 ASR 与现有 batch 收尾冲突 | M5 新增独立 online API，不改 batch API 语义 |
| 代码注释滞后造成误判 | M4 中清理所有 “M1 骨架 / 返回空串” 旧注释 |

---

## 12. 相关参考

- Windows sherpa 实现：
  - `openless-all/app/src-tauri/src/asr/local/sherpa.rs`
  - `openless-all/app/src-tauri/src/asr/local/sherpa_runtime.rs`
  - `openless-all/app/src-tauri/src/asr/local/sherpa_provider.rs`
  - `openless-all/app/src-tauri/src/asr/local/sherpa_download.rs`
- Windows Foundry 对照：
  - `openless-all/app/src-tauri/src/asr/local/foundry.rs`
  - `openless-all/app/src-tauri/src/asr/local/foundry_provider.rs`
  - `openless-all/app/src-tauri/src/asr/local/foundry_runtime.rs`
- macOS 本地 Qwen3 流式 token 参考：
  - `openless-all/app/src-tauri/src/asr/local/local_provider.rs`
- 主听写链路：
  - `openless-all/app/src-tauri/src/coordinator/dictation.rs`
- 前端本地 ASR：
  - `openless-all/app/src/lib/localAsr.ts`
  - `openless-all/app/src/pages/LocalAsr.tsx`
  - `openless-all/app/src/pages/settings/LocalModelSection.tsx`
