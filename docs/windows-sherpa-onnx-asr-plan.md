# Windows sherpa-onnx 本地 ASR 实施规划

> 状态：草案 / 待评审
> 日期：2026-05-12
> 范围：仅 Windows；不替换 macOS `local-qwen3`；不替换 Windows `foundry-local-whisper`

按 OpenLess 现有架构（Coordinator 单一拥有者、ASR provider 独立模块、`AudioConsumer`
接口）来做，**不重写主链路、不动 macOS、不替换 Foundry**，新增一个 Windows 实验
provider。

---

## 1. 目标与非目标

### 目标

- **Windows 新增本地 ASR provider**：`sherpa-onnx-local`
- **复用现有听写主链路**：Recorder / Coordinator / polish / insert / history
- **支持中文为主，中英混合可用**
- **第一阶段 batch，第二阶段流式**
- **可与 `foundry-local-whisper` 并存切换**

### 非目标

- 不替换 macOS `local-qwen3`
- 不替换 Windows `foundry-local-whisper`，仅作为新选项
- 不做 Linux 支持（本期）
- 不做语者分离、长会议转写、字幕导出
- 不做云端模型，不做模型自训

### 明确边界

- **不动 Coordinator 的 phase enum / hotkey 流程**
- **不动 polish / insertion / history**
- **sherpa runtime 只通过 `AudioConsumer` + 转写函数对外暴露**
- **任何 sherpa 错误必须降级**：不能让用户的话丢失（与现有 ASR 失败语义一致）

---

## 2. 架构定位

按现有结构对齐 Foundry 路径：

```
asr/local/
  mod.rs                  # 增加 sherpa provider id 与 helper
  foundry_provider.rs     # 保留
  foundry_runtime.rs      # 保留
  sherpa_provider.rs      # 新增：AudioConsumer + transcribe()
  sherpa_runtime.rs       # 新增：模型加载 / 推理调用 / 生命周期
  sherpa_models.rs        # 新增：模型 catalog 静态表
```

主要扩展点：

- `ActiveAsr::SherpaOnnxLocal(Arc<SherpaOnnxAsr>)`
- `coordinator/dictation.rs`：`begin_session` / `end_session` 增加
  `#[cfg(target_os = "windows")]` 分支
- `commands.rs`：增加准备 / 释放 / 状态 / 模型管理命令
- `types.rs`：增加 `UserPreferences` 字段
- 前端 Settings 高级页：在 Windows 下新增第三个本地 ASR toggle

---

## 3. 模型策略

### 第一批模型（重点是中文）

| 模型 | 用途 | 备注 |
|---|---|---|
| **SenseVoice small (zh/en/ja/ko/yue, int8)** | 中文 + 多语言默认 | 体验通常优于 Whisper small；包小、速度快 |
| **Paraformer (zh, int8)** | 中文专用强力档 | 中文听写更稳；不擅长英文 |
| **Whisper small (multilingual, int8)** | 英文/通用 fallback | 与 Foundry Whisper 体验对齐基准 |

模型形态全部用：

- **ONNX**
- **量化 int8**
- **CPU 推理优先**

后续可选：

- **streaming Zipformer (zh)**：第二阶段流式使用

### 模型分发策略

- **不打进安装包**
- **首次启用时下载**
- **下载源带镜像**：HuggingFace / 镜像 / 自托管 CDN
- **校验 SHA-256**
- **存放路径**：
  ```
  %APPDATA%\OpenLess\models\sherpa-onnx\<alias>\
  ```

---

## 4. 模块设计

### 4.1 `sherpa_models.rs`

静态目录 + alias 解析，模仿 `foundry.rs::MODELS`：

```rust
pub const PROVIDER_ID: &str = "sherpa-onnx-local";
pub const DEFAULT_MODEL_ALIAS: &str = "sense-voice-small-zh";

pub struct SherpaModel {
    pub alias: &'static str,
    pub display_name: &'static str,
    pub family: SherpaFamily, // SenseVoice / Paraformer / Whisper / Zipformer
    pub languages: &'static [&'static str],
    pub mode: SherpaMode,     // Offline / Online
    pub files: &'static [SherpaModelFile], // name + sha256 + size + url
}
```

边界：

- **不在这里写下载逻辑**
- **不依赖 sherpa-onnx 类型**，纯描述

### 4.2 `sherpa_runtime.rs`

只这一处依赖 `sherpa-onnx` crate。

职责：

- **初始化 OfflineRecognizer / OnlineRecognizer**
- **缓存当前已加载的 recognizer**
- **暴露**：
  - `ensure_loaded(alias) -> Result<LoadedHandle>`
  - `transcribe_pcm(pcm: &[i16]) -> Result<String>`（offline）
  - `create_stream() -> SherpaStream`（online，第二阶段）
  - `release_now()`
  - `status_snapshot()`
