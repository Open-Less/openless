# Releasing OpenLess

This document defines who may cut releases and how. It is policy, not a how-to for
day-to-day development.

## Authority: admins only

**Only repository administrators may create version tags and publish releases.**

- Only an admin may create a release tag (`v*-tauri`) or publish a GitHub Release. Linux assets ride the same tag, but the L01-L12 device acceptance must be finished *before* the tag is cut.
- Contributors (including AI agents) **must not** create release tags, publish
  releases, or trigger release automation. If a release is needed, **request an admin
  to cut it** — open an issue or ping a maintainer with the target version and the
  channel (Beta or Stable).
- Release automation (CI workflows that build, sign, and publish artifacts, and the
  auto-updater feed) must be **admin-triggered only**. Tag-triggered pipelines are
  considered admin-triggered because only admins may push the triggering tag.

## Channels and tags

Two channels; the branch name equals the channel name:

- **`beta`** — default development branch and the Beta channel.
- **`main`** — the Stable channel (正式版). Always releasable; only maintainers merge
  `beta → main`.

Tauri host release tags (created by an admin only):

- **Stable release:** push tag `v<version>-tauri`.
- **Beta release:** push tag `v<X.Y.Z>-Beta.<N>-tauri`, for example
  `v1.3.15-Beta.1-tauri` (published as a GitHub pre-release; never auto-updates
  Stable users). The updater still recognizes the historical `*-beta-tauri`
  suffix for existing releases, but new releases use the `Beta.<N>` form.

