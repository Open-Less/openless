# Releasing OpenLess

[English](RELEASING.md) | [简体中文](RELEASING.zh.md)

This checklist defines who may cut releases, how version numbers and packages stay consistent, and how release artifacts are verified and published. It is maintainer release policy rather than a day-to-day development guide (see [openless-all/README.md](openless-all/README.md) for local builds).

## 1. Release Authority

**Only repository administrators may create release tags and publish releases.**

- Contributors and AI agents **must not** create release tags (`v*-tauri`), publish GitHub Releases, or trigger release workflows. Request an admin with the target version and channel (`Beta` or `Stable`).
- Release automation is **admin-triggered only** (tag-triggered pipelines count as admin-triggered because only admins can push release tags).
- Linux packages may be attached to an existing release tag only after exact-candidate device and installation acceptance is complete. Desktop, Android, and HarmonyOS tags never wait for Linux.

## 2. Channels, Branches & Tag Formats

| Channel | Branch | Tag format | Publication |
| --- | --- | --- | --- |
| **Beta** | `beta` (default branch) | `v<X.Y.Z>-Beta.<N>-tauri` (e.g. `v2.0.0-Beta.2-tauri`) | GitHub pre-release; never auto-updates Stable users |
| **Stable** | `main` (maintainer merge from `beta`) | `v<X.Y.Z>-tauri` | GitHub latest release |

- **Dated build metadata**: Synchronized application versions may include SemVer build metadata using the `build.` prefix (e.g. `2.0.0-Beta.2+build.20260924`), while keeping the Git tag in `v2.0.0-Beta.2-tauri` format (released clients match numeric `Beta.<N>` tags; Tauri 2.10.1 NSIS maps bare numeric metadata into a 16-bit Windows product-version field). Build metadata does not advance SemVer precedence — increment `N` for every public Beta.
- **Legacy suffix**: The updater still recognizes historical `*-beta-tauri` tags, but all new Beta releases use `Beta.<N>`.

## 3. Version-Sync Gate (5 Files)

CI fails a Tauri release unless **all five** files carry the exact same version. For plain `X.Y.Z`, bump them together with `scripts/bump-version.sh <X.Y.Z>`; for prerelease versions (`X.Y.Z-Beta.N`), update all five files by hand:

1. `openless-all/app/package.json`
2. `openless-all/app/package-lock.json` (root and nested `packages.""`)
3. `openless-all/app/src-tauri/tauri.conf.json`
4. `openless-all/app/src-tauri/Cargo.toml`
5. `openless-all/app/src-tauri/Cargo.lock` (the `name = "openless"` package entry)

> `openless-all/app/Cargo.lock` belongs only to the framework-independent Core/Linux workspace and is not part of the five-file Tauri version-sync gate.

### License Boundary

- Published `1.x` releases remain **MIT**.
- `2.0.0-Beta.1` onward uses **`AGPL-3.0-only`** ([LICENSE](LICENSE)); vendored third-party libraries and fonts retain their upstream licenses (see [docs/ATTRIBUTION.md](docs/ATTRIBUTION.md)).

## 4. Unified Release Pipeline & Download Assets

Pushing a `v*-tauri` tag triggers a single entry point: `.github/workflows/release-tauri.yml` (**Release (all platforms)**).

1. **Preflight** creates or validates a shared GitHub Release **draft**.
2. **macOS, Windows, Android (all 4 ABIs), and HarmonyOS** build in parallel from the same immutable commit via reusable workflows and attach assets to the shared draft.
3. **Standalone / repair runs** remain available for Android weekly/debug builds, Windows draft repairs, and HarmonyOS manual dispatches.

### Asset Naming & Updater Manifests

