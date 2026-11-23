# 平台边界白名单

宿主 `src-tauri/src` 不直接调用平台框架 API（`windows::Win32` / `objc2` / `jni` /
`core_foundation`）。所有平台实现都在 `crates/openless-platform`，宿主只经它的能力门面调用。

本文件逐条列出**仍然保留在宿主**的平台调用，以及为什么它们不能下沉。

检查方式：

```bash
scripts/check-platform-boundary.sh
```

## 允许保留的文件

| 文件 | 为什么保留 |
|---|---|
| `lib.rs` | 应用入口与启动编排：注册平台层的注入点（`AndroidEnv`、`AsrPaths`、诊断钩子、前台应用探测、Android 上下文）、创建窗口、启动运行时。这里的平台调用是"启动胶水"，不是能力实现。 |
| `tauri_coordinator_host.rs` | Tauri 宿主适配层：把内核 coordinator 的事件接到窗口/托盘/热键。平台差异只体现在"用哪个 tauri API"，不涉及平台原生调用。 |
| `native_capsule.rs` | 拉起外置 `native-capsule` 可执行（Windows/macOS 各自实现），宿主只负责进程生命周期与参数。 |
| `desktop_resources.rs` | 桌面资源策略（图标/模板/随包文件），与平台能力无关。 |
| `commands/*` | IPC 命令层：参数校验、权限检查、调用内核。平台调用仅限"权限查询转发到平台层"。 |
| `android/native_bridge.rs` | **Android 的 IPC 边界**：Kotlin 侧通过 JNI 调进来的入口，等价于桌面端的 `commands/*`。它持有 app 的 Coordinator/Backend 句柄与 JNI 导出符号，属于应用组装而不是平台能力。 |
| `android/platform_env.rs` | 实现平台层的 `AndroidEnv` 接口（宿主注入点本身）。 |
| `android/non_android.rs` | 非 Android 目标上的显式桩（`NotAndroid` 语义），供跨平台命令层编译。 |
| `windows_ime_profile.rs` | Windows IME 语言配置的宿主薄封装；非 Windows 分支返回 `NotWindows` 状态。 |
| `mobile_runtime.rs` | Android 启动编排：把 tao 的 JavaVM/Activity context 交给平台层并初始化存储根。 |
| `macos_card_panel.rs` | 卡片面板的窗口类替换：在 AppKit 安装 KVO 之前把 tao 的 `NSWindow` 初始化成 `NSPanel`。这依赖 tao 的 ivar 布局，属于**窗口创建胶水**，无法下沉（平台层不链接 tao）。 |
| `macos_streaming_input.rs` | macOS 流式逐字输入（WP7 待迁移）。 |
| `persistence/*` | Android Keystore 分支：凭据库的宿主侧实现（平台层 `credentials` 只提供 macOS Keychain 原语与 Windows keyring）。 |

## 迁移原则

1. **能力实现下沉**：新增平台能力一律进 `crates/openless-platform`，按平台分目录。
2. **宿主持有对象、平台层拿参数**：窗口句柄、`HWND`、`NSWindow*`、JavaVM 由宿主提供，平台层只接受不透明参数。
3. **注入优先于依赖**：平台层需要宿主信息时，定义 `install_*` 钩子（`AndroidEnv`、`AsrPaths`、诊断钩子、前台应用探测、`AndroidContext`），不反向依赖宿主模块。
4. **显式失败**：不支持的平台返回明确错误（`unsupported`），不静默返回 `None`。
