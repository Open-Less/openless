# Android Kotlin 脚手架

[English](README.md) | [简体中文](README.zh.md)

运行以下命令后，将这些文件复制到 `src-tauri/gen/android/`：

```bash
cd openless-all/app
npm run tauri:android:init
```

## 复制 / 合并路径

| 源文件（本目录） | 目标位置（init 之后） |
| --- | --- |
| `OpenLessOverlayService.kt` | `gen/android/app/src/main/java/com/openless/app/OpenLessOverlayService.kt` |
| `OverlayPermissionActivity.kt` | `gen/android/app/src/main/java/com/openless/app/OverlayPermissionActivity.kt` |
| `AndroidManifest.v1.snippet.xml` | 位于 `../manifests/` 中；合并进 `gen/android/.../AndroidManifest.xml` |
| `AndroidManifest.v3.snippet.xml` | 位于 `../manifests/` 中；**future / not complete** —— 仅 overlay v3 |

Tauri `android init` 会在 `gen/android/app/src/main/AndroidManifest.xml` 下生成基础清单。
构建 APK v1 之前，请把 v1 片段中的权限合并进该文件。

## 清单片段

- **v1**（`AndroidManifest.v1.snippet.xml`）：用于应用内听写的 `RECORD_AUDIO` 与 `MODIFY_AUDIO_SETTINGS` —— APK v1 必需。
- **v3**（`AndroidManifest.v3.snippet.xml`）：overlay + 前台服务 —— **not complete / future**。

不要把 v3 片段当作已发布内容；它们只记录计划中的权限与服务条目。
