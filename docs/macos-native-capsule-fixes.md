# macOS 低内存胶囊：Dock 闪现与退出崩溃修复

日期：2026-10-12。关联 [PR #1180](https://github.com/Open-Less/openless/pull/1180)。

## 范围和已有工作

低内存模式的总体设计见 [desktop-low-memory-mode.md](desktop-low-memory-mode.md)。
本修复保留主窗口三秒关闭宽限期、胶囊按需启动/Idle 回收和设置页运行态提示，
仅调整 macOS 辅助程序的应用身份和原生窗口初始化。

低内存模式启动独立胶囊时，macOS Dock 短暂给图标
腾出位置，随后又收回。此前的诊断还发现，胶囊处理 Shutdown 时抛出
`NSRangeException`，退出状态为 SIGABRT。单纯追加 `effectiveAppearance`
观察者的修补仍会在 `_windowLayerContext` 的内部观察者清理处崩溃。

本地 ASR 依赖、MLX/Whisper Metal 默认构建配置和模型加载/卸载策略保持原状。
完整 macOS Host 构建仍需要原有原生子模块和 MetalToolchain。

## Dock 身份必须在 AppKit 注册之前确定

只在 winit EventLoopBuilder 上设置 Accessory 策略，不能覆盖启动早期的
LaunchServices 注册。本机此前的包内路径测试曾采到短暂的 Foreground 身份。

辅助程序现在自带 `Info.plist`，由其 `build.rs` 嵌入 Mach-O 的
`__TEXT,__info_plist`。声明独立的 `com.openless.native-capsule` bundle ID
和 `LSUIElement = true`。因此，即使执行文件位于主应用的
`OpenLess.app/Contents/MacOS` 中，也有自己的 agent 身份。辅助程序在入口
设置 Accessory，winit 继续使用相同策略。

该 metadata 只链接到辅助程序，不修改主应用的 Info.plist 或 Dock 行为。
Apple 对 [LSUIElement](https://developer.apple.com/documentation/bundleresources/information-property-list/lsuielement)
的定义是可显示界面、但不出现在 Dock 的 agent application。

## NSPanel 转换必须发生在初始化和 KVO 之前

原来在 eframe 的 CreationContext 回调里，对已初始化窗口执行
`object_setClass`。此时 AppKit 内部和 winit delegate 已安装 KVO 观察者；
换类破坏了观察者对应的运行时类，之后的析构无法正常清理。

新适配器只在辅助进程创建首个窗口期间，临时替换 NSWindow 的
`initWithContentRect:styleMask:backing:defer:` 实现：

1. 只匹配 `WinitWindow`；其他窗口调用原始初始化函数。
2. 检查 WinitWindow 的实例大小等于 NSWindow，且足以容纳 NSPanel。
3. 在初始化前转换为不允许成为 key/main window 的 NSPanel 子类。
4. 调用真正的 NSPanel 初始化实现，并在创建时传入 NonactivatingPanel 样式。
5. eframe 提供 CreationContext 后，立即恢复 NSWindow 的原始初始化函数。
6. 验证最终对象属于 NSPanel，再设置窗口层级和 Space 行为。

AppKit 与 winit 后续的 KVO 注册、外观通知和析构都作用于同一个初始化后的
panel，无需手动注册观察者或吞掉 Objective-C 异常。非激活样式用于 NSPanel，
参见 Apple 的 [nonactivatingPanel](https://developer.apple.com/documentation/appkit/nswindow/stylemask-swift.struct/nonactivatingpanel)。

这仍然是对 winit 窗口分配细节的原生适配。直接依赖固定为 `winit = 0.30.13`，
其 WinitWindow 没有实例变量。升级 winit/SDK 时必须复查初始化 ABI、类布局、
主题切换、焦点和退出行为；尺寸检查无法代替这些检查。布局不兼容时保留原
初始化函数，随后明确报告创建失败，不对已安装 KVO 的对象强行换类。

## 本机验证与复现

在 `openless-all/app` 中运行：

```sh
npm run build:native-capsule
cargo fmt --check -p openless-native-capsule
cargo clippy --locked -p openless-native-capsule -- -D warnings
cargo test --locked -p openless-capsule --features ui,host
python3 scripts/test-native-capsule-macos.py \
  --helper src-tauri/binaries/openless-native-capsule-aarch64-apple-darwin
npm run tauri build -- --bundles app
# 本地构建：为整个新 App 重建 ad-hoc 签名及资源清单。
codesign --force --deep --sign - src-tauri/target/release/bundle/macos/OpenLess.app
codesign --verify --deep --strict src-tauri/target/release/bundle/macos/OpenLess.app
```

脚本需要已登录的 macOS 图形会话。它构造声明为前台应用的临时 `.app`，将辅助
程序放在其 MacOS 目录，完成整个副本的本地签名及严格签名检查后才运行。
直接复制签名执行文件到未正确签名的包内可能产生“OpenLess 已损坏”系统弹窗，
因此不使用未签名的包内测试副本，也不改动 `/Applications/OpenLess.app`。

脚本覆盖 Classic、Typeless、Siri 三种样式和 Shutdown、管道 EOF、Idle 三种
结束方式，共九次独立启动。显式 Shutdown/Idle 测试保留 stdin，避免 EOF
掩盖消息处理错误。每次必须收到真实 Ready，采到 UIElement 身份，未采到
辅助程序成为前台应用，并以状态 0 在超时前退出。

| 检查 | 结果 |
| --- | --- |
| 原生辅助程序 release 构建 | 通过，macOS arm64 |
| 原生辅助程序 fmt / clippy `-D warnings` | 通过 |
| 共享胶囊 ui/host 单元测试 | 7 通过 |
| 已签名 `.app` 包内运行 | 独立 bundle ID，UIElement 身份 |
| 在 PR 最新提交上复跑三种样式、三种结束方式 | 九次启动均状态 0；242 次身份采样全为 UIElement；退出耗时 19–39 ms |
| 已签名测试包严格校验 | 每次启动前校验整个临时 `.app` 的签名及资源清单 |
| 本地 App 包内辅助程序回归 | 一次独立启动和 Shutdown 通过，31 次采样均为 UIElement |
| 完整 Tauri Host / 安装包 | 独立的本地云端 ASR 测试构建已完成；本 PR 默认 MLX/Whisper Metal 构建未复测 |

LaunchServices 采样可以检测身份回退，无法证明在所有系统版本上绝无极短瞬态。
全屏 Space、多屏、实际点击后的焦点和 Windows 真机仍待验证。

## 后台内存复核

2026-10-12，在 Apple Silicon macOS 上确认安装的主程序/胶囊构建 UUID 与包含
此修复的本地构建一致，低内存开关开启、所有窗口关闭、仅保留菜单栏后采样：

| 进程 | PID | phys_footprint |
| --- | --- | --- |
| OpenLess 主进程 | 17875 | 48 MB |
| 唯一剩余 WebKit Networking（保守计入） | 17879 | 5760 KB，约 5.6 MB |
| 合计 | | 约 54 MB |

未发现 WebContent、WebKit GPU 或原生胶囊进程。设置窗口三秒宽限期内重开保留
弹窗状态属于预期行为，不能单独作为 WebView 未回收的证据。

此快照使用云端 ASR，本地测试 Host 未启用 MLX/Whisper Metal 编译选项；该构建
策略不包含在本 PR 内。这里验证关闭窗口后的资源回收，不承诺 PR 默认构建或
启用本地模型后的具体内存值。普通模式约 310 MB 的历史快照是在主窗口和识别
状态开启时取得，窗口状态不同，不能据此声称同状态下精确节省约 250 MB。

本地 App 完成 ad-hoc 重签及严格校验后可用于测试；正式发行仍由原有流程执行
Developer ID 签名及公证。原生窗口初始化依赖当前 winit/SDK，升级后需重新验证。
