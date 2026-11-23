# openless-platform

平台能力层：所有与操作系统打交道的实现都收在这里，宿主只做 UI、IPC 与进程模型。

## 分层

```
openless-core        端口 trait + 业务规则（不含平台实现）
      ↑ 只依赖类型
openless-platform    平台后端（本 crate，自成 workspace）
      ↑
宿主                  src-tauri（macOS / Windows / Android）、Harmony
```

约定：

1. 宿主**不得**直接依赖 `arboard` / `enigo` / Win32 / WinRT，只能调用本 crate 的门面；
2. 本 crate 不依赖任何宿主框架（tauri / egui），保持可单独测试；
3. 目录**按平台分**，每个平台下按能力分文件；
4. 能力之间**只通过门面组合**，不共享内部状态。

## 可选编译模块（类 Kconfig）

**能力**是 Cargo feature，**平台**由 `target_os` 自动选择。默认全开：

```bash
cargo build                                    # 全部能力
cargo build --no-default-features --features clipboard,text   # 只编剪贴板与文本写入
cargo test  --no-default-features --features storage          # 只测存储模块
```

| feature | 内容 | 依赖 |
|---|---|---|
| `clipboard` | 全格式快照 / 历史排除 / 历史清理（Windows / macOS / Android） | arboard |
| `input` | 按键注入 + 全局热键 + 侧键组合 + 组合键 + 鼠标侧键 + macOS Fn | global-hotkey, parking_lot, once_cell |
| `text` | 逐字输入 + 模拟粘贴 | → `input` |
| `selection` | 选区模拟复制通道 | → `clipboard`, `input` |
| `audio` | 采集（设备/输入流/回环参考）+ 静音 + 提示音 | cpal, coreaudio-sys |
| `permissions` | TCC / 注册表 / Android 运行时权限 | serde, cpal, winreg |
| `storage` | 原子文件存储 + 平台加固 | uuid, libc |
| `credentials` | 保险库原语（Windows CredMan / macOS Keychain） | keyring, security-framework, zeroize, anyhow |
| `asr` | 本地 ASR 元数据（provider id / 模型收据 / sherpa 描述 / 诊断钩子） | anyhow, serde, serde_json, uuid |
| `asr-sherpa` | sherpa-onnx 原生运行时（仅 Windows / Android arm64） | sherpa-onnx, tokio, parking_lot |
| `asr-apple` | Apple Speech（仅 macOS） | objc2, block2, futures-util, parking_lot |
| `asr-foundry` | Foundry Local（Windows WinML） | foundry-local-sdk, zip, tokio, uuid |
| `ime` | Windows TSF 通道（提交 / 选区读取） | serde, tokio, parking_lot |
| `ime-profile` | Windows IME 语言配置 | winreg |
| `diagnostics-1266` | issue-1266 观测（宿主 feature 转发） | — |

feature 之间的依赖写在 `Cargo.toml` 里，关闭底层能力会连带关掉依赖它的能力，
不会留下悬空引用。

## 目录

