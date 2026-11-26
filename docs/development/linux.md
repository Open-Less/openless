# Linux 宿主与 egui 接口说明

Linux 端采用独立的原生 Rust 实现（`openless-all/app/linux-egui/`），使用 `egui` / `eframe` 界面与常驻宿主进程，**不依赖 Tauri 或 WebKitGTK**。宿主在进程内直接调用 `openless-core` 的类型化 Rust 契约接口。

## 契约与基线来源

- **共享契约版本**：`openless_core::BACKEND_CONTRACT_VERSION`（当前 `2.0.0`，见 [`openless-all/app/crates/openless-core/src/lib.rs`](../../openless-all/app/crates/openless-core/src/lib.rs)）。破坏性宿主契约变更必须递增主版本号并在本文记录迁移说明。
- **跨层契约清单**：[`openless-all/app/contract/backend-2.0.json`](../../openless-all/app/contract/backend-2.0.json) 定义启动快照（`startupSnapshot`）、语义事件（`backendEvent`）与 Linux 门面接口（`linuxFacade`）。
- **命令与事件兼容基线**：[`docs/linux-egui-command-event-baseline.json`](../linux-egui-command-event-baseline.json) 记录 Tauri 兼容命令、传统事件、Core 语义事件与 Linux 能力矩阵的归属边界，由 `openless-all/app/scripts/check-command-event-baseline.ps1` 校验。

## 模块与运行架构

1. **宿主构建与端口注入**（[`backend.rs`](../../openless-all/app/linux-egui/src/backend.rs)）
   `LinuxBackendBuilder` 组装 `openless-core` 实例，并注入 Linux 平台的音频采集（`cpal`）、凭据存储（Secret Service）、文本插入（Fcitx5 插件 / 剪贴板回退）、热键、本地设置与资源路径适配器。
2. **宿主接口门面**（[`lib.rs`](../../openless-all/app/linux-egui/src/lib.rs) / [`runtime.rs`](../../openless-all/app/linux-egui/src/runtime.rs)）
   宿主通过 `LinuxBackendRuntime` 与 `openless_core::contract` 直接调用强类型 Rust API（读取快照、保存配置、启动/停止/取消听写、管理词典与风格包等），并通过事件订阅持续消费 Core 发出的语义事件（`BackendEvent`）。
3. **常驻宿主与界面子进程通信**
   常驻宿主与 egui 界面子进程经 Unix Socket（JSONL v2）交换请求、状态快照与增量事件：
   - 建立连接并完成 Hello/Ready 握手后，宿主先下发完整快照，再推送增量事件；重开窗口时重新握手。
   - 关闭主窗口后的常驻行为取决于系统托盘状态。
   - 切换页面时仍需持续消费事件流；迟到回执按窗口代次与会话 ID 过滤，避免旧会话污染新状态。
4. **设置写入、历史与远程输入**
   - 设置更新通过字段补丁（`preference_patch.rs`）串行提交，仅在修订号冲突时按规则重试并保留失败草稿。
   - 历史记录与音频探测放到后台异步任务，避免阻塞 UI 渲染帧；概览页复用统计缓存。
   - 远程输入会话仅向当前连接推送本会话事件，会话结束后仍保留必要的取消与撤销语义。

## 系统集成与能力边界

- **Fcitx5 集成**：Fcitx5 插件源码位于 [`openless-all/app/scripts/linux-fcitx5-plugin/`](../../openless-all/app/scripts/linux-fcitx5-plugin/)（协议说明见其 [README.zh.md](../../openless-all/app/scripts/linux-fcitx5-plugin/protocols/README.zh.md)）。单实例锁获取后，宿主会检测 Fcitx5 插件与会话 D-Bus 状态，再注册所需热键；若 Fcitx5 由 Wayland 合成器托管且无法重启，则继续使用当前运行实例。
- **能力如实上报**（[`capabilities.rs`](../../openless-all/app/linux-egui/src/capabilities.rs)）：Linux 端当前不提供内置本地 ASR 推理、应用内自更新器以及可靠的跨应用手改观察能力，宿主通过 `LinuxCapabilitySnapshot` 如实向界面声明。

## 开发、测试与本地打包

在 `openless-all/app` 目录下执行（需要 Rust 1.99+ 与 Linux 音频/桌面系统开发库）：

```bash
# 运行共享 Core 与 Linux egui 单元测试
cargo test --locked -p openless-core
cargo test --locked -p openless-linux-egui

# 本地打包 deb / rpm 与 Fcitx5 插件（不提供 AppImage）
bash scripts/package-linux-egui.sh

# 校验安装包 ELF 依赖、文件清单、桌面元数据、Fcitx5 路径与 SHA-256
bash scripts/verify-linux-egui-packages.sh
```

> 说明：Linux 自动化发布工作流目前保持暂停，手动发布与版本修订号（`openless-all/app/linux-egui/package-revision`）规则见 [维护者发布清单](../../RELEASING.zh.md)。

## 日志与排查

宿主进程与界面子进程共用同一日志文件 `openless.log`（时间戳使用本机时区；单文件上限 2 MiB，宿主启动时轮转，最多保留 3 份历史）。可在「设置 → 高级 → 调试」导出含轮转历史的日志，或使用 `OPENLESS_LOG` 覆盖日志级别，例如：

```bash
OPENLESS_LOG=info,openless_core=debug openless
```

排查听写或落字异常时，可按日志标签 `[fcitx]`、`[hotkey]`、`[dictation]`、`[insert-diagnostics]` 对照实际行为。日志仅记录状态与错误类别，不记录用户听写正文、服务密钥或剪贴板内容（实现见 [`logging.rs`](../../openless-all/app/linux-egui/src/logging.rs)）。
