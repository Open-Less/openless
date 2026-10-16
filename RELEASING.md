# Releasing OpenLess

This document defines who may cut releases and how. It is policy, not a how-to for
day-to-day development.

## Authority: admins only

**Only repository administrators may create version tags and publish releases.**

- Only an admin may create a release tag (`v*-tauri`) or publish a GitHub Release. Linux assets require a separate accepted build and explicit administrator upload.
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

These tags build the macOS, Windows, and Android Tauri hosts in parallel. Linux is
independent: `.github/workflows/release-linux-egui.yml` is manual-only, runs the
same Core/host/deb/rpm verification as CI, and uploads Actions artifacts. It does
not react to Tauri tags or change a GitHub Release. After Linux product acceptance,
an administrator may run it against the accepted existing release tag, verify its
commit and package checksums, and explicitly attach the deb/rpm files to that
Release. Linux has no in-app updater manifest or AppImage.
For each new Linux package candidate, increment `openless-all/app/linux-egui/package-revision`
from the last `-N` suffix (for example `2.0.0-Beta.2-79` → `2.0.0-Beta.2-80`).
Both local packaging and the shared CI/tag build use this explicit revision; never drop
it or derive it from the Tauri tag, whose SemVer remains `2.0.0-Beta.2`.

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

Before independently attaching Linux assets, additionally require all of the following:

1. The egui team has completed the [Linux Host/UI gaps and acceptance](docs/linux-egui-handoff/07-acceptance.md). The existing `eframe::App` is a starting implementation; its presence and successful packaging alone do not establish product completeness.
2. Linux core/host tests, dependency gates, and secret-surface gates are green on Ubuntu.
3. The Linux workflow verifies ELF dependencies, deb/rpm contents, desktop metadata, fcitx5 plugin paths and package SHA-256 checksums. There is no AppImage, minisign key or Linux updater manifest.
4. The manual build targets the same existing tag whose commit, CI and Linux product acceptance were reviewed. `workflow_dispatch` only uploads Actions artifacts; an administrator separately uploads the verified packages to that Release.
5. Attach the Linux checksum file as `SHA256SUMS-linux` so it does not replace an existing desktop/Android checksum file.

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