- **No duplicate aliases in new releases**: Publish only the original versioned macOS DMGs, Windows installers, Android APKs, macOS `.app.tar.gz` updater archives, `updater-signatures.json`, and `OpenLess_harmony_arm64.hap` + `SHA256SUMS`. Do not generate or attach legacy alias names (`OpenLess_macos_*`, `OpenLess_windows_*`, `OpenLess_android_*`, `OpenLess_harmony.hap`). Existing historical releases are left untouched.
- **Updater manifests live on CDN, not in GitHub Release assets**:
  - Stable: `https://dl.aeoform.com/updates/stable/latest-{target}-{arch}.json`
  - Beta: `https://dl.aeoform.com/updates/beta/latest-{target}-{arch}-beta.json`
  - The workflows emit manifests as run artifacts (deliberately excluded from GitHub Release assets). After verifying the draft, upload the generated manifests and release binaries to CDN.
- **Website / download links**: Resolve actual asset names via the GitHub Releases API (Stable uses latest stable release; Beta explicitly targets its prerelease tag because GitHub `latest` excludes prereleases).

### Linux (Paused Automation)

- Linux CI and automatic release attachment are currently paused. Linux source and local build/packaging scripts remain maintained.
- Before manually distributing or attaching Linux packages (`.deb` / `.rpm`), increment `openless-all/linux/egui/package-revision` (e.g. `2.0.0-Beta.2-79` → `2.0.0-Beta.2-80`) and complete exact-candidate on-device installation and dictation acceptance. Linux has no AppImage or in-app updater.

## 5. HarmonyOS Packaging, Signing & AppGallery Connect

HarmonyOS builds via `.github/workflows/release-harmony.yml` using the publicly mirrored HarmonyOS command-line tools (`../harmony/scripts/prepare-harmony-sdk.sh`, SHA-256 verified and cached via `actions/cache`; override with `HARMONY_SDK_URL` / `HARMONY_SDK_SHA256` or a `["self-hosted","harmony"]` runner with `HARMONY_TOOLCHAIN`). Never commit the SDK archive.

### Channel Matrix

| `channel` input | Artifact | Signing material | Destination |
| --- | --- | --- | --- |
| `none` (default) | `OpenLess_harmony_arm64.hap` | **Unsigned** | GitHub Release + `SHA256SUMS` |
| `release` | `OpenLess_harmony_arm64-signed.hap` | `release.{cer,p7b,p12}` | AppGallery Connect production candidate only (never GitHub Release) |
| `test` | `OpenLess_harmony_arm64-signed.hap` | `test.{cer,p7b,p12}` | AppGallery Connect test channel only |

- Automatic calls from `release-tauri.yml` pass `channel: ${{ vars.HARMONY_AUTO_CHANNEL || 'none' }}`. Set repository variable `HARMONY_AUTO_CHANNEL` to `release` or `test` to upload a signed candidate package to AGC automatically; store submission for review remains a manual human step.
- `ci.yml` always builds unsigned (`OPENLESS_HARMONY_CHANNEL=none`). Locally verify an unsigned package with `../harmony/scripts/verify-harmony-hap.sh <hap> <toolchain> <name> <code> --allow-unsigned`.
- `openless-all/harmony/AppScope/app.json5` keeps placeholder `versionName`/`versionCode`; `../harmony/scripts/package-harmony-hap.sh` temporarily injects `versionName` from `package.json` and `versionCode` from `../linux/egui/package-revision` during build and restores the file afterward.
- The signing step exports `OPENLESS_HARMONY_SIGNING_DIR` via `$GITHUB_ENV` and validates signing material via `../harmony/scripts/check-harmony-signing.sh` before compiling (never declare `OPENLESS_HARMONY_SIGNING_DIR` as a job-level `env:` key).

### Required Repository Secrets & Variables (HarmonyOS)

