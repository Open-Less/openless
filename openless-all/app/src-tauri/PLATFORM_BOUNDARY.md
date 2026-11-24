# 平台边界规则

宿主 `src-tauri/src` 不直接调用平台框架 API；平台实现都在 `crates/openless-platform`。

检查方式：

```bash
scripts/check-platform-boundary.sh
```

## 放行规则（从宽到严）

1. **整文件放行**：下表列出的文件，必须写清理由；
2. **逐行放行**：该行带 `// boundary-allow: <理由>` 尾注释；
3. **平台层门面调用**不算越界：`openless_platform::…` 与宿主 JNI 门面 `crate::android::jni::…`。

## 整文件放行

| 文件 | 为什么不能下沉 |
|---|---|
| `android/native_bridge.rs` | **Android 的 IPC 边界**：Kotlin 通过 JNI 调进来的入口，等价桌面端 `commands/*`。它持有 Coordinator/Backend 句柄与 JNI 导出符号，属于应用组装而不是平台能力。 |
| `macos_card_panel.rs` | 在 AppKit 安装 KVO **之前**把 tao 的 `NSWindow` 初始化成 `NSPanel`。依赖 tao 的 ivar 布局，而平台层不链接 tauri/tao。 |

## 逐行放行（当前 7 处，全部是 tauri-nspanel）

`lib.rs` 与 `capsule_mirrors.rs` 里用到 `tauri_nspanel::WebviewWindowExt::to_panel()` /
`tauri_nspanel::init()` / `NSWindowCollectionBehavior`。这些 API 需要 **tauri 的窗口对象与插件注册表**，
平台层不链接 tauri，无法下沉。窗口的 AppKit 行为（层级、collectionBehavior、非激活样式位、
`orderFrontRegardless`）已经全部在 `openless-platform::macos::window` / `macos::capsule_window` 里。

## 已下沉到平台层的平台原语（供对照）

| 领域 | 平台层位置 |
|---|---|
| Windows 窗口区域 / DWM 配色 / 置顶显示 / 按标题查找 | `windows/window.rs` |
| Windows 胶囊命中测试、点击穿透、不抢焦点 | `windows/capsule_window.rs` |
| Windows 前台应用与窗口激活 | `windows/front_app.rs` |
| Windows 选区（模拟复制 + 零剪贴板 TSF→UIA） | `windows/selection.rs` |
| Windows 外壳集成（打开路径/设置页） | `windows/shell.rs` |
| macOS 窗口焦点 / 激活 / 可拖动 / 非激活显示 | `macos/window.rs` |
| macOS 胶囊展示（层级 + 跨 Space + 不抢焦点） | `macos/capsule_window.rs` |
| macOS 本地化名称（NSLocale） | `macos/locale.rs` |
| Android JavaVM / Context / ndk-context 初始化 | `android/context.rs` |

## 迁移原则

1. **能力实现下沉**：新增平台能力一律进 `crates/openless-platform`，按平台分目录。
2. **宿主持有对象、平台层拿参数**：窗口句柄（`HWND` 以 `usize` 传递、`NSWindow*` 以裸指针传递）、
   JavaVM 由宿主提供，平台层只接受不透明参数——这也是为什么平台层的 Windows 接口用 `usize`
   而不是 `windows::Win32::Foundation::HWND`。
3. **注入优先于依赖**：平台层需要宿主信息时定义 `install_*` 钩子（`AndroidEnv`、`AsrPaths`、
   诊断钩子、前台应用探测、`AndroidContext`），不反向依赖宿主模块。
4. **显式失败**：不支持的平台返回明确错误（`unsupported`），不静默返回 `None`。
5. **窗口类替换与插件注册留在宿主**：它们依赖 tauri/tao 的内部布局与注册表，逐行放行并写明理由。
