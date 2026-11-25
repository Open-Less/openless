# Linux 宿主接口与日志

[Linux 宿主](../../openless-all/app/linux-egui/) 用 egui/eframe 与共享 Core 组成 Linux 应用。宿主实现 Fcitx5、CPAL 录音、Secret Service、桌面窗口与手机远程输入，Core 处理听写、渠道、资料和事件。当前走本地 deb/rpm 打包。

## 接口和调用顺序

1. [backend.rs](../../openless-all/app/linux-egui/src/backend.rs) 用 `LinuxBackendBuilder::from_shared_providers` 创建后端，为 Core 注入录音、插入、凭据、任务与设置接口。
2. [lib.rs](../../openless-all/app/linux-egui/src/lib.rs) 的 `LinuxHost` 提供 `backend`、`snapshot`、`subscribe`、`save_settings`、`update_settings_strict`。宿主直接调用 Rust 类型化接口，再从 `EventSubscription` 消费 Core 事件。启动信息、事件序列和接口以 [backend-2.0.json](../../openless-all/app/contract/backend-2.0.json) 的 `startupSnapshot`、`backendEvent`、`linuxFacade` 为准。
3. 常驻宿主通过 Unix socket JSONL v2 接入界面子进程。Hello/Ready 握手完成后先给完整快照，再发送增量。重开窗口重新握手。关闭窗口的实际驻留行为取决于托盘状态。进入其他页面仍要消费事件，迟到回执按窗口代次和会话身份处理。
4. `settings_save.rs` 串行写字段补丁，仅重试修订号冲突并保留失败草稿。`history.rs` 将历史文件和 WAV 探测放到后台单任务，概览共享缓存。远程输入向本连接发送本会话事件，结束后仍保留必要的取消与撤销语义。

Fcitx5 是启动依赖。持有单实例锁后检查插件与会话 D-Bus，再注册必要热键。Fcitx5 由合成器托管且不可重启时，让运行中的副本继续工作。Linux 无本地 ASR 推理、自更新器及可靠的原生手改观察能力，宿主应如实报告能力。

## 打包与验证

从应用目录运行 `cargo test --locked -p openless-linux-egui`，打包及验包走已签入的脚本。分发所需的真实设备和安装证据列在[开发需求](../requirements.md#linux)。[打包脚本](../../openless-all/app/scripts/package-linux-egui.sh) · [验包脚本](../../openless-all/app/scripts/verify-linux-egui-packages.sh)

## 日志

宿主与界面子进程写同一份 `openless.log`。单文件上限 2 MiB，宿主启动时轮转，最多保留三份历史。设置 → 高级 → 调试可导出含轮转历史的日志。`OPENLESS_LOG` 优先于兼容环境变量和本机设置，例如 `OPENLESS_LOG=info,openless_core=debug openless`。日志时间使用用户本地时区。

找听写异常时按 `[fcitx]`、`[hotkey]`、`[dictation]`、`[insert-diagnostics]` 对照实际效果。失败写 `error`，可继续的降级用 `warn`，状态变化用 `info`，高频过程细节用 `debug`。日志记录状态与类别，避免包含用户听写正文、凭据或剪贴板。实现见 [logging.rs](../../openless-all/app/linux-egui/src/logging.rs)。
