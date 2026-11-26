# OpenLess 维护者发布清单

[English](RELEASING.md) | [简体中文](RELEASING.zh.md)

本文档规定谁可以切出版本、版本号与包产物如何保持一致，以及发布前后的校验与上架流程。它是维护者发布规范，而非日常本地开发指南（本地全平台构建见 [openless-all/README.zh.md](openless-all/README.zh.md)）。

## 1. 发布权限

**只有仓库管理员可以创建版本标签并发布 GitHub Release。**

- 贡献者与 AI 代理**不得**创建发布标签（`v*-tauri`）、发布版本或触发发布工作流；需要发布时请联系维护者，写明目标版本号与渠道（`Beta` 或 `Stable`）。
- 发布自动化仅允许**管理员触发**（由于只有管理员可推送发布标签，标签触发流水线等同于管理员触发）。
- Linux 软件包仅在完成该确切候选版本的真机与安装验收后才可手动附到对应标签；桌面端、Android 与 HarmonyOS 标签不等待 Linux。

## 2. 渠道、分支与标签约定

| 渠道 | 分支 | 标签格式 | 发布形态 |
| --- | --- | --- | --- |
| **Beta** | `beta`（默认开发分支） | `v<X.Y.Z>-Beta.<N>-tauri`（如 `v2.0.0-Beta.2-tauri`） | GitHub Pre-release；绝不向 Stable 用户推送自动更新 |
| **Stable** | `main`（由维护者从 `beta` 合并） | `v<X.Y.Z>-tauri` | GitHub Latest 正式版 |

- **日期构建元数据**：同步后的应用版本号可带 `build.` 前缀的 SemVer 元数据（如 `2.0.0-Beta.2+build.20260924`），但 Git 标签必须保持 `v2.0.0-Beta.2-tauri` 格式（已发布客户端仅识别数字 `Beta.<N>` 后缀，且 Tauri 2.10.1 的 NSIS 打包器会把纯数字元数据映射到 16 位 Windows 产品版本字段）。构建元数据不提升 SemVer 优先级，每次公开 Beta 仍需递增 `N`。
- **历史兼容**：更新器仍兼容历史 `*-beta-tauri` 后缀，但所有新 Beta 发布一律使用 `Beta.<N>`。

## 3. 版本号五处同步闸门

除非以下**五处**版本号完全一致，否则 Tauri 发布 CI 将直接失败。普通 `X.Y.Z` 版本可运行 `scripts/bump-version.sh <X.Y.Z>` 批量更新；预发布版本（`X.Y.Z-Beta.N`）请手动同步五处：

1. `openless-all/app/package.json`
2. `openless-all/app/package-lock.json`（根字段与嵌套 `packages.""`）
3. `openless-all/app/src-tauri/tauri.conf.json`
4. `openless-all/app/src-tauri/Cargo.toml`
5. `openless-all/app/src-tauri/Cargo.lock`（`name = "openless"` 条目）

> `openless-all/app/Cargo.lock` 仅属于框架无关的 Core / Linux workspace，不在 Tauri 五处同步闸门之内。

### 许可证边界

- 已发布的 `1.x` 版本保持 **MIT** 许可证。
- 自 `2.0.0-Beta.1` 起仓库采用 **`AGPL-3.0-only`**（见 [LICENSE](LICENSE)）；内置的第三方组件与字体保留各自上游许可（见 [docs/ATTRIBUTION.md](docs/ATTRIBUTION.md)）。

## 4. 统一发布流水线与下载资产

推送 `v*-tauri` 标签仅触发单一入口：`.github/workflows/release-tauri.yml`（**Release (all platforms)**）。

1. **前置检查（Preflight）** 创建或校验共享的 GitHub Release **草稿（draft）**。
2. **macOS、Windows、Android（全部 4 个 ABI）与 HarmonyOS** 在同一次 Actions 运行中从同一不可变提交并行构建，并将产物附到共享草稿。
3. Android 每周/调试构建、Windows 草稿修复与 HarmonyOS 手动派发仍可按需单独运行。

### 资产命名与更新器清单

