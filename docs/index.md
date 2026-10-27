# OpenLess 文档入口

状态：canonical；更新：2026-10-19。实现说明与当前源码保持一致；范围、接口合同和验收要求由各自文档维护。各专项文档的更新时间与状态单独标注。

## 范围与架构

- [2.0 范围](2.0-requirements.md)：平台边界、版本锚点与完成条件。
- [架构](architecture.md)：Core / 平台 Host / 界面分层、模块地图与验证入口。
- [目录与工程结构](structure.md)：仓库与应用工作目录、构建清单、源码定位和生成目录。
- [桌面验收清单](2.0-desktop-acceptance.md)：功能域 → 源码入口 → 必须的真实证据。
- [发布规范](../RELEASING.md)：分支、渠道、版本同步和平台发布条件。

## Linux 接入交接（egui，目录内互链）

- [交接总览](linux-egui-handoff/README.md)：阅读顺序、责任与复用起点。
- [01 Core 合同](linux-egui-handoff/01-core-contract.md)、[02 缺口登记](linux-egui-handoff/02-gap-register.md)（L01–L12）、[03 热键与窗口](linux-egui-handoff/03-hotkeys-and-windows.md)、[04 页面与领域](linux-egui-handoff/04-ui-domains.md)、[05 原生宿主与数据](linux-egui-handoff/05-native-host-and-data.md)、[06 事件与会话](linux-egui-handoff/06-events-and-sessions.md)、[07 验收](linux-egui-handoff/07-acceptance.md)。

- [Linux egui 日志约定](linux-egui-logging.md)：级别语义、`OPENLESS_LOG` 开关、轮转策略与不记录的内容。
- [Linux 已知缺口](linux-egui-known-gaps.md)：当前仍未闭环的行为与复现路径（如 G01 纠错卡）。

## 接口契约

- [加密云同步客户端](encrypted-cloud-sync.md)：Beta 3 新协议、受控数据范围、系统凭据库、恢复与验证边界。

- [官方云同步](cloud-sync.md)：可同步字段、GitHub 身份边界、版本冲突、本地恢复与验证。
- [云同步服务端交接](cloud-sync-server-handoff.md)：同步服务地址与端口（apic.openless.top:9443）、客户端请求行为、状态码映射与服务端验收清单。
- [Linux egui 后端契约](linux-egui-backend-contract.md)：`contract/backend-2.0.json`、启动快照、事件面与公开签名。

## 平台与运营

- [macOS CI 与打包耗时](macos-build-performance.md)：基线日志、Rust 编译优化、缓存边界与仅 macOS 验证入口。
- [CI 触发范围与缓存配额](ci-trigger-and-cache-policy.md)：改动范围门控、10 GB 缓存分配与维护、发版预热点。
- [Android APK / 悬浮窗计划](android-mobile-apk-overlay-plan.md)（实施中）
- [Android 输入法](android-ime.md)：语音、笔画、英文、轻量拼音、后台服务与真机验收入口。
- [火山引擎 ASR 配置](volcengine-setup.md)
- [讯飞（iflytek）ASR 配置](xfyun-asr.md)
- [百炼（DashScope）ASR 模型](bailian-asr-models.md)
- [Tauri CSP 边界](tauri-csp.md)
- [Gemini 专用转写](gemini-transcribe.md)：模型、请求形态与失败边界。
- [腾讯混元实时 ASR 长语音](tencent-long-audio.md)：分段策略与结束判定。
- [手机输入法后端整理与引擎选型](mobile-ime-engine.md)：Android / 鸿蒙共用后端与引擎边界。
- [远程输入：手机一次性证书配置](remote-input-certificates.md)
- [远程输入：息屏与录音恢复](remote-input-recording-recovery.md)
- [Homebrew 同步与本机提醒](homebrew-openless-updates.md)
- [macOS 实验性本地语音模型](experimental-local-asr.md)：三模型、旧版下架、升级清理和隔离验收。
- [本地 ASR 生命周期](local-asr-lifecycle.md)：下载、选择和推理状态的边界。

## 发布记录

- [2.0.2-Beta.1 发布说明](releases/2.0.2-Beta.1-release-notes.md)
- [2.0.1 发布说明](releases/2.0.1-release-notes.md)

## 验证入口（在 `openless-all/app` 执行）

- `npm test`：构建 React + 全部前端/合同测试（含 Core 快捷键回归）。
- `npm run build && npm run test:ui-motion`：生产前端的 Chromium/WebKit 动效交互回归；运行前使用 `npx playwright install --with-deps chromium webkit` 安装测试浏览器。
- `cargo fmt --all --check`：根 workspace 的 openless-core、linux-egui；Tauri 单独执行 `cargo fmt --manifest-path src-tauri/Cargo.toml --check`。
- `cargo test -p openless-core --locked`、`cargo test -p openless-linux-egui --locked`。
- `src-tauri` 及 `backend-tests` 被 workspace exclude，按平台独立构建。源码构建 Tauri 前初始化子模块：`git submodule update --init --recursive`；Core/Linux 独立检查不依赖 Tauri 子模块。
