# 2026-09-26 Beta 开放 PR 集成

状态：记录。基线是当时的 `origin/beta`（`d9113e0b`）。集成发生在新分支上，没有改写 `beta` 或已有功能分支。

## 汇入

以下当时仍开放、且以 `beta` 为基线的 PR，都已作为合并提交进入该分支。`#1107` 已包含在 `#1111` 中。

| PR | 结果 |
| --- | --- |
| #1111 Beta 3 加密同步与 macOS 打包 | 整包汇入 |
| #1110 远程录音过期回调 | 生产逻辑已在 Beta 3；保留已格式化的测试 |
| #1106 Android APK 构建耗时 | 保留 Beta 3 的 ZIP 完整性检查，补上 Linux egui 证书指纹测试辅助 |
| #1104 Requesty | 汇入 |
| #1102 纯修饰键听写 | 两侧修饰键都保留，避免 Ctrl+Ctrl 被折成一侧 |
| #1094 胶囊流式原文与插入动效 | 汇入当时尚未进入基线的部分 |
| #1066 iOS Swift 应用与键盘 | 汇入源工程 |
| #1067 Android 输入法 | 汇入，并保留 iOS 说明 |
| #1060 Linux egui parity host | 作为当前 Linux 界面；共享桌面路径保留 Beta 上更新的热键、流式插入和润色预览 |
| #1096 百炼按渠道选择接口 | 汇入 Core；界面文案沿用已有目录 |
| #1091 ModelScope 下载 Qwen | 汇入下载源；远程元数据仍按当前 Beta 懒加载 |
| #1085 MiniMax ASR | 汇入；主机匹配保持精确域名，避免无关自定义端点被当成 MiniMax |
| #1087 速记 | 基线已有速记、历史保留和热键监督；保留加密同步写入与问答输出目标 |
| #1074 录音反馈时序 | 基线已包含；macOS 流式插入仍在会话结束时确认，不回到逐批等待 AX |
| #1073 macOS 润色预览布局 | 预览窗使用现有工具窗样式；保留 nonactivating panel |
| #1064 内置 PI | 汇入 `pi-backend` 与 `openless-computer`；Linux 包只在后端已准备时打入 |
| #1065 Linux 完整界面 | 与 #1060 的模块布局冲突，保留更新的 parity host，不把旧界面切片混回去 |
| #1055 Linux 设计令牌与设置弹窗 | 已被 parity host 覆盖，不回混早期切片 |

## 整理

- 根 workspace 的 `Cargo.lock` 按 Rust 1.98 重新锁定，使 `openless-computer` 可被 `--locked` 解析。
- 代码注释约定为英文，并说明当前约束；界面文案仍在 `src/i18n/`。本次把 Linux 打包脚本里的注释改成英文。全仓库历史注释没有在同一次集成里逐文件重写，避免把集成差异淹没在注释翻译中。
- 删除了不参与构建的 `memory-graph.md`。

## 未在本环境执行的验证

本环境的默认编译器是 Rust 1.83，而 Linux egui 与 `openless-computer` 需要更新的 toolchain。已用 Rust 1.98 确认 workspace `cargo metadata --locked` 能解析 `openless-core`、`openless-computer` 和 `openless-linux-egui`。没有跑完整 `cargo test` 或桌面打包。
