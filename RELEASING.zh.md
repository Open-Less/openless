# 发布 OpenLess

[English](RELEASING.md) | [简体中文](RELEASING.zh.md)

本文档规定谁可以发布版本以及如何发布。这是政策，不是日常开发的入门指南。

## 权限：仅限管理员

**只有仓库管理员可以创建版本标签并发布版本。**

- 只有管理员可以创建发布标签（`v*-tauri`）或发布 GitHub Release。只有在针对该发布候选版本完成 L01-L12 设备验收之后，Linux 产物才可以搭乘同一个标签。桌面/Android 标签可以在不发布 Linux 软件包的情况下切出。
- 贡献者（包括 AI agent）**不得**创建发布标签、发布版本或触发发布自动化。如果需要发布，**请请求管理员切出**——提交 issue 或联系维护者，说明目标版本和渠道（Beta 或 Stable）。
- 发布自动化（构建、签名并发布产物的 CI 工作流，以及自动更新器 feed）必须**仅由管理员触发**。由标签触发的流水线视为由管理员触发，因为只有管理员可以推送触发用的标签。

## 渠道与标签

两个渠道；分支名等于渠道名：

- **`beta`** —— 默认开发分支，也是 Beta 渠道。
- **`main`** —— Stable 渠道（正式版）。始终可发布；只有维护者可以合并 `beta → main`。

Tauri 宿主发布标签（仅由管理员创建）：

- **Stable 发布：** 推送标签 `v<version>-tauri`。
- **Beta 发布：** 推送标签 `v<X.Y.Z>-Beta.<N>-tauri`，例如 `v1.3.15-Beta.1-tauri`（作为 GitHub 预发布发布；绝不会自动更新 Stable 用户）。对于已有的发布，更新器仍然识别历史上的 `*-beta-tauri` 后缀，但新的发布使用 `Beta.<N>` 形式。

