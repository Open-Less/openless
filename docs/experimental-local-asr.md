# macOS 实验性本地语音模型

macOS 的旧本地模型管理功能已下架。「设置 → AI 服务与模型 → 本地模型」保留灰色、不可点击的占位入口；鼠标悬浮或键盘聚焦时显示 **「该功能已下架，请去实验性与扩展里面」**。旧本地渠道也停用并置灰。Windows 原有模型管理入口和云服务渠道不受这次下架影响。

新功能位于 **设置 → 实验与扩展 → 实验性本地语音模型**，排在扩展列表第一位。三个实验单独开启，不会自动替换已有首选渠道。

## 三种方案

| 模型 | 下载与平台 | 运行方式 |
| --- | --- | --- |
| **TextStream（推荐）** | 模型约 1.9 GB，另需独立 Python/MLX 运行组件；Apple Silicon，macOS 14+ | OpenLess 采集 PCM；仅录音时启动独立 worker，输出可修订草稿和最终文本；结束或取消后退出 |
| **SenseVoice Small** | 约 240 MB；macOS | CPU；普通话、粤语、英语、日语、韩语；录音停止后返回全文 |
| **Embedding Gemma** | 约 715 MB；Apple Silicon，macOS 15+ | Core ML 英语语音转文字；录音停止后返回全文；不是语言模型或多模态生成服务 |

TextStream 本身使用 Qwen3-ASR/MLX。移除的是主程序中的旧 Qwen/Whisper/Apple Speech 原生实现、MLX 链接和 Metal 资源，不是声称 TextStream 不再依赖 Qwen/MLX。它的依赖只进入所选模型目录中的 `.runtime`，不安装到系统 Python，也不打开自己的麦克风。

每个模型都有独立开关、下载/安装状态、错误与重试入口，以及明确的内存占用警告。网络失败不会初始化推理，也不会把失败的模型标成可用。

## 开关、渠道与生命周期

1. 开启开关：显式下载固定版本的文件并校验大小/SHA-256；TextStream 还安装固定版本、哈希锁定的运行组件。此阶段不导入 MLX 或加载模型。
2. 完成后注册独立语音渠道并提示用户选择。下载不会启用一个原本没有启用渠道的语音流程，也不会替换已有首选渠道。
3. 实际使用：在传统语音识别流程中，开启需要的渠道并拖到最上方。录音开始前检查实验开关、文件完整性和会话代次，之后才加载引擎。
4. 关闭实验开关：取消未完成任务、移除该实验拥有的自动渠道、释放推理，**保留模型文件**。停用后可以单独删除。
5. 停用渠道或切换到其他渠道：释放对应运行状态，不更改实验开关或删除文件。
6. 重启/打开设置：文件和元数据查询，不加载模型。只有持久化标记为中断的、先前由用户明确启动的下载/安装/删除操作可以恢复。单独的 `enabled=true` 不允许补下载；文件丢失时须手动重试。

状态文件：`experimental-asr.json`。旧的两模型状态按原有 SenseVoice/Gemma 索引迁移，TextStream 默认关闭，不会错位。文件目录分别为 `experimental-textstream-qwen3`、`experimental-sensevoice-small`、`experimental-embedding-gemma-en`。

## TextStream 的流式约定

- OpenLess 始终拥有录音；worker 只接收 16 kHz、单声道、16-bit PCM，不调用录音设备。
- 草稿是整体替换快照，可缩短、修订；不能按追加字符串处理。
- 完成时以 decoder 的 finalization 结果为准。取消、EOF、协议错误和超时都不能把最后一段草稿当最终结果写入历史。
- 录音期间，本机应用可以订阅 `http://localhost:7890/stream`。接口仅绑定 loopback，检查 Host/Origin，并限制订阅数和排队长度。
- SSE 的终态仍是 `type: stream`、`done: true`，且 `draft` 为空。录音结束后关闭 worker 和接口。
- worker 有加载/完成超时、父进程死亡监控、进程组清理和十分钟音频上限。端口占用会在推理导入前失败。
- TextStream 强制实时送入音频，但不修改用户保存的“录音后识别”偏好；其他渠道仍遵循该偏好。

## 旧版升级清理

macOS 启动时，在配置的模型根目录和默认根目录中清除已知旧 Qwen/Whisper 安装与中断下载目录，并退休旧渠道。操作幂等；失败记日志，下次启动重试。旧同步设置也不能绕过后端重新下载、启用或推理。

不删除根目录、实验包、Windows 模型、录音、无关文件、其他应用数据或 macOS 系统语音资源；不沿模型路径、父目录或模型根目录的符号链接删除外部文件。**只有运行新版应用才执行真实升级清理，编译/测试不清理当前安装应用的数据。**

## 模块与来源

- Core：`crates/openless-core/src/experimental_asr.rs` 管理状态、文件、渠道和租约，不依赖 Tauri 或原生推理库。
- 原生：`src-tauri/src/asr/local/experimental/` 中分别实现 Gemma FFI 和 TextStream 的安装、协议、worker；共享适配器只负责选择模块和会话生命周期。
- TextStream：MIT；实现基于固定上游修订的流式设置，保留许可。Qwen 权重固定 revision；Python 3.12.15、uv 0.13.1、mlx-qwen3-asr 0.4.4、MLX 0.32.3、silero-vad-lite 0.4.0；依赖由 `requirements.lock` 固定版本与 wheel 哈希。
- SenseVoice：固定仓库 revision 和两个文件的 SHA-256。
- Gemma：固定发布文件、大小和 SHA-256，保留 NOTICE、Apache-2.0 许可；部署还需遵守 NOTICE 指向的 Gemma 使用政策。适配器不调用其他应用的可执行文件或私有 Swift ABI。

## 测试与隔离验收

```sh
cd openless-all/app
cargo test --locked -p openless-core
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo test --locked --manifest-path src-tauri/backend-tests/Cargo.toml
npm test
npx playwright test e2e/macos-experimental-asr.spec.ts
python3 -B -m unittest discover -s src-tauri/src/asr/local/experimental/textstream -p 'test_*.py'
```

debug 原生二进制及 debug `.app` 支持专用验收入口（发布构建不包含）：

```sh
src-tauri/target/debug/openless --openless-local-asr-check \
  /tmp/openless-asr-acceptance /path/to/english-16k-mono.wav --download
# 已安装并校验后去掉 --download，禁止测试暗中补下载：
src-tauri/target/debug/bundle/macos/OpenLess.app/Contents/MacOS/openless \
  --openless-local-asr-check /tmp/openless-asr-acceptance /path/to/english-16k-mono.wav
```

目录必须是显式指定的 `openless-asr-acceptance*` 隔离目录，不能是符号链接。验收使用真实权重/原生引擎和包含 hello、test、system 的固定 WAV，不用麦克风，不访问系统凭据库。覆盖三模型推理、TextStream SSE/最终文本一致性、取消不提交、停用保留文件、禁用后重启不加载、已启用重启/设置查询不加载，以及隔离旧模型的升级清理。通过后写入该目录的 `passed.json`。

浏览器回归使用 IPC mock，只证明界面行为，不能替代真实原生推理或签名发布验收。debug bundle 也不能替代 notarization 或真实 Windows 设备测试。

本次已完成的真实模型、进程生命周期、Release 打包与验证边界见 [验收记录](local-asr-verification.md)。
