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
| `capsule` | 胶囊几何 + 窗口控制（不抢焦点 / 点击穿透 / 命中测试） | objc2 |
| `updater` | 自更新发现 + 验签（桌面与 Android 共用） | semver, reqwest, tokio, base64, minisign-verify |
| `updater-android` | 兼容别名（已并入 `updater`） | — |
| `android` | Android JNI 通道 + 无障碍 / Shizuku / 悬浮层 | jni, tokio, updater |
| `diagnostics-1266` | issue-1266 观测（宿主 feature 转发） | — |

feature 之间的依赖写在 `Cargo.toml` 里，关闭底层能力会连带关掉依赖它的能力，
不会留下悬空引用。

## 目录

```
src/
  android/
    accessibility.rs   无障碍服务通道
    clipboard.rs   剪贴板（Java ClipboardManager）
    context.rs   JavaVM / Activity context 注入点
    edit_observation.rs   编辑观察
    input.rs   按键注入（Android）
    insert_tiers.rs   插入策略分级
    jni.rs   JNI 助手与 Java 类访问
    local_migration.rs   本地数据迁移
    mod.rs   Android 后端 + AndroidEnv 接缝
    overlay_control.rs   悬浮层 Intent 控制
    overlay.rs   悬浮层权限与状态
    permissions.rs   运行时权限
    selection.rs   选区读取（无障碍）
    shizuku.rs   Shizuku 授权与注入
    text.rs   文本写入（无障碍 / Shizuku）
  asr/
    apple.rs   Apple Speech（macOS）
    descriptors.rs   sherpa 运行时阶段/事件
    diagnostics.rs   诊断事件钩子
    foundry/
      descriptors.rs   provider id 与阶段
      mod.rs   Foundry Local
      native.rs   运行时下载 / 校验 / 安装
      provider.rs   Whisper 分块推理
      runtime.rs   模型路由与 GPU→CPU 回退
  asr/
    ids.rs   provider id 契约
    mod.rs   本地 ASR 门面 + PcmConsumer
    paths.rs   宿主注入的目录解析器
    receipt.rs   模型文件收据
    sherpa/
      blocking_decode.rs   串行化解码 worker
      mod.rs   sherpa-onnx 运行时
      provider.rs   sherpa provider
      runtime.rs   sherpa 离线/在线识别器
    capture/
      android_capture.rs   Android AudioRecord + AEC/降噪/AGC
      backend.rs   cpal 输入流 + Windows 回环
      macos_tap.m   tap 的 Objective-C 实现
      macos_tap.rs   系统音频 tap（14.6+）
      mod.rs   采集门面（CaptureError / StreamFormat / Sink）
  audio/
    cue.rs   合成提示音
    device_watch.rs
    mod.rs   音频能力门面
    mute/
      macos.rs   macOS 静音
  audio/
    mute.rs   录音期间系统静音
    mute/
      unsupported.rs   其它平台显式不可用
      windows.rs   Windows 静音
  capsule/
    geometry.rs   显示器几何 / 放置 / 镜像
    mod.rs   胶囊能力门面
  clipboard.rs   剪贴板门面：Intent / Snapshot / 全格式快照
  common/
    block_reason.rs   文档读取被拒原因
    codec.rs   UTF-16 分块编解码（纯逻辑）
    error.rs   PlatformError（部分失败语义）
    geometry.rs   Rect / UiaAnchor（跨能力共用）
    issue1266.rs   issue-1266 诊断观测
    mod.rs   跨平台纯逻辑
    uia_text.rs   UIA 字符单元解码（纯逻辑）
  credentials/
    keychain.rs   macOS Keychain
    mod.rs   凭据库门面
    store.rs   统一保险库原语
  ime/
    gate.rs   文本通道安全门
    ipc.rs   WM_COPYDATA 提交与查询
    mod.rs   TSF 通道 + 共享恢复助手
    protocol.rs   TSF 线协议
    session.rs   插入会话状态机
    target.rs   前台焦点捕获
    uia/
      edit_settle.rs   编辑事件收敛
  ime/
    uia.rs   UIA 文档读写
  input/
    combo.rs   自定义组合键听写
    global_runtime.rs   global-hotkey 共享运行时
    hotkey/
      macos.rs   CGEventTap 后端
      mod.rs   全局热键
      unsupported.rs   无后端平台的显式失败
      windows.rs   WH_KEYBOARD_LL 后端
  input/
    inject.rs   按键注入抽象
    macos_dictation_key.rs   Fn 听写键（macOS）
    mod.rs   输入能力门面 + HostHooks
    mouse.rs   鼠标侧键听写
    side_combo/
      keys.rs   快捷键解析
      macos.rs   macOS 侧键组合
      mod.rs   侧键组合
      windows.rs   Windows 侧键组合
  lib.rs   能力门面导出 + feature 门控
  macos/
    ax.rs   AX 文档读写（选区 / 光标上下文 / 编辑监听）
    capsule_window.rs   交互区域命中 / 点击穿透 / 鼠标状态
    cgevent.rs   CGEvent 按键注入
    clipboard.rs   纯文本剪贴板（arboard）
    front_app.rs   NSWorkspace 前台应用 + 激活
    guide.m   引导窗口 AppKit 实现
    guide.rs   辅助功能授权引导窗口
    keystroke.rs   CGEvent 逐字投递 + TIS 输入源切换
    mod.rs   macOS 后端
    secure_input.rs   Secure Event Input 状态
    selection.rs   哨兵 + Cmd+C 读取选区
    text.rs   模拟粘贴快捷键（CGEvent）
    window.rs   窗口置前（带异常兜底）
  permissions/
    macos.rs   TCC（辅助功能 / 麦克风）
    mod.rs   权限探测门面
    other.rs   注册表 / Android 运行时权限
  selection.rs   选区读取门面：CaptureOutcome
  selection/
    target.rs
    windows_target.rs
  storage/
    mod.rs   原子文件存储
    secure_file.rs   平台加固的小文件写入
  text.rs   文本写入门面：type_text / 粘贴快捷键
  unsupported/
    clipboard.rs   剪贴板不可用
    input.rs   输入不可用
    mod.rs   无实现平台的显式失败
    selection.rs   选区读取不可用
    text.rs   文本写入不可用
  updater/
    android_logic.rs   版本比较 / ABI / 验签
    android_pipeline.rs   Android 更新流水线
    android.rs   Android APK 发现 / 传输
    artifact_fallback.rs   产物地址回退
    manifest.rs   清单与并发发现
    mod.rs   自更新门面
    signature_index.rs   签名索引
  windows/
    capsule_window.rs   命中测试 / 点击穿透 / 光标 / 前台窗口
    clipboard.rs   全格式快照 + 历史排除
    front_app.rs   前台应用（窗口标题）与窗口激活
    history.rs   WinRT 历史清理
    ime_profile.rs   IME 语言配置（注册表）
    input.rs   SendInput 底层
    mod.rs   Windows 后端
    selection.rs   模拟复制读取选区
    shell.rs   打开 URI / 文件管理器 / 设置页
    text.rs   逐字输入与模拟粘贴
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

边界由 `scripts/check-platform-boundary.sh` + `src-tauri/PLATFORM_BOUNDARY.md` 强制。

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
