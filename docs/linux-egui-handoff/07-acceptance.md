# 07：验收与证据

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-08。

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
| 打包 | `release-linux-egui.yml`（deb/rpm/AppImage，独立 manifest） |

## 3. 真机矩阵（L11/L12）

- 桌面会话：X11 与 Wayland 分开记录。
- 输入：fcitx5 插件热键路由、PRIMARY 选区、落字目标应用矩阵。
- 音频：CPAL 设备、系统静音/恢复终态。
- 凭据：Secret Service（含锁定/未解锁态）。
- 打包：AppImage / deb / rpm 安装、升级、回滚、卸载残留；签名与分发渠道证据。

## 4. 记录格式

`ID / owner / commit / 已完成效果 / 自动证据 / 设备证据 / 剩余限制`。“待实测”不是“未实现”，也不是“已完成”；验收报告按证据分级表述（见[范围](../2.0-requirements.md)第 4 节）。