- **生命周期**：
  - `lifecycle: AsyncMutex<()>`（与 Foundry 一致，串行化加载/释放）
  - 闲时延迟释放（参考 `local_asr_keep_loaded_secs` 模式）

边界：

- **不知道 Coordinator**
- **不知道 Recorder**
- **不动 UI**
- **不发 Tauri 事件**

错误统统返回 `anyhow::Error`，由上层翻译为前端文案。

### 4.3 `sherpa_provider.rs`

形状与 `foundry_provider.rs` 完全对齐：

```rust
pub struct SherpaOnnxAsr {
    runtime: Arc<SherpaRuntime>,
    model_alias: String,
    language_hint: Option<String>,
    buffer: Mutex<Vec<u8>>,        // PCM s16le 16kHz mono
    cancel_generation: AtomicU64,
}

impl AudioConsumer for SherpaOnnxAsr {
    fn consume_pcm_chunk(&self, pcm: &[u8]) { ... }
}

impl SherpaOnnxAsr {
    pub async fn transcribe(&self, timeout: Duration) -> Result<RawTranscript> { ... }
    pub fn cancel(&self) { ... }
}
```

边界：

- **batch 阶段不做实时 token 回调**
- **流式阶段独立加 `transcribe_stream(on_token)`，不破坏 batch API**

### 4.4 `coordinator/dictation.rs` 集成

新增分支，**完全 mirror 现有 foundry 分支**：

`begin_session`：

```rust
#[cfg(target_os = "windows")]
if sherpa::is_sherpa_onnx_local(&active_asr) {
    let local = Arc::new(SherpaOnnxAsr::new(...));
    store_asr_for_session(inner, sid, ActiveAsr::SherpaOnnxLocal(Arc::clone(&local)));
    let consumer: Arc<dyn AudioConsumer> = local;
    start_recorder_and_enter_listening(inner, sid, &active_asr, consumer).await?;
    return Ok(());
}
```

`end_session`：

```rust
#[cfg(target_os = "windows")]
ActiveAsr::SherpaOnnxLocal(local) => {
    match local.transcribe(sherpa_transcribe_timeout()).await {
        Ok(r) => { schedule_sherpa_release(...); r }
        Err(e) => { /* 与 foundry 失败分支同形 */ }
    }
}
```

边界：

- **不修改 Foundry 分支**
- **不修改 macOS Qwen3 分支**
- **复用 `RawTranscript` / `polish` / `insertion`**

### 4.5 `commands.rs`

新增命令（与 Foundry 同形，方便前端代码复用模式）：

- `sherpa_asr_status`
- `sherpa_asr_prepare`
- `sherpa_asr_release`
- `sherpa_asr_catalog`
- `sherpa_asr_set_model`

只在 `#[cfg(target_os = "windows")]` 下注册。

### 4.6 `types.rs`

新增字段（默认值 Windows = SenseVoice 中文，其他平台不可用）：

```rust
#[serde(default = "default_sherpa_model_alias")]
pub sherpa_onnx_model: String,

#[serde(default)]
pub sherpa_onnx_language_hint: String,

#[serde(default = "default_local_asr_keep_loaded_secs")]
pub sherpa_onnx_keep_loaded_secs: u32,
```

**不改 `default_active_asr_provider()`**：Windows 默认仍是
`foundry-local-whisper`，sherpa 通过高级开关启用。

### 4.7 前端

在 Windows 高级页加第三个 toggle 行：

- Foundry Local Whisper
- **Sherpa-Onnx Local（新增，实验）**
- 模型选择 / 准备 / 删除 / 路径

复用现有 `LocalAsr` UI 模式。i18n key 用 zh-CN 源 + en 镜像（按 AGENTS.md 规则）。

---

## 5. 依赖与打包

### 5.1 Rust crate

```toml
[target.'cfg(target_os = "windows")'.dependencies]
sherpa-onnx = "..."   # 选最新稳定版，feature 关闭非必要后端
```

注意：

- **关掉 CUDA / DirectML 等 feature**（v1 只用 CPU）
- **避免依赖 dynamic ONNX Runtime**：优先静态或随包附带 DLL
- **不要引入新的 native build chain**：保证 GH Actions Windows runner 能编

### 5.2 DLL / native 资源

如果 sherpa-onnx crate 自带 `onnxruntime.dll` / `sherpa-onnx.dll`：

- 通过 `build.rs` copy 到 target dir
- 由 Tauri bundler 一同打进 NSIS / MSI
- WiX 的 `Component` 落到 `INSTALLDIR`
- **严格遵守 AGENTS.md 的 Windows CI 红线**：
  - 两轮 NSIS / MSI
  - bash shell
  - `-sice:ICE80`
  - 不动 Repair 步骤

如果 crate 不带 DLL：

- 第一次启用时从镜像下载，与模型同目录
- 用 LoadLibrary delay-load

