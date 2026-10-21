# macOS 低内存模式：文字更正卡片关闭崩溃

日期：2026-10-18。作为 PR #1180 的独立修复提交。

## 问题与范围

手动修改听写结果后出现的“是否记住这个词”卡片仍由 Tauri/WebView 绘制。
它与独立 egui 录音胶囊的窗口实现不同。叉号调用 `reject_pending_correction`，
最后一条建议消失后，Coordinator 隐藏卡片，低内存策略销毁窗口及 WebView。
确认最后一条建议、建议到期和插入失败卡片关闭也使用相同的窗口生命周期。

`8918451a` 为临时卡片引入了“初始化普通窗口后执行 `object_setClass`”的做法。
AppKit 此时已经安装内部 KVO 观察者，换类会破坏原生对象的清理路径。
用户应用在 2026-10-18 21:28:43 的记录为主进程 SIGABRT。

在隔离旧实现中，首次叉号操作即在 `objc_exception_throw` 捕获：
`WKWindowVisibilityObserver` 无法为 `contentLayoutRect` 移除未注册的观察者。
堆栈依次经过 `stopObserving`、`viewWillMoveToWindow`、`removeFromSuperview`、
Wry `InnerWebView::drop`，确认异常发生在 WebView 脱离已换类窗口的销毁过程。

## 创建与释放方案

只在主线程同步创建临时卡片期间，使用作用域保护器替换 NSWindow 初始化函数。
匹配到首个尚未初始化的 `TaoWindow` 时，将其转换为兼容 NSPanel 子类，
调用真实 NSPanel 初始化函数，并从一开始设置 NonactivatingPanel 样式。
其余窗口类继续调用原初始化实现。

Tao 0.35.3 的窗口带有 `focusable` 实例字段，不能照搬无实例字段的 winit 适配：

- NSPanel 子类保留同类型、同偏移的 `focusable` 字段和 Tao 的 `sendEvent:` 实现
- 检查原类父类、字段集合、字段类型与偏移以及完整实例大小
- `canBecomeKeyWindow`、`canBecomeMainWindow` 返回 false，保持输入目标焦点
- 构建成功、返回错误或 Rust unwind 时都恢复原初始化函数
- 布局不兼容时保留普通窗口初始化路径，随后返回创建错误并销毁该窗口

初始化完成后不再换类、补装 KVO 或吞掉 Objective-C 异常。
临时卡片不进入 tauri-nspanel 的永久持有注册表。
关闭仍执行 hide + destroy，释放 WebView，保留主进程的托盘、Core 和热键。

普通模式和移动端继续使用各自原有窗口策略；本次不改变模型构建选项。

## 可重复的图形会话测试

`src-tauri/examples/card-lifecycle.rs` 使用真实的打包前端和生产窗口创建、关闭函数。
启动握手与词条数据由隔离用例提供，不启动用户后端、不写用户词库与设置。
循环从真实卡片按钮经过 Tauri IPC 关闭，或等待前端建议到期。

```sh
cd openless-all/app
npm run build
MACOSX_DEPLOYMENT_TARGET=14.0 CARGO_PROFILE_RELEASE_STRIP=none cargo build --locked --release \
  --manifest-path src-tauri/Cargo.toml --example card-lifecycle
python3 scripts/test-card-lifecycle-macos.py
# 首轮等待实际鼠标点击叉号，后续轮次自动进行：
python3 scripts/test-card-lifecycle-macos.py --manual-first
```

脚本先为整个临时 `.app` 签名并严格校验，再运行测试；不会替换已安装的应用。
九轮覆盖叉号、确认、超时各三次，检查同一宿主持续运行并能反复创建窗口，
用 Objective-C 弱引用检查九个 WKWebView 均已释放，最后检查普通窗口仍为 NSWindow。
调试旧实现时可指定旧 fixture 二进制并传入 `--debug-exception`，
在 `objc_exception_throw` 停止并输出异常和堆栈。

测试的原生句柄采集遵循 Tauri 2.11.2 的具体所有权实现：`WithWebview` 对 WebView、
controller、NSWindow 分别执行 `Retained::into_raw`，用例必须接管并释放三个 +1 引用。
对这些指针再执行 retain 会由测试本身制造残留。生产卡片使用的 `window.ns_window()`
则返回 autoreleased 借用指针，二者不能混用。

## 本机验证结果

macOS arm64 图形会话中，自动九轮及首轮实际点击叉号的九轮均以状态 0 通过。
每轮结束后宿主继续创建下一张卡片；两组各九个 WKWebView 的弱引用最终全部清空，
最后的普通窗口检查也通过。旧实现首次拒绝操作即捕获上述 KVO 异常。

TypeScript/Vite、原生 egui 辅助程序及完整本地 macOS App 构建通过；
最终 `.app` 的 `codesign --verify --deep --strict` 通过。
本地 App 使用此前已采用的 MLX 可选、Whisper CPU/Accelerate 构建配置，
PR 不携带这些本机构建配置改动。
