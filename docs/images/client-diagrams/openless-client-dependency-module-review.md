# OpenLess 客户端依赖与模块审查

基线：`beta` / `defd6924`

## ZIP / 压缩依赖

- 客户端直接声明的 `zip` 已是 `9.0.0`，锁定到 `9.0.5`。
- `openless-core` 也使用 `zip 9.0.0`。
- `tauri-plugin-updater 2.14.0` 仍间接带入 `zip 4.6.1`。
- `sherpa-onnx-sys 1.13.8` 构建链仍间接带入 `zip 2.6.1` 和 `zstd 0.13.3`。
- 这两个上游链路仍是 `cargo audit` 的剩余高风险来源，未用 ignore 规则隐藏。

## 模块边界

- 前端窗口入口集中在 `src/App.tsx`：主界面、引导、胶囊、辅助窗口按窗口用途分流。
- 前端 IPC 按领域拆分在 `src/lib/ipc/`，但由 `src/lib/ipc/index.ts` 统一 barrel re-export；当前约 57 个模块消费者，Vite 构建会提示循环 chunk 依赖。
- Rust 宿主的边界基本清晰：`lib.rs` 负责 Tauri 入口和窗口生命周期，`Coordinator` 负责会话/热键，`openless-core` 负责听写领域逻辑，`persistence` 负责本地状态。
- 维护热点是超大文件：`persistence/credentials.rs`、`core_adapters.rs`、`recorder.rs`、`lib.rs`、`coordinator/hotkey_loops.rs`，以及前端 `LocalAsr/index.tsx`、`ProvidersSection.tsx`。

## 当前验证

- 前端完整测试通过。
- Rust workspace 656 tests 通过。
- 最新 beta CI 全平台通过。
- 生产 npm audit 0 vulnerabilities。

## 后续建议

1. 先升级 `tauri-plugin-updater` 和 `sherpa-onnx` 到能消除旧 ZIP/zstd 链的兼容版本，并分别跑 Windows/Android/更新器回归。
2. 按领域逐步取消前端 IPC barrel 的跨 chunk re-export，先从 devices、hotkeys、history、style-packs 开始。
3. 再拆分 Rust 大文件，优先把 `lib.rs` 的窗口/启动辅助函数、`credentials.rs` 的迁移与平台实现、`core_adapters.rs` 的 provider 适配拆到独立模块。