带日期的构建可以在同步后的应用版本中携带 SemVer 构建元数据，例如 `2.0.0-Beta.2+build.20260924`。其发布标签保持现有的 `v2.0.0-Beta.2-tauri` 格式：已发布的客户端只识别数字形式的 `Beta.N` 标签后缀。请在发布标题和更新器清单中包含完整的应用版本。构建元数据不会推进 SemVer 优先级；每个新的公开 Beta 仍然递增 `N`。日期元数据请使用 `build.` 前缀：否则 Tauri 2.10.1 会把数字日期映射到 Windows 产品版本的第四个组成部分，而该字段是 16 位。完整版本仍然出现在应用和更新器清单中；NSIS 对文件元数据使用其支持的数字回退形式。参见[固定版本的 NSIS 打包器](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.10.1/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs#L149)。

这些标签只有一个入口，`.github/workflows/release-tauri.yml`（`Release (all platforms)`）。一个简短的前置检查会创建或校验共享的**草稿**；随后 macOS/Windows、全部四个 Android ABI 以及 HarmonyOS 在同一次 Actions 运行中并行构建。Android 和 HarmonyOS 是可复用的工作流，从调用方不可变的提交解析，而不是在桌面发布之后延迟派发。Android 不再有自己的标签推送触发器，因此 APK 不会被构建两次。

独立的 Android 每周/调试/手动构建以及 HarmonyOS 手动修复仍然可用。桌面分支/手动构建和 Windows 草稿修复保持现有范围。依赖关系图见[发布分发](docs/release-distribution.md)。

Linux 专用的 CI 和发布自动化已暂停：可复用的 Linux 检查与发布入口已被移除，桌面发布也不再派发 Linux。Linux 源码、本地构建/打包/验证脚本以及设备验收要求仍然可用。不会自动构建或附加任何 Linux 产物或别名。在手动分发软件包之前，请针对该确切候选版本完成 [docs/linux-egui-handoff/07-acceptance.md](docs/linux-egui-handoff/07-acceptance.md) 中的 L01-L12 验收。仅编译通过不等于设备验收。桌面与 Android 交付仍然相互独立；Linux 没有应用内更新器，也没有 AppImage。

HarmonyOS 会构建、签名并附加单个 `OpenLess_harmony_arm64.hap`，并且可以把已签名的软件包（App Pack）上传到 HUAWEI AppGallery Connect。自动的可复用调用使用 `channel: none`，即仓库路径：HAP 以未签名形式附加到 GitHub Release，任何东西都不会到达应用商店。将仓库变量 `HARMONY_AUTO_CHANNEL` 设为 `release`（生产）或 `test`（测试渠道），即可让已签名的软件包自动作为 AGC 候选包上传；该上传绝不会提交审核，因此无论哪种方式，上架提交仍然是人工决定。

构建需要官方 HarmonyOS 命令行工具（SDK + hvigor + ohpm + 内置 node）。华为的下载页面要求登录账号，但同一归档也有公开镜像——即镜像 `k2-fsa/sherpa-onnx` 自己的 `harmony-os` 工作流所使用的那份——因此 `scripts/prepare-harmony-sdk.sh` 会从该镜像下载 `commandline-tools-linux-x64-6.0.1.251.zip`，校验固定的 SHA-256，并将其展开到 `~/.local/share/harmony/current` 下。**不需要任何仓库变量，也不需要自托管 runner**；工具链由 `actions/cache` 按工具版本缓存。

要迁移到更新的工具版本，可覆盖 `HARMONY_SDK_URL` / `HARMONY_SDK_SHA256`，或者在已安装该工具的机器上设置 `HARMONY_TOOLCHAIN` 并以 `runner_labels: ["self-hosted","harmony"]` 派发。不要提交该归档：SDK 许可证不允许再分发，这也是它在作业运行时获取而非内置到仓库的原因。

### 哪个渠道获得哪个软件包

| 渠道 | 产物 | 签名 | 去向 |
| --- | --- | --- | --- |
| 仓库 / GitHub Release | `OpenLess_harmony_arm64.hap` | **未签名** | Release 资产、`SHA256SUMS` |
| AppGallery 正式版 | `OpenLess_harmony_arm64-signed.hap` | `release.*` 材料 | 仅 AppGallery，绝不作为 Release 资产 |
| 测试渠道 | `OpenLess_harmony_arm64-signed.hap` | `test.*` 材料 | AppGallery 测试渠道 |

以 `channel: none`（默认）、`channel: release` 或 `channel: test` 派发工作流：

- `none` 是仓库路径。它会构建 HAP，将**未签名**的软件包附加到 GitHub Release，既不涉及签名步骤也不涉及应用商店，因此完全不需要密钥。
- `release` / `test` 选择签名前缀（`release.cer/p7b/p12` 对比 `test.cer/p7b/p12`），执行签名，并上传到 AppGallery。

工作流产物的打包结果同时携带两个软件包；Release 步骤只上传那一个未签名的 HAP 及其校验和，AppGallery 步骤只上传已签名的那个。CI 从不签名：`ci.yml` 始终以 `OPENLESS_HARMONY_CHANNEL=none` 构建。

没有商店证书的本地机器仍然可以生成软件包：`OPENLESS_HARMONY_CHANNEL=none` 只构建并交付未签名的 HAP，而 `scripts/verify-harmony-hap.sh <hap> <toolchain> <name> <code> --allow-unsigned` 可以在没有设备的情况下检查其内容。对于每个新的 Linux 软件包候选版本，请从上一个 `-N` 后缀递增 `openless-all/app/linux-egui/package-revision`（例如 `2.0.0-Beta.2-79` → `2.0.0-Beta.2-80`）。本地打包使用这个显式修订号（Linux CI/标签构建已暂停）；绝不要丢弃它，也不要从 Tauri 标签推导它，后者的 SemVer 仍然是 `2.0.0-Beta.2`。

## 下载资产：新发布中不再有重复别名

未来的发布运行只发布原始带版本号的 macOS DMG、Windows 安装程序和 Android APK。更新器签名会合并为每次发布一个 `updater-signatures.json`，而不是单独的 `.sig` 附件。临时 `.sig` 文件仍保留在构建产物中，用于生成索引和 CDN 清单。HarmonyOS 只发布 `OpenLess_harmony_arm64.hap` 及其校验和。它们不再生成或附加重复名称 `OpenLess_macos_*`、`OpenLess_windows_*`、`OpenLess_android_*` 或 `OpenLess_harmony.hap`。macOS `.app.tar.gz` 更新器归档仍然发布，签名包含在索引中。Linux 自动化仍然暂停。格式、兼容性限制和切换方式见[发布分发](docs/release-distribution.md)。

已有的发布及其下载别名不会被迁移或删除。别名辅助脚本仍可供遗留/手动使用，但没有自动发布工作流会调用它。桌面部分重跑也会过滤掉旧的别名产物，而不是再次发布它们；它们不会删除已附加到现有发布的资产。

网站/CDN 的下载解析必须使用所选发布 API 中的实际资产名称（或指向这些资产的外部重定向），而不是假设旧的固定别名存在于新发布中。此变更不实现网站重定向。Stable 选择可以使用最新的 stable 发布；beta 选择必须显式选择其预发布标签，因为 GitHub 的 `latest` 不包含预发布。历史固定名称链接对历史发布仍然有效。更新器清单生成及其 CDN 产物交接保持不变。

根据 [2026-09-06 2.0 需求](docs/2.0-requirements.md)，Windows 和 macOS 必须完整保留各自 Tauri 1.x 的功能。egui 团队负责 Linux Host/UI 工作和 Linux 产品验收。Linux 应用的缺口不会阻塞 Windows/macOS 交付，但共享 Core 的缺陷会。Linux 软件包在自身产品验收通过之前不得附加。现有的 Android 构建不会把该范围扩大为新的完全支持承诺。

## 版本同步闸门

除非**五个**位置携带相同版本，否则 Tauri 发布会无法通过 CI。请用 `scripts/bump-version.sh <X.Y.Z>` 一起升级它们：

- `openless-all/app/package.json`
- `openless-all/app/package-lock.json`（根级与嵌套的 `packages.""`）
- `openless-all/app/src-tauri/tauri.conf.json`
- `openless-all/app/src-tauri/Cargo.toml`
- `openless-all/app/src-tauri/Cargo.lock`（`name = "openless"` 块）

根部的 `openless-all/app/Cargo.lock` 只属于与框架无关的 core/Linux 工作区，不是五个 Tauri 应用版本位置之一。

## 许可证边界

已发布的 1.x 版本仍为 MIT。`2.0.0-Beta.1` 是仓库 `AGPL-3.0-only` 许可证的实际边界；第三方 vendor 文件保留其各自的 MIT、Apache、LGPL 或其他上游条款。

该脚本接受纯 `X.Y.Z`；对于像 `X.Y.Z-Beta.N` 这样的预发布版本，请手动编辑这些文件。

## 发布前检查清单（供切出发布的管理员使用）

1. 分支是目标渠道（Beta 用 `beta`，Stable 用 `main`）。
2. 五个版本文件全部一致（版本同步闸门通过）。
3. 被打标签的提交上 CI 通过。
4. 满足适用的[桌面功能与设备验收](docs/2.0-desktop-acceptance.md)、签名和分发要求；仅构建通过并不能证明产品就绪。在包含 Linux 资产之前，必须完成后文的 Linux 验收。
5. 然后——且仅在那时——推送发布标签。
6. Beta 和 Stable 的标签工作流都会把 Tauri 和 Android 资产上传到共享的草稿。等待**两个**工作流都成功，验证软件包，然后发布该草稿（Beta 作为预发布；Stable 作为 latest）。
   发布之后，如果不先把该发布退回草稿状态，就不要重新运行资产工作流。

   这些工作流还会写入渠道更新器清单（`latest-{target}-{arch}.json` 和
   `latest-{target}-{arch}-beta.json`），但有意将它们**排除**在发布资产之外——
   当前的客户端可以读取 CDN 并独立发现已签名的 GitHub 发布。
   主仓库的 `mirror-release.yml` 现在会镜像已发布的二进制文件，校验其
   Minisign 签名，并生成/上传滚动清单。可下载的运行产物
   仍可用于诊断或手动恢复：

   | 平台 | 工作流产物 |
   | --- | --- |
   | macOS | `openless-macos-<arch>-updater` |
   | Windows | `openless-windows-x64-updater` |
   | Android | `openless-android-updater-manifests` |

   目标位置（就地覆盖；文件名按平台和渠道命名，绝不按版本命名）：

   ```text
   https://dl.aeoform.com/updates/stable/latest-{target}-{arch}.json
   https://dl.aeoform.com/updates/beta/latest-{target}-{arch}-beta.json
   ```

   它们的 `url` 字段必须指向 CDN 产物（`.../releases/{tag}/{asset}`）。
   镜像必须先成功上传二进制文件，才能推进滚动清单。如果阿里云不可用，
   新客户端可以回退到 GitHub 发布发现/下载；仅依赖 CDN 的旧客户端仍然需要滚动清单。
   在切换网站工作流之前，请在主仓库中配置 `OSS_BUCKET`、`OSS_ACCESS_KEY_ID` 和
   `OSS_ACCESS_KEY_SECRET`。网站迁移只能在对主仓库镜像运行成功之后合并。
   在 CDN 切换之前构建的客户端读取旧的 `releases/latest/download/latest-*.json`
   资产，而新发布不再携带这些资产，因此它们需要手动下载一次。
7. 使用管理员的 CLI/UI 凭据发布 Stable 会自动触发 `sync-homebrew-cask.yml`。
   它只接受已发布的 latest Stable 标签，在 DMG 文件名中保留应用构建元数据，
   校验 ARM/Intel 版本是否匹配，计算它们的校验和并提交 cask 更新。
   草稿、Beta 和更早的发布永远不会更新 Homebrew。如果需要修复，请运行
   `gh workflow run sync-homebrew-cask.yml -f tag=v<version>-tauri`；不需要重建资产，
   也不需要手动编辑 cask。不要通过资产工作流的 `GITHUB_TOKEN` 发布，
   因为它不会发出下游发布事件。

Linux 工作流派发已暂停。在手动分发 Linux 软件包（或恢复自动化）之前，必须满足以下全部条件：

1. egui 团队已完成 [Linux Host/UI 缺口与验收](docs/linux-egui-handoff/07-acceptance.md)。现有的 `eframe::App` 只是一个起步实现；它的存在以及打包成功本身并不能证明产品完整。
2. Linux core/host 测试、依赖闸门和密钥暴露面闸门在 Ubuntu 上通过。
3. 为 ELF 依赖、deb/rpm 内容、桌面元数据、fcitx5 插件路径和软件包 SHA-256 校验和运行本地打包验证器。没有 AppImage、minisign 密钥或 Linux 更新器清单。
4. 检出已审阅的不可变标签，并验证软件包版本与 `package.json` 匹配，从而可证明这些软件包来自已审阅的提交。
5. 生成校验和文件并以 `SHA256SUMS-linux` 附加，这样不会替换已有的桌面/Android 校验和文件。

## HarmonyOS 签名材料与 AppGallery 凭据

HAP 构建从环境读取所有内容；密钥材料绝不会被提交、缓存或打印。

### 各部分材料的来源

以下所有操作都在一台你信任的机器上、使用你自己的华为开发者账号完成一次。这里没有任何一步可以自动化：AppGallery 只会向通过验证的开发者签发材料，并且不会接受自签名的替代品。

| 步骤 | 位置 | 你会得到 |
|---|---|---|
| 1 | <https://developer.huawei.com> → 注册 + 实名认证（个人或企业） | 一个可以创建证书的账号 |
| 2 | AppGallery Connect → 我的项目 → 创建项目 → 添加应用（HarmonyOS，包名必须是 `openless.hm.com`，与 `harmony/AppScope/app.json5` 一致） | `APPGALLERY_APP_ID`（应用信息里的 APP ID） |
| 3 | 本地：生成密钥库与证书请求 | `release.p12` + `release.csr` + 两个密码 |
| 4 | AGC → 用户与访问 → 证书管理 → 申请证书（**发布证书**）→ 上传 `release.csr` | `release.cer` |
| 5 | AGC → Profile 管理 → 申请 Profile（**发布 Profile**，绑定上面的证书与包名） | `release.p7b` |
| 6 | AGC → 用户与访问 → API 密钥 → 创建客户端（勾选发布相关权限） | `APPGALLERY_CLIENT_ID` / `APPGALLERY_CLIENT_SECRET` |

第 3 步，不使用 DevEco Studio：

```bash
keytool -genkeypair -keystore release.p12 -storetype PKCS12 \
  -alias harmony-release -keyalg EC -groupname secp256r1 \
  -sigalg SHA256withECDSA -validity 9125 \
  -dname "CN=OpenLess, O=<你的组织>, C=CN"
keytool -certreq -keystore release.p12 -alias harmony-release -file release.csr
```

请将 `release.p12` 和两个密码离线保存；它们是 AppGallery 将会信任的私钥的唯一副本。

在上传任何内容之前，先试运行校验材料：

```bash
bash openless-all/app/scripts/check-harmony-signing.sh \
  ~/.local/share/harmony/signing/release release
```

它会证明四件否则会在后期才失败的事情：证书属于该密钥库中的密钥、Profile 是为该证书签发的、`bundle-info.bundle-name` 等于 `harmony/AppScope/app.json5`，以及没有任何内容已过期。工作流在开始构建之前会运行同样的检查。

### 将它们存储在仓库中

```bash
base64 -w0 release.cer       # → HARMONY_RELEASE_CERT_BASE64
base64 -w0 release.p7b       # → HARMONY_RELEASE_PROFILE_BASE64
base64 -w0 release.p12       # → HARMONY_RELEASE_KEYSTORE_BASE64
```

`HARMONY_<CHANNEL>_*` 这样的名称可以让生产材料和测试材料共存。与渠道无关的 `HARMONY_CERT_BASE64` / `HARMONY_PROFILE_BASE64` / `HARMONY_KEYSTORE_BASE64` 仍被接受，并在未设置渠道专用密钥时使用。

### 工作流会做什么，以及它不能做什么

自动化：构建 → 签名 → 校验签名 → 作为 AppGallery *候选*包上传 → 将**未签名**的 HAP 附加到 GitHub Release。

始终手动：AGC 要求人工填写商店列表（图标、截图、隐私政策 URL、软件著作权、分类与年龄分级）并点击提交审核。`scripts/upload-appgallery.mjs` 有意止步于替换候选包。

| 类型 | 名称 | 用途 |
|---|---|---|
| 密钥 | `HARMONY_KEYSTORE_BASE64` | 发布 `.p12` 密钥库的 base64 |
| 密钥 | `HARMONY_CERT_BASE64` | 发布 `.cer` 证书的 base64 |
| 密钥 | `HARMONY_PROFILE_BASE64` | 发布 `.p7b` 描述文件（provisioning profile）的 base64 |
| 密钥 | `HARMONY_KEYSTORE_PASSWORD` | 密钥库密码 |
| 密钥 | `HARMONY_KEY_PASSWORD` | 密钥密码（回退到密钥库密码） |
| 变量（可选） | `HARMONY_KEY_ALIAS` | 密钥库内的密钥别名；默认为 `harmony-<channel>` |
| 变量（可选） | `HARMONY_TOOLCHAIN` | 自托管 runner 上 DevEco / HarmonyOS SDK 的绝对路径 |
| 变量（可选） | `HARMONY_SDK_URL` | 覆盖工具归档的 URL |
| 变量（可选） | `HARMONY_SDK_SHA256` | 该归档的 SHA-256 |
| 变量（可选） | `HARMONY_AUTO_CHANNEL` | `none`（默认）/ `release` / `test` —— 自动标签构建派发所使用的渠道；`release` 或 `test` 会把已签名的 App Pack 作为候选包上传到 AppGallery（绝不提交审核） |

`test` 渠道读取同样的三个密钥（`HARMONY_KEYSTORE_BASE64`、`HARMONY_CERT_BASE64`、`HARMONY_PROFILE_BASE64`）。将测试 Profile 存放在那里，并通过以 `channel: test` 派发来切换渠道，这样一次只有一组密钥与正在构建的渠道匹配。
| 密钥 | `APPGALLERY_CLIENT_ID` | AppGallery Connect API 客户端 ID |
| 密钥 | `APPGALLERY_CLIENT_SECRET` | AppGallery Connect API 客户端密钥 |
| 密钥 | `APPGALLERY_APP_ID` | HarmonyOS 应用的 AppGallery Connect 应用 ID |

无需配置其他任何东西：构建会从校验和固定的镜像下载官方命令行工具，因此托管 runner 不需要变量、不需要自托管机器，也不需要华为账号。签名目录由工作流自身导出（`$RUNNER_TEMP/harmony-signing`），不得重新声明为 `env:` 键——YAML 的 `env` 优先级高于 `$GITHUB_ENV`，因此空占位符会遮蔽它，构建就会回退到一个不存在的目录。

该上传会替换 AppGallery Connect 中的候选包；它绝不会提交应用审核，因为 AGC 要求人工提交。在没有 `APPGALLERY_*` 密钥的情况下，工作流仍会将 HAP 附加到 GitHub Release，并报告商店上传已被跳过。

`openless-all/app/harmony/AppScope/app.json5` 中的 `versionName`/`versionCode` 是开发占位符；打包脚本会为构建重写它们（`versionName` 来自 `package.json`，`versionCode` 来自 `linux-egui/package-revision`，该值只增不减），并在之后恢复该文件。

## 发布说明与贡献者署名

每个发布页面都必须使用以下顺序：

1. **用户可见的功能与改进** —— 描述用户可以做什么，并在适用时说明平台和实验状态的边界。
2. **缺陷修复** —— 概述受影响的行为以及修正。
3. **架构变更** —— 解释相关的 Core/Host、数据、协议和依赖变更，不要把内部重构呈现为新的用户功能。
4. **构建与分发变更** —— 涵盖打包、CI、签名、更新器和分发工作。区分标签之后的构建工具修复与标签中包含的应用变更。
5. **PR 与参与者** —— 提供完整、带来源链接的署名索引。
6. **下载/升级说明与验证证据**。

对于 Stable，以**上一个公开发布的 Stable** 为对比基准，包括其间各个 Beta 累积的功能。未发布的草稿或预留的版本标签不是对比基准。对于 Beta，请说明用作基准的上一个公开 Beta。始终明确写出两个对比标签。

在**每个功能、修复、架构和构建条目**下，链接相关 PR 并标明其提交者。带链接的署名索引必须区分 PR 提交者、提交作者/共同作者、正式审阅者（包括审阅状态）、讨论参与者和合并者。必要时获取提交/审阅/评论的所有分页。未关联的 Git 作者名请按名字保留；不要为 bot/AI 签名虚构 GitHub 账号、角色、批准或人类身份。评论不是代码审阅批准，集成 PR 的参与者不一定是其中每个功能的作者。不要发布贡献者电子邮件地址。

对于通过 squash、cherry-pick 或集成分支纳入的工作，请同时注明**原始贡献 PR** 和**实际集成 PR/提交**。对已关闭但已集成的来源 PR 要准确标注；不要称其为独立合并。请从祖先关系、文件历史和集成记录来核实纳入情况，而不是仅凭 PR 标题或日期。如果某个 PR 不可用，请引用已核实的提交并记录署名的局限性。链接完整对比，并保留平台排除项（例如，Linux 源码工作并不意味着交付了 Linux 软件包）。

将发布页面的 Markdown 保存在 `docs/releases/<version>-release-notes.md`。在编辑已有的公开发布之前，先保存其当前正文和资产元数据；编辑之后，读回并校验正文、渠道/发布标志以及资产名称/摘要/大小。仅修改说明的变更不得重建软件包、移动标签或更改已发布的渠道。同样的格式适用于后续发布。

## 流程摘要

1. 通过 PR 将工作落到 `beta` 上（PR 针对 `beta` 开启，绝不针对 `main`）。
2. 对于 Stable 发布，由维护者合并 `beta → main`。
3. **管理员**升级 Tauri 版本（五个位置同步），确认 CI 通过，并推送发布标签，从而触发 macOS/Windows/Android 资产工作流及其渠道更新器清单。
4. 两个发布工作流都通过后，由管理员验证并发布共享草稿。只有在完成确切标签的 Linux 验收之后，Linux 软件包才会被独立附加。
