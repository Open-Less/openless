# OpenLess iOS 平台代码

iOS 相关 Rust、Swift 与前端代码的统一入口。桌面端通过 `#[cfg(not(mobile))]` 分层，不受影响。

## 目录结构

```text
ios/
├── swift/               # Swift 模板（CI 复制到 gen/apple/Sources/，M3 键盘扩展）
├── manifests/           # Info.plist 片段（如需独立维护）
└── frontend/            # React 模块（Vite 别名 @ios）

src-tauri/src/ios/       # Rust 运行时模块（crate::ios）
```

## Rust（`src-tauri/src/ios/`）

| 模块 | 职责 |
|------|------|
| `clipboard.rs` | UIPasteboard 写入（App 内听写结果的复制兜底） |
| `keyboard_status.rs` | 检测键盘扩展是否在系统设置中启用（M3 设置页用） |

麦克风权限不走本目录：`permissions.rs` 的 iOS `platform` 模块直接对接
`AVAudioSession`，复用桌面已有的 `check_microphone_permission` /
`request_microphone_permission` IPC 面。

## 平台配置（`src-tauri/tauri.ios.conf.json`）

tauri-cli 2.10.1 的 `strip_semver_prerelease_tag` 在版本同时含带点 prerelease
（`-Beta.2`）与 build 元数据（`+build.YYYYMMDD`）时会拼出非法 build 段
（上游 bug：`{prefix}{number}` 顺序写反）。iOS 平台配置覆盖 `version` 为纯
X.Y.Z——这同时满足 Apple 对 CFBundleShortVersionString「三段纯数字」的硬性
要求。**版本号必须与 tauri.conf.json 的 X.Y.Z 前缀保持一致**（bump-version.sh
与 CI 门禁同步）。

## 脚本链

```bash
cd openless-all/app
npm run tauri:ios:init          # 生成 src-tauri/gen/apple（gitignored）
npm run copy:ios-scaffolding    # patch project.yml + 复制 Swift 模板 + 重跑 xcodegen
npm run tauri:ios:dev           # 模拟器/真机开发
npm run tauri:ios:build         # 构建（签名需在 Xcode 配置 Team）
```

`copy-ios-scaffolding.mjs` 做三件事：

1. 部署目标 14.0 → 15.0（Xcode 27 的 iOS SDK 拒绝 < 15 的部署目标）；
2. Info.plist 插入 `NSMicrophoneUsageDescription`；
3. 重跑 `xcodegen generate`（xcodegen 由 tauri ios init 依赖，brew 安装）。

## 本机开发前置

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
brew install xcodegen cocoapods
xcodebuild -downloadComponent MetalToolchain   # Xcode 27 拆分的组件，桌面构建需要
```

## 真机安装

签名模式由环境变量控制，`copy:ios-scaffolding` 可在两种模式间互相切换（幂等）：

```bash
# 付费账号（完整功能，含键盘扩展）：
OPENLESS_IOS_DEVELOPMENT_TEAM=<TeamID> npm run copy:ios-scaffolding
npm run tauri -- ios build --target aarch64 --export-method debugging
xcrun devicectl device install app --device <UDID> <导出的.ipa>

# 免费 Apple ID（无键盘变体；免费签名不支持 App Group）：
rm -rf src-tauri/gen/apple && npm run tauri -- ios init
OPENLESS_IOS_NO_KEYBOARD=1 npm run copy:ios-scaffolding
# 免费 iPhone 安装走 Xcode GUI（Xcode 会做 free provisioning）：
#   npm run tauri -- ios build --target aarch64 --open  ← 先启动保持 CLI 存活
#   然后在 Xcode 里选真机 Run；7 天有效期，过期重跑
```

注意：
- Xcode GUI 直接 Run 依赖存活的 tauri CLI（RPC server）；CLI 未运行时
  「Build Rust Code」阶段必然失败。`--open` 流程会先拉起 CLI。
- 构建脚本已自举 PATH（homebrew npm + ~/.cargo/bin），Dock 启动的
  Xcode 也能正确执行。
- 手机端首次运行：设置 → 通用 → VPN与设备管理 信任证书；iOS 16+ 需
  开启开发者模式。

## 键盘扩展（M3 v1）

结构：`ios/swift/KeyboardExtension/`（Swift 源，复制到 `gen/apple/KeyboardExtension/`）
+ project.yml 注入的 `OpenLessKeyboard` 扩展 target（App Group entitlement、
`RequestsOpenAccess`）。主 App 设置页写入 App Group 的 `kb-config.json`，
键盘读取后直连 OpenAI 兼容 `/audio/transcriptions` 转写并插入光标。

手动验证（模拟器/真机，均需手动操作）：

1. 构建安装后：系统设置 → 通用 → 键盘 → 键盘 → 添加新键盘 → OpenLess；
2. 进入扩展设置开启「允许完全访问」（网络 + 麦克风必需）；
3. 打开任意可输入应用（如备忘录），切到 OpenLess 键盘；
4. 主 App 设置 → 权限 → 键盘转写配置里填端点/Key/模型并保存；
5. 按住麦克风说话，松开后转写文本应插入输入框。

v1 限制：apiKey 明文存于 App Group JSON（真机分发需换 Keychain access
group）；无润色/纠错管线（Rust C ABI 桥为 M3+）；仅支持 OpenAI 兼容转写
端点。

## CI

Workflow：`.github/workflows/ci.yml` 的 `ios-check` job（aarch64-apple-ios
cargo check）；`.github/workflows/ios-build.yml`（M4）做无签名模拟器构建产物。

## 相关文档

- [AGENTS.md](../../AGENTS.md) — 排查约定
- [docs/architecture.md](../../docs/architecture.md) — 分层架构
