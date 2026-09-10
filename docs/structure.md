# 应用目录与工程结构

状态：canonical；更新：2026-09-08。分层与调用链见 [架构](architecture.md)。

## 仓库与应用工作目录

```text
1-app/                            Git 仓库、分支与发布边界
├── AGENTS.md / docs/              规则、架构、合同说明和平台交接
├── README.md / README.zh.md       面向使用者和贡献者的双语介绍
├── RELEASING.md / USAGE.md        发布规则与使用说明
├── .github/workflows/            CI、Tauri、Android、Linux 发布
├── Casks/                        Homebrew 分发定义
├── Examples/                     示例数据
├── assets/ / video-materials/     产品展示材料
├── scripts/                      仓库级辅助脚本
└── openless-all/
    ├── design_handoff_openless/   设计交接材料
    └── app/                      npm 与 Core/Linux Cargo 工作目录
        ├── src/                  React / TypeScript 界面
        ├── crates/openless-core/  共享业务 Rust crate
        ├── src-tauri/            Tauri Host，独立 Cargo manifest
        ├── linux-egui/           Linux Host 和 egui UI
        ├── android/              Kotlin / AIDL / manifest / 前端片段
        ├── windows-ime/          原生 TSF/IME 工程
        ├── contract/             机器可读 backend-2.0 合同
        ├── scripts/              构建、平台检查与合同测试
        └── public/               Vite 静态资源
```

## 按任务定位源码

路径以 `openless-all/app/` 为基准。

| 任务 | 入口 | 相关边界 |
| --- | --- | --- |
| 启动与窗口分支 | `src/main.tsx`、`src/App.tsx` | typed IPC 启动快照；Tauri 配置与运行时窗口 |
| 主界面、页面与设置 | `src/components/FloatingShell.tsx`、`src/pages/`、`src/pages/settings/` | `src/state/` 组织界面状态；业务规则归 Core |
| 多语言、主题、组件 | `src/i18n/`、`src/styles/`、`src/components/` | 八种界面语言；tokens/global 样式 |
| 新增或调整 IPC | `src/lib/ipc/`、`src-tauri/src/commands/`、`src-tauri/src/lib.rs` | Rust/TypeScript 类型、注册、事件与 `contract/` 同步 |
| 共享业务入口 | `crates/openless-core/src/api.rs` | `events.rs`、`ports.rs`、`domains.rs`、`config.rs` |
| 听写和服务 | Core `dictation_engine.rs`、`provider_*`、`asr/`、`polish.rs` | Host 的录音、插入和本地模型适配 |
| 历史、词库、纠错、风格包 | Core `history.rs`、`vocabulary.rs`、`correction.rs`、`style_pack_store.rs` | Tauri `persistence/` 与对应 command |
| 官方云同步 | Core `cloud_sync.rs`、`cloud_sync_types.rs`、`cloud_sync_validation.rs`、`cloud_sync_transaction.rs` | Tauri `commands/cloud_sync.rs`；GitHub 身份、有限字段与版本冲突见 [云同步合同](cloud-sync.md) |
| Tauri 组装与系统能力 | `src-tauri/src/coordinator.rs`、`core_adapters.rs`、`tauri_coordinator_host.rs` | 窗口、热键、权限、平台输入与生命周期 |
| Linux 原生接入 | `linux-egui/src/main.rs`、`lib.rs`、`backend.rs` | `audio/credentials/fcitx5/hotkeys/settings` 等 Host 模块；见 [交接](linux-egui-handoff/README.md) |
| Android 集成 | `android/`、`src-tauri/src/android/` | `@android` 别名与 `merge-android-*.mjs` 生成链 |
| Windows 输入法 | `windows-ime/`、`src-tauri/src/windows_ime_*.rs` | 原生工程、IPC 协议、目标应用和安装检查 |

Core 其余模块按领域列于 [架构模块地图](architecture.md)。平台缺口、事件签名与验收项由专项文档维护，本文件只提供定位。

## 构建清单与生成文件

| 文件或目录 | 作用与维护方式 |
| --- | --- |
| `package.json` / `package-lock.json` | npm 命令、前端依赖与锁定版本；脚本从应用目录执行 |
| `Cargo.toml` / `Cargo.lock` | Core 与 Linux workspace；不覆盖 `src-tauri` |
| `src-tauri/Cargo.toml` / `Cargo.lock` | Tauri Host 的独立依赖图；本地 path 子模块须在解析前就绪 |
| `src-tauri/backend-tests/Cargo.toml` | 独立 Rust 回归 crate，按 CI 选择平台执行 |
| `vite.config.ts` / `tsconfig.json` | WebView 构建、TypeScript 与 Android 别名 |
| `src-tauri/tauri.conf.json` / `src-tauri/capabilities/` | 应用元数据、初始窗口、打包与 Tauri 能力权限 |
| `src-tauri/vendor/` | 原生 ASR 引擎与子模块；升级按 [qwen-asr 清单](qwen-asr-submodule-upgrade-checklist.md) |
| `src/lib/ipc/provider-descriptors.generated.json` | Core 导出的公开 provider 目录；生成命令见 [架构](architecture.md) |
| `contract/language-catalog.json` | 工作语言的原生保存值、显示代码、ASR 代码与 Apple locale；前端及 Core 直接共用，新增语种不分别修改三份映射 |
| `src-tauri/gen/` | Tauri 平台生成目录；Android 手写源与合成脚本保留在 `android/`、`scripts/` |
| `node_modules/`、`dist/`、各 `target/` | 依赖和构建产物，不作为源码或 docs 的事实来源 |

检查命令集中在 [架构的验证入口](architecture.md)，版本与发布流程集中在 [RELEASING.md](../RELEASING.md)。不要在目录说明中复制易变的命令数量、分支领先数或单次测试结果。

## 源码格式与注释

应用目录的 `.editorconfig` 定义基础缩进和换行。TypeScript、JavaScript、CSS、HTML、JSON 与 shell 使用锁定版本的 Prettier，执行 `npm run format` 修改格式，执行 `npm run format:check` 检查。配置见 `.prettierrc.json`；生成文件和第三方源码不参与格式化。

Rust 使用 rustfmt，分别覆盖根 workspace、`src-tauri/Cargo.toml` 与 `src-tauri/backend-tests/Cargo.toml`。C/C++ 使用 clang-format 23，遵循应用目录的 `.clang-format`。Android 手写 Kotlin 使用 ktfmt 0.64 的 `--kotlinlang-style --do-not-remove-unused-imports` 选项。

注释说明当前职责、调用约束、生命周期与失败处理；涉及 FFI 时写明所有权和 ABI 前提。已完成任务的过程说明、失效文档引用及重复代码含义的注释应删除。对协议兼容或平台限制的说明保留必要依据。