| Name | Purpose |
| --- | --- |
| `HARMONY_AUTO_CHANNEL` (var) | Optional auto-upload channel (`none`, `release`, or `test`; defaults to `none`) |
| `HARMONY_KEYSTORE_BASE64` | Base64-encoded `.p12` keystore (or channel-specific `HARMONY_<CHANNEL>_KEYSTORE_BASE64`) |
| `HARMONY_CERT_BASE64` | Base64-encoded `.cer` certificate (or `HARMONY_<CHANNEL>_CERT_BASE64`) |
| `HARMONY_PROFILE_BASE64` | Base64-encoded `.p7b` profile (or `HARMONY_<CHANNEL>_PROFILE_BASE64`) |
| `HARMONY_KEYSTORE_PASSWORD` | Keystore password (or `HARMONY_<CHANNEL>_KEYSTORE_PASSWORD`; `HARMONY_KEY_PASSWORD` falls back to this) |
| `HARMONY_KEY_ALIAS` | Signing key alias (or `HARMONY_<CHANNEL>_KEY_ALIAS`) |
| `APPGALLERY_CLIENT_ID` | HUAWEI AppGallery Connect API client ID |
| `APPGALLERY_CLIENT_SECRET` | HUAWEI AppGallery Connect API client secret |
| `APPGALLERY_APP_ID` | HUAWEI AppGallery Connect app ID |

Without `APPGALLERY_*` secrets, the workflow still builds and attaches the unsigned HAP to the GitHub Release and skips store upload.

## 6. Pre-Release Checklist (Admin)

1. Confirm branch matches target channel (`beta` for Beta, `main` for Stable via maintainer merge from `beta`).
2. Confirm all five version files match (version-sync gate green).
3. Confirm CI is green on the exact commit to be tagged.
4. Smoke-test an installed candidate on affected platforms: launch, permissions, recording, cancellation, text insertion/fallback, upgrade path, and newly changed user flows.
5. Push the release tag (`v<X.Y.Z>-tauri` or `v<X.Y.Z>-Beta.<N>-tauri`).
6. Wait for all required jobs in `Release (all platforms)` to finish and inspect the shared draft:
   - Verify expected installers, APKs, unsigned HarmonyOS HAP, `updater-signatures.json`, and `SHA256SUMS`.
   - Verify signing/notarization status (macOS/Windows) and no duplicate legacy aliases.
   - Run any draft repair **before** publishing (published assets are immutable).
7. Upload the workflow-generated CDN updater manifests and release binaries to CDN.
8. Draft and review release notes (Section 7), then publish the GitHub Release as **pre-release** (Beta) or **latest** (Stable).

## 7. Release Notes & Contributor Attribution Standard

Publish every release's notes on [GitHub Releases](https://github.com/Open-Less/openless/releases) in this exact section order:

1. **User-visible features and improvements** (state platform scope and experimental boundaries)
2. **Bug fixes** (affected behavior and resolution)
3. **Architecture changes** (Core/Host, data, protocol, dependency updates — never frame internal refactors as new user features)
4. **Build and distribution changes** (packaging, CI, signing, updater, CDN; distinguish post-tag build-tooling repairs from tagged app changes)
5. **PRs and participants** (complete, source-linked attribution index)
6. **Download/upgrade instructions and verification evidence**

### Attribution Rules

- **Baseline tags**: Explicitly name both comparison tags (Stable compares against the previous publicly released Stable; Beta compares against the previous public Beta — never an unpublished draft).
- **Accurate roles**: Link relevant PRs and submitters under each item. In the index, distinguish PR submitter, commit authors/co-authors, formal reviewers (with review state), discussion participants, and merger. Do not treat comments as formal approvals, do not fabricate GitHub handles for bot/AI or unlinked Git authors, and do not publish contributor email addresses.
- **Squash / integration PRs**: Credit both the original contribution PR (clearly labeled if closed-but-integrated rather than independently merged) and the integration PR/commit, verified via Git ancestry/history.
- **Editing published notes**: Save the existing release body and asset metadata before editing; read back after editing to verify body, pre-release/latest flags, and asset digests/sizes. Notes-only edits must never move tags or rebuild packages.
