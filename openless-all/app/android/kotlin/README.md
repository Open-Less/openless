# Android Kotlin scaffolding

Copy these files into `src-tauri/gen/android/` after running:

```bash
cd openless-all/app
npm run tauri:android:init
```

## Copy / merge paths

| Source (this folder) | Destination (after init) |
| --- | --- |
| `OpenLessOverlayService.kt` | `gen/android/app/src/main/java/com/openless/app/OpenLessOverlayService.kt` |
| `OverlayPermissionActivity.kt` | `gen/android/app/src/main/java/com/openless/app/OverlayPermissionActivity.kt` |
| `AndroidManifest.v1.snippet.xml` | 在 `../manifests/`，merge 进 `gen/android/.../AndroidManifest.xml` |
| `AndroidManifest.v3.snippet.xml` | 在 `../manifests/`，**future / not complete** — overlay v3 only |

Tauri `android init` generates the base manifest under `gen/android/app/src/main/AndroidManifest.xml`.
Merge the v1 snippet permissions into that file before building APK v1.

## Manifest snippets

- **v1** (`AndroidManifest.v1.snippet.xml`): `RECORD_AUDIO` and `MODIFY_AUDIO_SETTINGS` for in-app dictation — required for APK v1.
- **v3** (`AndroidManifest.v3.snippet.xml`): overlay + foreground service — **not complete / future**.

Do not treat v3 snippets as shipped; they document planned permissions and service entries only.
