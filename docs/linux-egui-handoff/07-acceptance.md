# 07：验收与证据

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-27。

## 1. 两个不同的完成门

- **本批 Core 移交门**：可调用的 `2.0.0` 合同、真实共享业务、平台 Interface、无设备示例/fixture、跨平台依赖检查、逐项缺口文档（已完成，见[01](01-core-contract.md)）。
- **egui 团队 Linux 产品门**：补齐[登记项](02-gap-register.md) L01–L12，取得真实桌面、设备、安装升级与正式分发证据。

## 2. 自动验证（在 `openless-all/app/` 执行）

| 验证 | 命令 |
| --- | --- |
| 格式 | 根 workspace：`cargo fmt --all --check`（openless-core / linux-egui）；Tauri：`cargo fmt --manifest-path src-tauri/Cargo.toml --check` |
| Core | `cargo test -p openless-core --locked` |
| Linux Host/合同 | `cargo test -p openless-linux-egui --locked` |
| Linux 目标编译 | `cargo check -p openless-linux-egui --all-targets --target x86_64-unknown-linux-gnu --locked`（需配置好带 `core/std` 的目标工具链） |
| 打包 | `release-linux-egui.yml`（deb/rpm + `SHA256SUMS`；AppImage 已从该渠道下线） |

## 3. 真机矩阵（L11/L12）

- 桌面会话：X11 与 Wayland 分开记录。
- 输入：fcitx5 插件热键路由、PRIMARY 选区、落字目标应用矩阵。
- 音频：CPAL 设备、系统静音/恢复终态。
- 凭据：Secret Service（含锁定/未解锁态）。
- 打包：deb / rpm 安装、升级、回滚、卸载残留；校验和与分发渠道证据。

## 4. 记录格式

`ID / owner / commit / 已完成效果 / 自动证据 / 设备证据 / 剩余限制`。“待实测”不是“未实现”，也不是“已完成”；验收报告按证据分级表述（见[范围](../2.0-requirements.md)第 4 节）。

## 5. 本轮（Beta.3-103）设备验收清单

本轮改动集中在启动/运行时/保存三条链（`fcitx5.rs`、`ui/bridge.rs`、`linux_app/{window,settings_save,history}.rs`、`preference_patch.rs`），以及上一轮的 Tauri 视觉对齐项。逐项结果按第 4 节格式记录，未实测的不要写成已验证。

### 5.1 fcitx5 成为启动硬依赖（`fcitx5.rs::prepare_fcitx5`）

| 场景 | 期望 |
| --- | --- |
| 插件已装、fcitx5 运行 | 启动不阻塞（15 秒就绪预算内），热键可用 |
| 插件缺失/未启用 | 弹出错误窗口（`startup.fcitx_help` 文案）；**关闭后进程以非零状态退出并释放单实例锁**，可立即重新启动 |
| 启动后才 `fcitx5 -r` 重启 | 插件重载完成后热键仍可用（验证就绪等待是否覆盖慢重启） |
| 无会话 D-Bus（ssh/`systemd-run` 启动） | 明确失败并给出修复指引，不得静默降级为全局热键 |

### 5.2 UI 子进程协议 v2（`ui/bridge.rs`）

| 场景 | 期望 |
| --- | --- |
| 关闭 UI 主窗后从托盘/启动器重开 | 宿主与后台能力（热键、录音、托盘）持续存活；重开后重新握手并收到全量快照，历史/设置不丢 |
| UI 进程被 `kill -9` | 宿主不崩；下次开窗握手正常 |
| 状态高频变化（历史、录音电平）时 UI 较慢 | 显示的是最新状态，不出现回退到旧快照 |
| 误接协议不匹配的 UI 二进制 | 回 `Rejected` 并断开，而不是渲染半懂数据 |

### 5.3 设置保存改为字段补丁（`linux_app/settings_save.rs`、`preference_patch.rs`）

| 场景 | 期望 |
| --- | --- |
| 快速连改多个开关/滑条 | 不出现“改 A 丢 B”；最终值与最后一次操作一致 |
| 制造 revision 冲突（另一端同时改设置） | 冲突被重试（≤3 次）后正确落盘；仍失败时保留草稿并显式提供重试，不打开设置页 |

### 5.4 历史缓存与删除身份（`linux_app/history.rs`）

| 场景 | 期望 |
| --- | --- |
| 删除历史条目 | 删除的是点中的那条（不再因刷新错位）；列表与概览计数同步 |
| 历史条目很多时打开概览/设置 | 首屏不卡顿（读取与 WAV 探测在后台任务，两条页面共享缓存） |

### 5.5 回归：上一轮 Tauri 视觉对齐项

OAuth 弹窗（加宽、标题字号、居中状态行、整行验证码框、关闭按钮悬停不缩放）、风格页卡片（图标选择器贴右、铅笔/重置徽标、名称/描述/标签行）、市场“我的发布”上传/更新/下架流程。

### 5.6 安装与升级

`dpkg -i` 用 103 覆盖 102（apt 视为升级，不回退）；rpm 同理；`apt remove` 后无残留（fcitx5 postinst/postrm 行为）；核对 `openless --version` 自报 `OpenLess 2.0.0-Beta.3-103`。
