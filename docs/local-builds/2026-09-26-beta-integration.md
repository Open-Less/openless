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

## 测试后整理

- `HistorySource` 只保留一个 `QuickNote`。重复变体让 `openless-core` 无法编译。
- 选区润色预览仍是独立工具窗。前端按 `?window=selection-polish-preview` 渲染 `SelectionPolishPreview`，能力表同时授权该窗口和语音意图窗。预览是否有效由 pending 标志决定。
- Linux 开发与打包从 `resources/pi-backend` 读取已准备的 PI 资源。准备脚本仍写入 `src-tauri/resources/pi-backend` 供 Tauri 打包，再链接到上述目录。Linux 源码和打包脚本不出现 Tauri 路径。
- CI 只保留 `linux-egui-package`。Core 与 remote TLS 已在可复用的 Linux workflow 里执行。
- 仓库根增加 `rust-toolchain.toml`，频道为 `stable`，避免默认 Cargo 1.83 解析不了 edition 2024。
- 去掉指向不存在的 `vendor/wayland-scanner` 的补丁。`wry` 的本地补丁保留。
- Tauri 宿主里的 Linux `cfg` 仍保留。支持的 Linux 产品是 egui；React 侧不再长出第二套 Linux 界面。契约按这个边界检查，而不是要求 Tauri 在 Linux 上 `compile_error!`。
- 前端测试运行器对 `.mjs` 也使用 `tsx`，这样合同测试可以导入 TypeScript 模块。
- 目录说明改为真实仓库根，并写上 workspace 的三个成员。

## 本环境验证

Rust 1.98.1（`stable`）。`cargo test --locked -p openless-core`：17 个测试二进制，1211 通过，0 失败，1 忽略。热键边界测试 `shared_hotkey_edges_own_hold_auto_and_combo_abort_semantics` 通过。`openless-computer` 11 通过。`src-tauri/backend-tests` 的 `remote_tls` 10 通过。`openless-linux-egui --lib`：186 通过，1 个忽略（需要真实 Wayland/X11 会话）。locale 测试先清掉 `LC_ALL`、`LC_MESSAGES` 和 `LANG`，避免宿主已有的 `LC_ALL` 盖过被测变量。前端 `npm run build` 后跑完除该热键门禁以外的 107 个发现测试，退出码 0；热键门禁单独通过。

未在本环境完成的部分：Tauri 桌面宿主、iOS 工程、Android 模拟器未构建；deb/rpm 未打包；`linux-egui` 只跑了库测试，没有跑全部 target 或发布包。
