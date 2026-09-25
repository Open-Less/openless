# Contributing to OpenLess / 参与贡献

This is the public contributor guide. Personal `AGENTS.md` / `CLAUDE.md` instructions are intentionally ignored by Git and are **not** project documentation.

本文件是公开贡献指南。个人使用的 `AGENTS.md`、`CLAUDE.md` 被 Git 忽略，不能作为仓库读者的规则来源。

## Branches and releases / 分支与发布

- Open pull requests against `beta`. Only maintainers merge `beta` into `main` for Stable releases.
- Only repository admins create `v*-tauri` release tags or publish GitHub Releases. See [RELEASING.md](RELEASING.md) for the version-sync gate, Beta drafts, product acceptance and signing requirements. Contributors and automated assistants must not create tags or trigger publication.
- Linux egui builds deb/rpm only. CI runs its tests and package verification on PRs; after device acceptance, an admin's shared release tag automatically attaches the verified packages. Linux has no AppImage or in-app updater manifest.

PR 请提交到 `beta`。正式版由维护者从 `beta` 合入 `main`；只有管理员可以创建发布 tag、发布 Release。Linux deb/rpm 在 PR 的 CI 中构建并验证，真机验收后随管理员推送的共用 tag 自动附加，不提供 AppImage 或 Linux 应用内更新清单。

## Source boundaries and checks / 源码边界与检查

- Read the [documentation index](docs/index.md), [architecture](docs/architecture.md) and [source map](docs/structure.md) before changing cross-platform interfaces. Shared business logic belongs in `openless-core`; platform-specific host code stays within its host.
- The application workspace is `openless-all/app`. Run `npm ci`, `npm test` and `npm run build` there for frontend and contract changes. Core/Linux Rust tests use `cargo test --locked -p openless-core` and `cargo test --locked -p openless-linux-egui --all-targets`; platform CI and release builds remain the authoritative cross-platform gates. Green CI does not substitute for device acceptance.
- When adding UI strings, update the relevant catalogs in `openless-all/app/src/i18n/` for **all eight supported locales** and run the frontend tests. Do not rely on private agent instructions for localization policy.
- For Windows installer changes, follow the current [Tauri release workflow](.github/workflows/release-tauri.yml) and [Windows packaging script](openless-all/app/scripts/windows-package-msvc.ps1). NSIS is built separately; MSI repair uses `-sice:ICE80` where required, and Beta versions that WiX cannot represent skip MSI. Do not infer current CI behavior from historical implementation plans.

勿提交凭据、个人规划或生成的 `node_modules`、`dist`、`target` 产物。涉及打包、平台权限或真实输入设备的修改，须补充对应平台的构建和实际设备验证证据。
