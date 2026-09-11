# 05：原生宿主、数据与系统集成

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。对应 L02、L03、L10、L11。平台效果归 Linux Host；现有代码可复用。

## 1. 音频（`audio.rs`）

- 已有：`LinuxCpalRecorder`（CPAL 录制，可指定首选设备）——已实现待实测（L11）。
- 缺口（L03）：无录音归档（RecordingArchive）、未执行录音期间系统静音/恢复、缺提示音与胶囊。关闭标准：归档、保留策略、失败恢复、同归档重试与历史重转写可用；所有终态恢复音量且反馈可见。

## 2. 上下文与观察（L02）

Core `ports.rs` 提供 `EditObservationSink` / `EditObservationAdapter`（`ports.rs:523-539`）；Linux 未注入实现，默认 `NoopEditObservationAdapter` 生效，Selection 的 `source_app` 为 None。关闭标准：按隐私设置捕获真实应用/允许的上下文；原生手改观察能产生 Core 纠错建议并拒绝迟到结果。

## 3. 数据与凭据

- `credentials.rs`：Secret Service 凭据存储（L11 待实测）。
- `model_store`（Core）：模型目录与下载；Linux 管理 UI 缺口见 L08。
- `remote_input.rs`：Remote Input TLS/H5、配对与会话桥接（L11 待实测）。
- `resources.rs`：打包资源路径。

## 4. 桌面集成（L10）

未实现：托盘（`capabilities.rs` 仅探测 `supports_tray`）、自启、通知（仅状态栏）、检查/下载/安装更新与重启。AppImage 能力判定（`LinuxPackageKind`）不等于更新器已接好。关闭标准：各能力有真实 Host 效果与失败反馈，不以配置状态伪造就绪。

## 5. 进程与运行时

- `runtime.rs`：任务派生与生命周期。
- `coding_agent.rs`：CLI 进程管理（L11 待实测）。
- `single_instance.rs`：单实例守护与启动意图转发（已实现）。
