# Releasing OpenLess

This document defines who may cut releases and how. It is policy, not a how-to for
day-to-day development.

## Authority: admins only

**Only repository administrators may create version tags and publish releases.**

- Only an admin may create a release tag (`v*-tauri`) or publish a GitHub Release. Linux assets are attached automatically by the tag workflow after product acceptance.
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

These tags build the macOS, Windows, and Android Tauri hosts and the independent Linux egui host in parallel. Linux is not part of the Tauri matrix: `.github/workflows/release-linux-egui.yml` builds only deb/rpm, attaches them to the same Release draft and publishes no in-app updater manifest or AppImage. Linux product acceptance must be completed **before** an admin pushes the shared tag. CI runs the same Linux packaging and verification path on PRs without attaching release assets.

Under the [2026-09-06 2.0 requirements](docs/2.0-requirements.md), Windows and macOS must fully retain their respective Tauri 1.x features. The egui team owns Linux Host/UI work and Linux product acceptance. Although Linux application gaps did not previously block Windows/macOS delivery, this shared-tag workflow now attaches Linux packages automatically: do not cut a shared release tag until Linux is accepted as well. Existing Android builds do not expand this scope into a new full-support commitment.

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
6. Beta tag workflows upload Tauri, Android and Linux egui assets to a shared draft.
   Wait for **all three** workflows to succeed, verify the packages, Linux deb/rpm
   checksums and Beta updater manifests, then publish that draft as a prerelease.
   Do not rerun an asset workflow after publication without first returning the release to draft.

Before pushing a tag that will automatically attach Linux assets, additionally require all of the following:

1. The egui team has completed the [Linux Host/UI gaps and acceptance](docs/linux-egui-handoff/07-acceptance.md). The existing `eframe::App` is a starting implementation; its presence and successful packaging alone do not establish product completeness.
2. Linux core/host tests, dependency gates, and secret-surface gates are green on Ubuntu.
3. The Linux workflow verifies ELF dependencies, deb/rpm contents, desktop metadata, fcitx5 plugin paths and package SHA-256 checksums. There is no AppImage, minisign key or Linux updater manifest.
4. The shared tag points to the same commit whose CI and Linux product acceptance were reviewed. A `workflow_dispatch` build only uploads Actions artifacts, never Release assets.

## Process summary

1. Land work on `beta` via PRs (open PRs against `beta`, never `main`).
2. For a Stable release, a maintainer merges `beta → main`.
3. An **admin** bumps the Tauri version (five-location sync), verifies CI is green,
   and pushes the release tag, which triggers the macOS/Windows/Android and Linux
   asset workflows (only supported platforms generate auto-update manifests).
4. After Linux product acceptance and the shared tag's Tauri, Android and Linux
   workflows pass, an admin reviews all assets and publishes the shared draft.