```
src/
  lib.rs              门面导出
  clipboard.rs        剪贴板：Intent / Snapshot / Clipboard
  text.rs             文本写入：send_paste_shortcut / type_text
  ime/gate.rs         文本通道安全门（Secure Input / 密码框 / 敏感应用黑名单）
  macos/
    ax.rs             macOS AX 文档读写（选区/光标上下文/编辑监听）
    front_app.rs      NSWorkspace 前台应用身份（含按 pid 查 bundle）
    secure_input.rs   IsSecureEventInputEnabled
    keystroke.rs      CGEvent 逐字投递 + TIS 输入源切换（主线程原语）
    guide.rs          辅助功能授权引导窗口（AppKit 在 guide.m）
  common/
    uia_text.rs       UIA 字符单元解码（纯逻辑）
  ime/
    protocol.rs       TSF 线协议（tag/状态码/payload/共享内存布局）
    target.rs         前台焦点线程与窗口捕获
    ipc.rs            WM_COPYDATA 提交与查询轮询
    session.rs        插入会话状态机
  windows/
    front_app.rs      Windows 前台应用（窗口标题）+ 窗口激活
  selection/
    target.rs           选区插入目标（Windows 焦点指纹 / macOS 前台应用）+ 复核结果
    windows_target.rs   Windows 焦点指纹（前台/焦点 HWND + pid/tid）
  input/
    mod.rs            输入能力门面
    inject.rs         按键注入：send_copy_shortcut
    hotkey/           全局热键监听（macos.rs / windows.rs / unsupported.rs）
    side_combo/       侧键组合（物理左右修饰键 + 伴随键）
      keys.rs         快捷键解析（主键 Code、组合 HotKey）
    combo.rs          自定义组合键听写
    global_runtime.rs global-hotkey 进程级共享运行时
    macos_dictation_key.rs  macOS Fn 听写键
    mouse.rs          Mouse4/5 侧键听写
  selection.rs        选区模拟复制：capture_via_simulated_copy / CaptureOutcome
  asr/
    ids.rs            provider id 契约 + 判定
    receipt.rs        原生模型文件收据（不初始化 SDK）
    descriptors.rs    sherpa 运行时阶段/事件负载（纯逻辑）
    diagnostics.rs    诊断事件钩子（宿主注册，引擎上报）
    sherpa/           sherpa-onnx 原生运行时（runtime / provider / blocking_decode）
    foundry/          Foundry Local（Windows WinML）真·运行时（runtime / native / provider）
    paths.rs          宿主注入的模型缓存/运行时/日志目录
    apple.rs          Apple Speech（macOS Speech.framework）
  audio/
    mute.rs           录音期间系统静音（含 mute/{windows,macos,unsupported}.rs）
    cue.rs            合成"开始录音"提示音（cpal，桌面平台）
    capture/          采集：设备枚举、输入流、回环参考
      mod.rs          门面：CaptureError / StreamFormat / SampleSink / classify_stream_error
      backend.rs      cpal 输入流（Windows/Linux）+ Windows WASAPI 回环
      android_capture.rs  Android AudioRecord + AEC/降噪/AGC（48 kHz 单声道）
      device_watch.rs   macOS 设备变更通知（CoreAudio 属性监听 + 常驻 runloop）
      macos_tap.rs    macOS 系统音频 tap（Core Audio process tap，14.6+）
      macos_tap.m     tap 的 Objective-C 部分（build.rs 编译）
  credentials/
    keychain.rs       macOS Keychain 通用密码项（复用 item 引用，避免重复授权）
    store.rs          统一保险库原语：Windows 凭据管理器 / macOS 钥匙串 / 其余显式不可用
  ime/gate.rs         文本通道安全门（Secure Input / 密码框 / 敏感应用黑名单）
  macos/
    ax.rs             macOS AX 文档读写（选区/光标上下文/编辑监听）
    front_app.rs      NSWorkspace 前台应用身份（含按 pid 查 bundle）
    secure_input.rs   IsSecureEventInputEnabled
    keystroke.rs      CGEvent 逐字投递 + TIS 输入源切换（主线程原语）
    guide.rs          辅助功能授权引导窗口（AppKit 在 guide.m）
  common/
    codec.rs          跨平台纯编解码
    error.rs          PlatformError（部分失败语义）
  windows/
    clipboard.rs      全格式快照 + 历史排除标记
    ime_profile.rs    IME 语言配置（注册表）
    history.rs        WinRT 剪贴板历史清理
    input.rs          SendInput 底层（键组合 / Unicode 码元）
    selection.rs      模拟 Ctrl+C + 剪贴板事务
    text.rs           逐字输入 + 粘贴快捷键
  macos/
    cgevent.rs        CGEvent 底层（Cmd+C / Cmd+V）
    clipboard.rs      纯文本通道（全格式快照待补）
    selection.rs      哨兵 + Cmd+C
    text.rs           Cmd+V（逐字输入仍在宿主，涉及 TIS）
  unsupported/        其余平台的显式失败后端
```

## 能力门面

```rust
// 剪贴板
Clipboard::set_text(text, Intent::Internal)  // 内部传参 → Windows 不进剪贴板历史
Clipboard::snapshot() / restore(&snapshot)   // 不透明快照 + 事务还原
Clipboard::read_text()                       // Ok(None)=无文本，Err(Busy)=打不开
Clipboard::sequence_number()                 // 仅 Windows

// 文本写入
text::send_paste_shortcut(PasteShortcut)     // Windows: SendInput；macOS: Cmd+V
text::type_text(text, newline_mode)          // Windows: SendInput 逐字 Unicode

// 输入
input::send_copy_shortcut()                  // Windows: Ctrl+C；macOS: Cmd+C
input::hotkey::HotkeyMonitor::start(...)     // 全局热键监听
input::hotkey::install_host_hooks(...)       // 注册宿主行为钩子（4 个）
input::combo::ComboHotkeyMonitor             // 自定义组合键
input::mouse::MouseDictationMonitor          // Mouse4/5
input::macos_dictation_key::Monitor          // macOS Fn
input::side_combo::SideAwareComboMonitor     // 侧键组合
input::side_combo::keys::parse_primary()     // 快捷键主键解析

// 选区
selection::capture_via_simulated_copy() -> CaptureOutcome

// 音频
audio::AudioMuteGuard::activate()            // 录音期间静音，drop 恢复
audio::play_start() / audio::stop_cue()      // 提示音

// 凭据（macOS）
credentials::keychain::read/write(service, account)
```

## 未迁入（有意保留在宿主）

| 能力 | 原因 |
|---|---|
| 无（采集接线与 AEC 回采已迁入） | — |
| TSF / UIA / AX 选区读取 | 需要宿主自己的窗口与焦点上下文 |
| 无 | 热键、权限、平台存储均已迁入 |
| Windows/Android 凭据 | `keyring` 已是跨平台抽象；Android 需要 Context |

## 关于 workspace

本 crate 自成 workspace（`[workspace]` 空表）。原因：`src-tauri` 用的是打过补丁的
cpal 0.15.3（`links = "alsa"` 经 alsa-sys 0.3.1），而 app 主 workspace 里的 linux-egui
用 cpal 0.18（alsa-sys 0.4），两者无法在同一解析图共存。这与 `src-tauri` 被
exclude 出主 workspace 是同一个理由。

### Android 目标的交叉检查

本机没有 Android Studio，但有 NDK，可直接对平台层做类型检查：

```bash
NDK=/data-win/openless-cache/mobile-rime/android-sdk/android-ndk-r26b
export CC_aarch64_linux_android=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang
export AR_aarch64_linux_android=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$CC_aarch64_linux_android
cargo check --target aarch64-linux-android --no-default-features --features android
```