- **新发布不附带重复别名**：仅发布原始带版本号的 macOS DMG、Windows 安装程序、Android APK、macOS `.app.tar.gz` 更新归档、合并后的 `updater-signatures.json`，以及 `OpenLess_harmony_arm64.hap` 与 `SHA256SUMS`。不再生成 `OpenLess_macos_*`、`OpenLess_windows_*`、`OpenLess_android_*` 或 `OpenLess_harmony.hap` 等重复别名；历史 Release 保持原样。
- **更新器清单托管在 CDN，不作为 GitHub Release 资产**：
  - Stable：`https://dl.aeoform.com/updates/stable/latest-{target}-{arch}.json`
  - Beta：`https://dl.aeoform.com/updates/beta/latest-{target}-{arch}-beta.json`
  - 工作流会将清单输出为运行产物（刻意不附在 Release 上），草稿核验完成后由维护者上传至 CDN。
- **官网与下载解析**：通过 GitHub Releases API 读取实际资产名（Stable 读取最新正式版；Beta 显式读取预发布标签，因 GitHub `latest` 不含 pre-release）。

### Linux（自动化暂停）

- Linux 专用 CI 与自动发布挂载当前处于暂停状态；源码与本地打包脚本持续维护。
- 手动打包或分发 `.deb` / `.rpm` 前，须递增 `openless-all/app/linux-egui/package-revision`（如 `2.0.0-Beta.2-79` → `2.0.0-Beta.2-80`），并对该确切候选版本完成真机安装与听写验收。Linux 不提供 AppImage 或应用内更新器。

## 5. HarmonyOS 打包、签名与 AppGallery Connect

HarmonyOS 通过 `.github/workflows/release-harmony.yml` 构建，`scripts/prepare-harmony-sdk.sh` 自动从公开镜像拉取并校验 SHA-256 后的命令行工具并由 `actions/cache` 缓存（可通过 `HARMONY_SDK_URL` / `HARMONY_SDK_SHA256` 或配置了 `HARMONY_TOOLCHAIN` 的自托管 runner 覆盖）；切勿将 SDK 归档提交进仓库。

### 渠道与产物矩阵

| `channel` 参数 | 产物 | 签名材料 | 去向 |
| --- | --- | --- | --- |
| `none`（默认） | `OpenLess_harmony_arm64.hap` | **未签名** | GitHub Release 资产 + `SHA256SUMS` |
| `release` | `OpenLess_harmony_arm64-signed.hap` | `release.{cer,p7b,p12}` | 仅上传至 AppGallery 正式候选包（绝不附到 GitHub Release） |
| `test` | `OpenLess_harmony_arm64-signed.hap` | `test.{cer,p7b,p12}` | 仅上传至 AppGallery 测试渠道 |

- `release-tauri.yml` 默认以 `HARMONY_AUTO_CHANNEL || 'none'` 调用；将仓库变量 `HARMONY_AUTO_CHANNEL` 设为 `release` 或 `test` 可自动上传签名候选包到 AGC，但**绝不自动提交审核**，上架仍需人工确认。
- `ci.yml` 始终以未签名模式构建（`OPENLESS_HARMONY_CHANNEL=none`）。本地可用 `scripts/verify-harmony-hap.sh <hap> <toolchain> <name> <code> --allow-unsigned` 校验未签名包。
- `openless-all/app/harmony/AppScope/app.json5` 中的 `versionName`/`versionCode` 为开发占位值；打包脚本构建时自动从 `package.json` 与 `linux-egui/package-revision` 注入，完成后还原。
- 签名步骤经 `$GITHUB_ENV` 导出 `OPENLESS_HARMONY_SIGNING_DIR` 并在编译前通过 `scripts/check-harmony-signing.sh` 预检（不可在 job 级 `env:` 中声明同名占位变量）。

### HarmonyOS 所需仓库 Secrets 与变量

