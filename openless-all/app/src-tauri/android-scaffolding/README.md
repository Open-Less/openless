# Android Kotlin scaffolding

Copy these files into `src-tauri/gen/android/` after running:

```bash
cd openless-all/app
npm run tauri:android:init
```

## Copy / merge paths

| Source (this folder) | Destination (after init) |
| --- | --- |
| `OpenLessImeService.kt` | `gen/android/app/src/main/java/com/openless/app/OpenLessImeService.kt` |
| `OpenLessOverlayService.kt` | `gen/android/app/src/main/java/com/openless/app/OpenLessOverlayService.kt` |
| `OverlayPermissionActivity.kt` | `gen/android/app/src/main/java/com/openless/app/OverlayPermissionActivity.kt` |
| `AndroidManifest.v1.snippet.xml` | merge into `gen/android/app/src/main/AndroidManifest.xml` |
| `AndroidManifest.v2.snippet.xml` | **future / not complete** — IME v2 only |
| `AndroidManifest.v3.snippet.xml` | **future / not complete** — overlay v3 only |

Tauri `android init` generates the base manifest under `gen/android/app/src/main/AndroidManifest.xml`.
Merge the v1 snippet permissions into that file before building APK v1.

## Manifest snippets

- **v1** (`AndroidManifest.v1.snippet.xml`): `RECORD_AUDIO` and `MODIFY_AUDIO_SETTINGS` for in-app dictation — required for APK v1.
- **v2** (`AndroidManifest.v2.snippet.xml`): IME service declaration — **not complete / future**.
- **v3** (`AndroidManifest.v3.snippet.xml`): overlay + foreground service — **not complete / future**.

Do not treat v2 or v3 snippets as shipped; they document planned permissions and service entries only.