A dated build may carry SemVer build metadata in the synchronized application
version, for example `2.0.0-Beta.2+build.20260924`. Keep its release tag in the existing
`v2.0.0-Beta.2-tauri` format: released clients only recognize a numeric `Beta.N`
tag suffix. Include the complete application version in the release title and
updater manifests. Build metadata does not advance SemVer precedence; each new
public Beta still increments `N`. Use the `build.` prefix for date metadata:
Tauri 2.10.1 otherwise maps a numeric date to the fourth Windows product-version
component, which is a 16-bit field. The full version still appears in the app and
updater manifest; NSIS uses its supported numeric fallback for file metadata.
See [the pinned NSIS bundler](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.10.1/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs#L149).

These tags build the macOS, Windows, and Android Tauri hosts in parallel.

Linux and HarmonyOS ride in the same Release through two dispatch-only entry
points, `.github/workflows/release-linux-egui.yml` and
`.github/workflows/release-harmony.yml`. Neither reacts to a `push:` event, so an
arbitrary commit can never rebuild a package; `release-tauri.yml` dispatches both
through `dispatch-platform-releases` as soon as the desktop/Android assets of a
`v*-tauri` tag are published. That dispatch is what makes a tag push ship a
complete Release: the two workflows only wait ten minutes for the Release to
exist, less than the desktop matrix needs, which is why they cannot be triggered
by the tag itself. To rebuild one platform for an already-published tag, dispatch
it by hand on the same tag - that stays the repair path.

Linux runs the same Core/host/deb/rpm verification as CI and **attaches the
verified packages to that Release automatically**; nothing is uploaded by hand,
and there is no in-app updater manifest or AppImage. Because the tag now ships
Linux unattended, the device acceptance in
[docs/linux-egui-handoff/07-acceptance.md](docs/linux-egui-handoff/07-acceptance.md)
is a **pre-tag** gate rather than a post-build one.

HarmonyOS builds, signs and attaches `OpenLess_harmony_arm64.hap` plus the
`OpenLess_harmony.hap` alias, and can upload the signed package (App Pack) to
HUAWEI AppGallery Connect. The automatic dispatch uses `channel: none`, the
repository path: the HAP is attached to the GitHub Release unsigned and nothing
reaches the store. Set the repository variable `HARMONY_AUTO_CHANNEL` to
`release` (production) or `test` (test channel) to have the signed package
uploaded as the AGC candidate package automatically; the upload never submits it
for review, so store submission remains a human decision either way.

The build needs the official HarmonyOS command-line tools (SDK + hvigor + ohpm +
bundled node). Huawei's download page wants an account, but the same archive is
mirrored publicly - the mirror `k2-fsa/sherpa-onnx`'s own `harmony-os` workflow
uses - so `scripts/prepare-harmony-sdk.sh` downloads
`commandline-tools-linux-x64-6.0.1.251.zip` from it, verifies the pinned SHA-256
and lays it out under `~/.local/share/harmony/current`. **No repository variable
and no self-hosted runner is required**; the toolchain is cached by
`actions/cache` under the tools version.

Override `HARMONY_SDK_URL` / `HARMONY_SDK_SHA256` to move to a newer tools
release, or set `HARMONY_TOOLCHAIN` and dispatch with
`runner_labels: ["self-hosted","harmony"]` on a machine that already has the
tools installed. Do not commit the archive: the SDK licence does not permit
redistribution, which is also why it is fetched at job time rather than vendored.

### Which channel gets which package

| Channel | Artifact | Signing | Where it goes |
| --- | --- | --- | --- |
| Repository / GitHub Release | `OpenLess_harmony_arm64.hap` (+ `OpenLess_harmony.hap`) | **unsigned** | Release assets, `SHA256SUMS` |
| AppGallery 正式版 | `OpenLess_harmony_arm64-signed.hap` | `release.*` material | AppGallery only, never a Release asset |
| 测试渠道 | `OpenLess_harmony_arm64-signed.hap` | `test.*` material | AppGallery test channel |

Dispatch the workflow with `channel: none` (default), `channel: release` or
`channel: test`:

- `none` is the repository path. It builds the HAP, attaches the **unsigned**
  package to the GitHub Release and touches neither the signing step nor the
  store, so it needs no secret at all.
- `release` / `test` select the signing prefix (`release.cer/p7b/p12` vs
  `test.cer/p7b/p12`), sign, and upload to AppGallery.

The workflow artifact bundle carries both packages; the Release step uploads only
the unsigned pair, and the AppGallery step uploads only the signed one. CI never
signs: `ci.yml` always builds with `OPENLESS_HARMONY_CHANNEL=none`.

A local machine without store certificates can still produce a package:
`OPENLESS_HARMONY_CHANNEL=none` builds and ships the unsigned HAP only, and
`scripts/verify-harmony-hap.sh <hap> <toolchain> <name> <code> --allow-unsigned`
checks its contents without a device.
For each new Linux package candidate, increment `openless-all/app/linux-egui/package-revision`
from the last `-N` suffix (for example `2.0.0-Beta.2-79` → `2.0.0-Beta.2-80`).
Both local packaging and the shared CI/tag build use this explicit revision; never drop
it or derive it from the Tauri tag, whose SemVer remains `2.0.0-Beta.2`.

## Direct download aliases

The release workflows also publish stable, version-independent asset names for the official website:

- macOS Apple Silicon: `OpenLess_macos_aarch64.dmg`
- macOS Intel: `OpenLess_macos_x86_64.dmg`
- Windows x64: `OpenLess_windows_x86_64-setup.exe`
- Android: `OpenLess_android_<abi>.apk` (one each for `arm64-v8a`, `armeabi-v7a`, `x86`, and `x86_64`)
- Linux: `OpenLess_linux_x86_64.deb` and `OpenLess_linux_x86_64.rpm`

Stable website links use `https://github.com/Open-Less/openless/releases/latest/download/<asset-name>`.
Beta links must use the release tag because `latest` excludes pre-releases:
`https://github.com/Open-Less/openless/releases/download/<tag>/<asset-name>`.

The Tauri and Android workflows attach their aliases automatically. Linux remains administrator-attached after product acceptance, so the administrator must attach the two files from the `openless-linux-egui-direct-download` Actions artifact to the accepted Release as well as the versioned packages. Do not replace the versioned assets; the aliases are an additional website-facing contract.

Under the [2026-09-06 2.0 requirements](docs/2.0-requirements.md), Windows and macOS must fully retain their respective Tauri 1.x features. The egui team owns Linux Host/UI work and Linux product acceptance. Linux application gaps do not block the Windows/macOS delivery, but shared Core defects do. Linux packages must not be attached before their own product acceptance. Existing Android builds do not expand this scope into a new full-support commitment.

## Version-sync gate

A Tauri release fails CI unless **five** locations carry the same version. Bump them together
with `scripts/bump-version.sh <X.Y.Z>`:

- `openless-all/app/package.json`
- `openless-all/app/package-lock.json` (root and nested `packages.""`)
- `openless-all/app/src-tauri/tauri.conf.json`
- `openless-all/app/src-tauri/Cargo.toml`
- `openless-all/app/src-tauri/Cargo.lock` (the `name = "openless"` block)

The root `openless-all/app/Cargo.lock` belongs only to the framework-independent core/Linux workspace and is not one of the five Tauri application version locations.

## License boundary

Published 1.x releases remain MIT. `2.0.0-Beta.1` is the effective boundary for
the repository's `AGPL-3.0-only` license; third-party vendor files retain their
own MIT, Apache, LGPL, or other upstream terms.

The script takes a plain `X.Y.Z`; for a prerelease version such as
`X.Y.Z-Beta.N`, edit the files by hand.

## Pre-release checklist (for the admin cutting the release)

1. Branch is the intended channel (`beta` for Beta, `main` for Stable).
2. All five version files match (version-sync gate green).
3. CI is green on the commit being tagged.
4. The applicable [desktop feature and device acceptance](docs/2.0-desktop-acceptance.md), signing, and distribution requirements are met; green builds alone do not establish product readiness. Linux acceptance below is required before including Linux assets.
5. Then, and only then, push the release tag.
6. Both Beta and Stable tag workflows upload Tauri and Android assets to a shared
   draft. Wait for **both** workflows to succeed, verify the packages and channel
   updater manifests, then publish that draft (Beta as a prerelease; Stable as latest).
   Do not rerun an asset workflow after publication without first returning the release to draft.
7. Publishing Stable with the administrator's CLI/UI credential automatically
   triggers `sync-homebrew-cask.yml`. It accepts only the published latest Stable
   tag, preserves application build metadata in DMG filenames, verifies matching
   ARM/Intel versions, computes their checksums and commits the cask update.
   Drafts, Beta and older releases never update Homebrew. If repair is needed, run
   `gh workflow run sync-homebrew-cask.yml -f tag=v<version>-tauri`; no asset rebuild
   or manual cask edit is needed. Do not publish via an asset workflow's
   `GITHUB_TOKEN`, which does not emit the downstream release event.

Before dispatching the Linux release build, additionally require all of the following:

1. The egui team has completed the [Linux Host/UI gaps and acceptance](docs/linux-egui-handoff/07-acceptance.md). The existing `eframe::App` is a starting implementation; its presence and successful packaging alone do not establish product completeness.
2. Linux core/host tests, dependency gates, and secret-surface gates are green on Ubuntu.
3. The Linux workflow verifies ELF dependencies, deb/rpm contents, desktop metadata, fcitx5 plugin paths and package SHA-256 checksums. There is no AppImage, minisign key or Linux updater manifest.
4. The dispatch pins the tag as an input; the workflow checks that tag out and rejects a `package.json` version mismatch, so the packages provably come from the reviewed commit.
5. The checksum file is produced and attached as `SHA256SUMS-linux` so it does not replace an existing desktop/Android checksum file.

## HarmonyOS signing material and AppGallery credentials

The HAP build reads everything from the environment; no key material is ever
committed, cached or printed.

### Where each piece comes from

Everything below is done once, on a machine you trust, with your own HUAWEI
developer account. Nothing here can be automated: AppGallery issues the material
only to a verified developer, and it will not accept a self-signed substitute.

| Step | Where | What you get |
|---|---|---|
| 1 | <https://developer.huawei.com> → 注册 + 实名认证（个人或企业） | an account that may create certificates |
| 2 | AppGallery Connect → 我的项目 → 创建项目 → 添加应用（HarmonyOS，包名必须是 `openless.hm.com`，与 `harmony/AppScope/app.json5` 一致） | `APPGALLERY_APP_ID`（应用信息里的 APP ID） |
| 3 | 本地：生成密钥库与证书请求 | `release.p12` + `release.csr` + 两个密码 |
| 4 | AGC → 用户与访问 → 证书管理 → 申请证书（**发布证书**）→ 上传 `release.csr` | `release.cer` |
| 5 | AGC → Profile 管理 → 申请 Profile（**发布 Profile**，绑定上面的证书与包名） | `release.p7b` |
| 6 | AGC → 用户与访问 → API 密钥 → 创建客户端（勾选发布相关权限） | `APPGALLERY_CLIENT_ID` / `APPGALLERY_CLIENT_SECRET` |

Step 3, without DevEco Studio:

```bash
keytool -genkeypair -keystore release.p12 -storetype PKCS12 \
  -alias harmony-release -keyalg EC -groupname secp256r1 \
  -sigalg SHA256withECDSA -validity 9125 \
  -dname "CN=OpenLess, O=<你的组织>, C=CN"
keytool -certreq -keystore release.p12 -alias harmony-release -file release.csr
```

Keep `release.p12` and both passwords offline; they are the only copy of the
private key that AppGallery will trust.

Before uploading anything, dry-run the material:

```bash
bash openless-all/app/scripts/check-harmony-signing.sh \
  ~/.local/share/harmony/signing/release release
```

It proves the four things that fail late otherwise: the certificate belongs to
the keystore key, the profile was issued for that certificate, `bundle-info.bundle-name`
equals `harmony/AppScope/app.json5`, and nothing has expired. The workflow runs the
same check before it starts building.

### Storing them in the repository

```bash
base64 -w0 release.cer       # → HARMONY_RELEASE_CERT_BASE64
base64 -w0 release.p7b       # → HARMONY_RELEASE_PROFILE_BASE64
base64 -w0 release.p12       # → HARMONY_RELEASE_KEYSTORE_BASE64
```

`HARMONY_<CHANNEL>_*` names let production and test material coexist. The
channel-agnostic `HARMONY_CERT_BASE64` / `HARMONY_PROFILE_BASE64` /
`HARMONY_KEYSTORE_BASE64` are still accepted and are used when no
channel-specific secret is set.

### What the workflow does, and what it cannot do

Automated: build → sign → verify the signature → upload as the AppGallery
*candidate* package → attach the **unsigned** HAP to the GitHub Release.

Manual, always: AGC requires a human to fill in the store listing (icon,
screenshots, privacy-policy URL, 软件著作权, 分类与年龄分级) and to press
提交审核. `scripts/upload-appgallery.mjs` deliberately stops at replacing the
candidate package.

| Kind | Name | Purpose |
|---|---|---|
| Secret | `HARMONY_KEYSTORE_BASE64` | base64 of the release `.p12` keystore |
| Secret | `HARMONY_CERT_BASE64` | base64 of the release `.cer` certificate |
| Secret | `HARMONY_PROFILE_BASE64` | base64 of the release `.p7b` provisioning profile |
| Secret | `HARMONY_KEYSTORE_PASSWORD` | keystore password |
| Secret | `HARMONY_KEY_PASSWORD` | key password (falls back to the keystore password) |
| Variable (optional) | `HARMONY_KEY_ALIAS` | key alias inside the keystore; defaults to `harmony-<channel>` |
| Variable (optional) | `HARMONY_TOOLCHAIN` | absolute path of the DevEco / HarmonyOS SDK on a self-hosted runner |
| Variable (optional) | `HARMONY_SDK_URL` | override the tools archive URL |
| Variable (optional) | `HARMONY_SDK_SHA256` | SHA-256 of that archive |
| Variable (optional) | `HARMONY_AUTO_CHANNEL` | `none` (default) / `release` / `test` - the channel the automatic tag-build dispatch runs with; `release` or `test` upload the signed App Pack to AppGallery as the candidate package (never submitted for review) |

The `test` channel reads the same three Secrets (`HARMONY_KEYSTORE_BASE64`,
`HARMONY_CERT_BASE64`, `HARMONY_PROFILE_BASE64`). Store the test profile there
and switch channels by dispatching with `channel: test`, so one secret set at a
time matches the channel being built.
| Secret | `APPGALLERY_CLIENT_ID` | AppGallery Connect API client id |
| Secret | `APPGALLERY_CLIENT_SECRET` | AppGallery Connect API client secret |
| Secret | `APPGALLERY_APP_ID` | AppGallery Connect app id of the HarmonyOS app |

Nothing else has to be configured: the build downloads the official command-line
tools from the checksum-pinned mirror, so a hosted runner needs no variable, no
self-hosted machine and no Huawei account. The signing directory is exported by
the workflow itself (`$RUNNER_TEMP/harmony-signing`) and must not be redeclared as
an `env:` key - YAML `env` outranks `$GITHUB_ENV`, so an empty placeholder would
shadow it and the build would fall back to a directory that does not exist.

The upload replaces the candidate package in AppGallery Connect; it never submits
the app for review, because AGC requires a human submission. Without the
`APPGALLERY_*` secrets the workflow still attaches the HAP to the GitHub Release
and reports that the store upload was skipped.

`versionName`/`versionCode` in `openless-all/app/harmony/AppScope/app.json5` are a
development placeholder; the packaging script rewrites them for the build
(`versionName` from `package.json`, `versionCode` from
`linux-egui/package-revision`, which only increases) and restores the file
afterwards.

## Release notes and contributor attribution

Every release page must use this order:

1. **User-visible features and improvements** — describe what users can do, with
   platform and experimental-status boundaries where applicable.
2. **Bug fixes** — summarize the affected behavior and the correction.
3. **Architecture changes** — explain relevant Core/Host, data, protocol and
   dependency changes without presenting internal refactors as new user features.
4. **Build and distribution changes** — cover packaging, CI, signing, updater and
   distribution work. Distinguish post-tag build-tooling repairs from application
   changes contained in the tag.
5. **PRs and participants** — provide a complete, source-linked attribution index.
6. **Download/upgrade instructions and verification evidence**.

For Stable, compare against the **previous publicly released Stable**, including
features accumulated across intervening Betas. An unpublished draft or reserved
version tag is not the comparison baseline. For Beta, name the previous public
Beta used as the baseline. Always state both comparison tags explicitly.

Under **every feature, fix, architecture and build item**, link the relevant PRs
and identify their submitters. The linked attribution index must distinguish PR
submitter, commit authors/co-authors, formal reviewers (including review state),
discussion participants and merger. Retrieve all pages of commits/reviews/comments
when needed. Preserve unlinked Git author names as names; do not invent GitHub
accounts, roles, approvals or human identities for bot/AI signatures. Comments are
not code review approvals, and an integration PR's participants are not necessarily
the authors of every feature it contains. Do not publish contributor email addresses.

For work incorporated through a squash, cherry-pick or integration branch, credit
both the **original contribution PR** and the **actual integration PR/commit**.
Label a closed-but-integrated source PR accurately; do not call it independently
merged. Verify inclusion from ancestry, file history and integration records, not
from PR titles or dates alone. If a PR is unavailable, cite the verified commit and
record the attribution limitation. Link the full comparison and retain platform
exclusions (for example, Linux source work does not imply Linux packages shipped).

Keep the release-page Markdown in `docs/releases/<version>-release-notes.md`.
Before editing an existing public release, save its current body and asset metadata;
after editing, read it back and verify the body, channel/publication flags and asset
names/digests/sizes. A notes-only change must not rebuild packages, move tags or
change the published channel. The same format applies to subsequent releases.

## Process summary

1. Land work on `beta` via PRs (open PRs against `beta`, never `main`).
2. For a Stable release, a maintainer merges `beta → main`.
3. An **admin** bumps the Tauri version (five-location sync), verifies CI is green,
   and pushes the release tag, which triggers the macOS/Windows/Android asset
   workflows and their channel-specific updater manifests.
4. After both release workflows pass, an admin verifies and publishes the shared
   draft. Linux packages are attached independently only after Linux acceptance.