### 5.3 模型下载

复用现有 `LocalAsr` 模型管理 UX：

- 镜像选择
- 进度 + 取消
- SHA-256 校验
- 失败重试

---

## 6. 实施里程碑

### M1 Provider 骨架 (0.5 周)

- `sherpa_provider.rs` / `sherpa_runtime.rs` / `sherpa_models.rs` 文件结构
- `ActiveAsr::SherpaOnnxLocal`
- `commands.rs` 桩函数
- 前端 toggle + i18n
- **不实际推理**，先打通主链路（mock transcribe 返回空串或固定字符串）

### M2 Batch 推理可用 (1.5 周)

- 接 `sherpa-onnx` crate
- offline recognizer 加载
- WAV/PCM → text
- 模型：先只接 **SenseVoice small zh**
- 错误降级：失败回到 Foundry / Volcengine
- Windows 本机 smoke test

### M3 模型管理 + 多模型 (1 周)

- 加 Paraformer / Whisper small
- 模型下载 / 校验 / 删除
- 镜像源切换
- 模型切换不需要重启

### M4 性能与稳定性 (1 周)

- 启动时延、首次加载时延
- 内存占用
- 长录音稳定性
- 取消（hotkey 再次按下）行为正确
- DLL 缺失 / 模型损坏 / 路径含中文 / 路径含空格 全部覆盖

### M5 流式 ASR（可选，二阶段）

- 接 OnlineRecognizer
- 边录边 partial → `local-asr-token` 事件
- 与现有 macOS Qwen3 stream UX 对齐

### M6 发布

- 高级页打开为实验
- 收集真实用户反馈
- 满足质量门槛后再决定是否提升为 Windows 默认

---

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| sherpa-onnx Windows 打包带 native DLL，触发 WiX / NSIS 兼容问题 | 严格走 AGENTS.md 的两轮 bundle + `-sice:ICE80`；早期就在 CI 跑 |
| ONNX Runtime 版本冲突 | 锁版本；不和其他 crate 共享 ORT |
| 模型体积大，下载失败 | 强制镜像 + 断点续传 + SHA-256 + 明确错误文案 |
| 安装路径含中文/空格导致模型加载失败 | 用 `\\?\` 长路径前缀 + 单元测试覆盖 |
| 首次加载耗时长（用户以为卡死） | 加载阶段发 Tauri 进度事件；胶囊显示"准备模型"态 |
| CPU 性能不足机器卡顿 | 默认 SenseVoice small int8；提供更小模型；超时降级 |
| 推理 panic 干扰主进程 | 推理放 `spawn_blocking`，错误 → anyhow，绝不 panic 向上 |
| 与 Foundry / Qwen3 并存导致状态混乱 | 切换 provider 时强制 release 另一边；测试覆盖 |
| 取消语义不一致 | 严格按现有 `cancel_generation` 模式实现 |
| macOS / Linux 编译被影响 | 全部 sherpa 代码 `#[cfg(target_os = "windows")]` 包裹 |

---

## 8. 验收标准

### 功能

- Windows 用户能在高级页启用 `sherpa-onnx-local`
- 默认模型 SenseVoice small zh 可下载、加载、转写
- 中文短句听写质量明显优于 Foundry Whisper small（盲测）
- 失败时不丢用户的话（自动降级或留 raw）
- 取消、重复触发、连按热键不崩

### 工程

- 不动 macOS 编译产物
- 不动 Foundry 路径
- 不引入新的 CI 红线
- Windows MSI / NSIS 两轮构建仍然通过
- 包体增量在可接受范围（建议 < 50MB，不含模型）

### 测试

- `cargo test` Windows 通过
- 手测脚本：
  - 中文短句
  - 中文长句（30s+）
  - 中英混合
  - 安静 / 噪音
  - 取消
  - 切换模型
  - 切换 provider
  - 卸载模型
  - 无网络再次启动

---

## 9. 不做什么（再次明确）

- **不重构 ASR trait 体系**
- **不引入 ASR 中间层抽象**
- **不替换 Foundry**
- **不动 macOS Qwen3**
- **不做 Linux**
- **不做云端 fallback 改动**
- **不做模型微调**
- **不做多 provider 自动选择**

---

## 10. 相关参考

- 现有 Windows 本地 ASR 实现：
  - `openless-all/app/src-tauri/src/asr/local/foundry.rs`
  - `openless-all/app/src-tauri/src/asr/local/foundry_provider.rs`
  - `openless-all/app/src-tauri/src/asr/local/foundry_runtime.rs`
- 现有 macOS 本地 ASR 实现：
  - `openless-all/app/src-tauri/src/asr/local/local_provider.rs`
- 主听写链路集成点：
  - `openless-all/app/src-tauri/src/coordinator/dictation.rs`
- Windows CI / 打包红线：见仓库根 `AGENTS.md`「Windows CI 红线」一节
