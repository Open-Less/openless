# Apple 识别、多模态与 CPU 预算修复

## 固定范围与待办

- [x] Apple Speech 恢复为 macOS 系统识别服务，独立于已下架和实验模型。
- [x] 恢复原生注册、系统语音权限声明和取消处理；渠道不显示模型管理控件。
- [x] Omni 的 `[DONE]` 结束流，不等待 HTTP EOF；静默流允许取消；连接握手单独限时 8 秒。
- [x] 实验模型使用启动时空闲逻辑 CPU 容量的 80% 作为平均 CPU 预算；CPU 推理线程至多 2。
- [x] TextStream 显式选择 Metal GPU；Gemma 保留 Core ML `.all`，CPU BLAS 单线程，原生推理使用 utility QoS。
- [x] CPU 检查点可以取消；超时设置取消标志；等待期间不把 UI/录音 CPU 累加成永远还不完的欠账。
- [x] TextStream 只升级已知、受管的 worker，不重装其 Python 环境、不重下模型。
- [x] 修复渠道编辑页返回按钮与 macOS 标题栏拖动区重叠。
- [x] 本地定向测试、前端完整测试与 diff 检查通过。
- [x] 提交修复，创建 [PR #1207](https://github.com/Open-Less/openless/pull/1207)。
- [ ] 云端 CI 全绿后合并；实时结果以 PR 检查与记录为准。

## 已验证证据

- Core 全量首次：1206 passed / 1 旧退役契约失败 / 1 外部 Codex 测试 ignored。旧契约移除 Apple 后，ProviderService 定向 30/30；退役元数据 7/7。
- Omni 三项真实本机 HTTP 流回归通过：`[DONE]` 后连接保持打开、空结束、静默读取消。
- 原生实验模型定向：16 passed / 4 需要模型文件的测试 ignored。含 CPU 实际计时、Darwin QoS、Swift/Rust 取消回调和受管 worker 升级。
- Apple 识别原生生产模块最终 37/37，包括只读系统类链接检查；不触发系统授权提示。
- 已有 SenseVoice 权重配合本地生成的固定英文合成语音，真实识别通过；音频 3.502 秒、推理约 604ms。没有读取或导出用户录音。
- Swift 以 macOS 14 部署基线完成 typecheck。Python 单元 11/11，Ruff、Black 和新增预算代码的 mypy strict 通过。
- 前端测试 runner 最终完整通过。浏览器原有 6 项通过；新增 Apple 渠道创建、分类、标题栏返回、停用/启用在 Chromium/WebKit 2/2 通过。

### CI 收尾

首轮 CI `38847638369`：Android、macOS Rust 1.88、UI 双浏览器、Windows、Linux 和 Changed areas 通过。macOS 完整原生单元测试为 579 passed / 1 failed / 4 ignored：剩余旧断言把 Apple 系统识别当作已退役模型。已把该断言改为「macOS 无需云凭据、其他平台不可用」，保留旧 Whisper 模型不可用的断言。本地完整原生测试还暴露 CPU 计时测试的并发测量边界问题：它把其他测试线程在推理让出期间的 CPU 也算进了推理预算。修正为在让出前冻结计量，不修改生产节流、不放宽预算断言。修正后使用正常原生依赖执行 `cargo test --lib --bins`：**580 passed / 0 failed / 4 explicit model-file-dependent ignored**。后续结果见 PR #1207，已有成功记录不删除、不伪装为新源码的检查结果。

## 明确边界

CPU 预算是分段平均限制，不是内核级瞬时硬上限。单次 ONNX/Core ML 调用可能短时突发；绝不暂停整个 OpenLess 进程来伪造限额。预算下限为 0.05 个逻辑核以保持进展。Apple 系统识别由 OS 管理，不声称能够限制其独立系统服务。

Gemma 已允许 GPU/ANE/CPU，但没有本轮完整模型实测，不能声称 ANE 已被实际使用。TextStream 的 Metal 可用性 API 已核对固定版本 MLX 0.32.3 的官方源码；没有下载完整模型或声称整条真实流已验收。

当前多模态域名与标准 DashScope 域名的匿名、无认证 HEAD 探测均未完成 TLS 握手（HTTP 000）。这证实当前网络路径有问题，不能据此推断 API Key、模型名、额度或业务接口正常。`audio=false` 日志属于文本探测，不能当作录音丢失证据。

## 发布状态

beta6 已公开，不移动该标签。Stable `v2.0.0-tauri` 在 2026-10-16 仍为草稿；本轮修复完成前暂停发布。不将这些源码改动冒充已安装应用中的功能；当前 `/Applications/OpenLess.app` 未被本轮修改。

持久验证日志：`~/Library/Caches/pi-openless-release-20261016/`。不提交日志中的用户内容、认证资料或本机模型文件。根目录既有 `package.json`、`pnpm-lock.yaml` 保持不动。