| 名称 | 用途 |
| --- | --- |
| `HARMONY_AUTO_CHANNEL`（变量） | 可选自动渠道（`none` / `release` / `test`，默认 `none`） |
| `HARMONY_KEYSTORE_BASE64` | Base64 编码的 `.p12` 密钥库（或按渠道 `HARMONY_<CHANNEL>_KEYSTORE_BASE64`） |
| `HARMONY_CERT_BASE64` | Base64 编码的 `.cer` 证书（或 `HARMONY_<CHANNEL>_CERT_BASE64`） |
| `HARMONY_PROFILE_BASE64` | Base64 编码的 `.p7b` Profile（或 `HARMONY_<CHANNEL>_PROFILE_BASE64`） |
| `HARMONY_KEYSTORE_PASSWORD` | 密钥库密码（或 `HARMONY_<CHANNEL>_KEYSTORE_PASSWORD`；未设 `HARMONY_KEY_PASSWORD` 时回退使用此值） |
| `HARMONY_KEY_ALIAS` | 签名密钥别名（或 `HARMONY_<CHANNEL>_KEY_ALIAS`） |
| `APPGALLERY_CLIENT_ID` | 华为 AppGallery Connect API Client ID |
| `APPGALLERY_CLIENT_SECRET` | 华为 AppGallery Connect API Client Secret |
| `APPGALLERY_APP_ID` | 华为 AppGallery Connect App ID |

未配置 `APPGALLERY_*` 时，工作流仍会把未签名 HAP 附到 GitHub Release 并自动跳过商店上传。

## 6. 管理员发布前检查表

1. 确认当前分支对应目标渠道（Beta 在 `beta`，Stable 为维护者合并后的 `main`）。
2. 确认五处版本号完全同步（版本闸门通过）。
3. 确认待打标签的提交上 CI 全绿。
4. 在受影响平台上对安装包完成冒烟验证：启动、系统权限、录音、取消、落字与剪贴板回退、升级链路及本次改动的功能。
5. 推送发布标签（`v<X.Y.Z>-tauri` 或 `v<X.Y.Z>-Beta.<N>-tauri`）。
6. 等待 `Release (all platforms)` 所有必需作业完成，核查共享草稿：
   - 核对安装程序、APK、未签名 HarmonyOS HAP、`updater-signatures.json` 与 `SHA256SUMS` 齐全。
   - 确认 macOS/Windows 签名公证正常，且未混入旧版重复别名。
   - 如需草稿修复，务必在**发布前**完成（已发布资产不可修改）。
7. 将工作流生成的更新器清单与发布产物同步上传至 CDN。
8. 按第 7 节格式整理发布说明，正式发布为 **Pre-release**（Beta）或 **Latest**（Stable）。

## 7. 发布说明与贡献署名规范

每个版本均需在 [GitHub Releases](https://github.com/Open-Less/openless/releases) 按以下固定顺序撰写说明：

1. **用户可见的新功能与改进**（注明适用平台与实验功能边界）
2. **问题修复**（说明受影响行为与修复结果）
3. **架构变更**（Core/Host、数据、协议与依赖变化，不将内部重构包装为用户新功能）
4. **构建与分发变更**（打包、CI、签名、更新器与 CDN；区分标签后构建工具修复与标签内应用改动）
5. **PR 与参与者索引**（完整、可溯源的贡献署名列表）
6. **下载/升级指引与验证记录**

### 署名要求

- **基线标签**：明确列出对比起止标签（Stable 对比上一个公开正式版；Beta 对比上一个公开 Beta，不得使用未发布草稿）。
- **角色准确**：每项条目下关联对应 PR 与提交者；索引中区分 PR 提交者、Commit 作者/共同作者、正式评审者（含评审状态）、讨论参与者与合并者。不将普通评论计为代码批准，不为机器人/AI 或未关联 Git 作者捏造 GitHub 账号，不公开贡献者邮箱。
- **整合/压缩合入**：同时署名原始贡献 PR（准确标注“已关闭但已整合”，而非独立合并）与实际整合 PR/提交，并以 Git 祖先历史核验。
- **修改已发布说明**：修改前先备份当前正文与资产元数据，修改后回读核验正文、渠道标记与资产摘要/大小；纯说明修订不得重打包或移动标签。
