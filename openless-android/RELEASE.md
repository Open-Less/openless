# OpenLess Android 发布说明

## 构建目标

Android 版本号不单独维护，而是直接与桌面版 OpenLess 源码同步，并在以下文件之间做一致性校验：

- `openless-all/app/package.json`
- `openless-all/app/src-tauri/tauri.conf.json`
- `openless-all/app/src-tauri/Cargo.toml`

默认调试构建：

```powershell
cd openless-android
.\build.ps1
```

输出：

```text
build\OpenLessAndroid-debug.apk
```

## 可选的 Release 签名

`build.ps1` 支持显式传入 release 签名参数：

```powershell
.\build.ps1 `
  -Configuration release `
  -KeystorePath C:\path\to\release.keystore `
  -KeystoreAlias your_alias `
  -StorePass your_store_password `
  -KeyPass your_key_password
```

输出：

```text
build\OpenLessAndroid-release.apk
```

如果未提供 release 签名参数，脚本会退回本地 debug keystore 流程。

## 工具链前置要求

- Android SDK platform `android-34`
- Android build-tools `34.0.0`
- `aapt2`
- `d8`
- `zipalign`
- `apksigner`
- `keytool`

脚本会从 `ANDROID_HOME`、`ANDROID_SDK_ROOT` 或 `%LOCALAPPDATA%\\Android\\Sdk` 自动解析这些工具路径。

## 发布前检查

- 运行 `.\build.ps1`
- 运行 `.\verify.ps1`
- 确认 APK 已成功生成
- 执行 `QA_CHECKLIST.md`
- 执行 `STORE_SUBMISSION_CHECKLIST.md`
- 确认提供商配置页仍能持久化保存
- 在真机上验证悬浮触发器与 IME 流程

包含构建的校验命令：

```powershell
.\verify.ps1 -BuildFirst
```

直接打印同步后的版本元数据：

```powershell
.\version.ps1
```

## 当前限制

- 当前工作区还没有包含 `adb` 实机验证证据
- Play Store 元数据、商店截图、隐私文案、最终品牌素材仍属于独立发布任务
- 与桌面系统强绑定的能力，例如桌面级全局热键诊断、开机自启等，仍需要按 Android 语义单独实现或映射，不能机械照搬
