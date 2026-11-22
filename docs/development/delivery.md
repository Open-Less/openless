# 自动检查、签名与发布分发

发布权限与候选交付条件见[发布规范](../../RELEASING.zh.md)。版本说明随各版本发布。[GitHub Releases](https://github.com/Open-Less/openless/releases)

## 自动检查与缓存

[CI 工作流](../../.github/workflows/ci.yml)定义运行条件、runner 与命令。[发布规范](../../RELEASING.zh.md)定义打包、签名与发布门槛。

### 触发与门控

| 事件 | 行为 |
| --- | --- |
| PR 指向 `main` 或 `beta` | `Changed areas` 按改动范围选择平台检查。`ci:desktop-only` 在下次 PR CI 时跳过移动端和浏览器矩阵。标签变更本身不派发 CI。 |
| 推送 `main` 或 `beta` | 无 PR 比较基线，运行完整矩阵。 |
| 推送其他分支 | 通用 CI 由 PR 或手动运行触发。 |
| 手动运行 `CI` | 默认 `platform=all`。`platform=macos` 只运行 macOS 检查，可指定 `macos_runner=macos-15-intel` 或 `macos-latest`。 |
| `v*-tauri` 标签 | 独立发布流程。Linux 包单独验收并由管理员处理。 |

并发控制会取消同一 PR 的旧 CI，或同一 ref 与参数组合的旧手动 CI。默认分支基线、标签发布和草稿修复有各自的并发策略。无发布权限的验证或 Dev 打包使用分支工作流产物。

[`ci-changed-areas.sh`](../../openless-all/app/scripts/ci-changed-areas.sh)调用 [`ci-impact.mjs`](../../openless-all/app/scripts/ci-impact.mjs)分析改动。Rust 区域按 Cargo manifest、路径依赖和目标 `cfg` 闭包判断。前端、工具和原生工程由各自路径规则覆盖。Git 比较、Cargo 图或路径分类不确定时运行检查，只有明确的 `false` 可以跳过。

| 区域 | 主要检查 |
| --- | --- |
| `tauri` | macOS、Windows stable 构建与平台契约。macOS 包含原生启动／窗口验证所需的输入，Windows stable 包含 MSI 与 TSF 路径。 |
| `msrv` | macOS、Windows 分别用工作流固定的 Rust 1.99 验证。`Windows checks` 按是否应运行聚合 stable 与 MSRV。 |
| `frontend` | Chromium、WebKit 分开运行 `test:ui-motion`，聚合作业反映任一必需浏览器的失败。失败保留 trace 和截图。 |
| `android` | Android 目标编译、Kotlin／清单契约以及工作流配置的设备或模拟器检查。 |
| `harmony` | ArkTS 契约、Rust 宿主与 HAP。常规 CI 的 HAP 不签名。 |
| `capsule` | macOS 与 Windows 的原生胶囊独立检查，覆盖共享 crate 的 `ui,host` feature。 |
| `ios-device` | `test` 与 `device-build` 由 Xcode runner 分别验证，保留共享契约、模拟器与 iOS 真机目标编译的各自证据。 |

Linux 专属 CI 与自动发行已暂停。Linux 验收使用本地构建、Fcitx5 插件、打包脚本与[待补的设备证据](../requirements.md#linux)。Ubuntu 上的浏览器、Android 和 Harmony 检查独立运行。浏览器夹具和编译结果与系统权限、已安装应用、真机交互分别取证。

### 缓存

- `beta` 的合格 CI、手动构建和周期构建负责填充共享缓存，按各工作流的条件执行。PR 和常规 tag 发布以恢复为主。旧标签的修复可能使用源版本自带的缓存入口。
- Playwright 与移动端 Rime 使用分开的 restore/save 步骤，Rust 通过 `save-if` 门控。npm 复合 action 只在允许写入的基线运行启用自动保存，其他运行使用 restore。
- Rust 分平台和工具链缓存。macOS stable、MSRV 和 release 的编译配置不同，不能根据同一个 `shared-key` 推断命中。锁文件变化可以回退到旧前缀缓存，但不会重用不匹配的 Cargo 产物。Windows stable 与 MSRV 并行，前端验证复用 stable runner。
- Playwright 以浏览器引擎和版本拆键。Harmony SDK 从固定校验和的归档准备，Rust host 依赖单独缓存，避免 SDK 占用缓存预算。
- macOS 打包参数从 [`macos-build-env.sh`](../../openless-all/app/scripts/macos-build-env.sh)加载，包含 codegen、strip 和 proc-macro host wrapper。正式包走 [`build-mac.sh`](../../openless-all/app/scripts/build-mac.sh)，其中 Apple Silicon 的 Finder 布局在 DMG 压缩前写入并验收。

### 维护与验证

[缓存维护工作流](../../.github/workflows/cache-maintenance.yml)每天 03:17 UTC、`main`／`beta` PR 关闭后以及手动运行时执行。它按 8 GB 维护预算回收关闭 PR 的缓存、每个分支缓存族旧于两代的条目，以及继续超额时最久未使用的分支缓存。开放或状态不可读的 PR 与 tag 不参与清理。闭合事件读取默认分支脚本。删除需要 `actions: write` 与 `pull-requests: read`，由维护作业持有。

从仓库根目录预览清理计划：

```bash
bash openless-all/app/scripts/ci-cache-usage.sh --prune --keep-family 2 --prune-budget --limit-gb 8 --repo Open-Less/openless --dry-run
```

区域与缓存策略由 `ci-changed-areas.test.mjs`、`ci-cache-sharing.test.mjs`、`ci-cache-usage.test.mjs` 及 `ci-consolidation.test.mjs` 检查。MicYou 更新由独立的[上游导入工作流](../../.github/workflows/sync-micyou.yml)提出草稿 PR。源码 pin、补丁与校验流程见 [MicYou 上游说明](../../openless-all/app/crates/micyou-dsp/UPSTREAM.md)。

## 签名与分发

本页维护发布资产、签名索引和 OSS 更新镜像的运维合同。历史 GitHub Release 附件保持原有格式。

### 单一 Release 入口，平台并行

`v*-tauri` 标签只触发 `.github/workflows/release-tauri.yml`，显示为 `Release (all platforms)`：

```text
tag → prepare-release（仅创建／验证共享草稿，不构建）
        ├─ desktop matrix（macOS ARM / Intel / Windows）→ desktop publish ┐
        ├─ Android plan → 四 ABI matrix → Android publish               ├─ 同一草稿
        └─ HarmonyOS build → HAP 上传／可选商店候选包                    ┘
```

- 三条构建线只依赖同一个轻量预检，不相互依赖；移除原先桌面发布后才 dispatch Harmony 的等待及 Harmony 等待桌面创建 Release 的轮询。
- Android／Harmony 使用同一 run 的本地 `workflow_call`，定义来自标签的不可变提交。Android 删除独立 tag push，避免同一标签重复构建四个 ABI；独立周检、debug／手动入口保留。
- 预检遇到已有已发布 Release、错误 tag／渠道、API 错误时停止，不重开或修改历史版本；已有草稿只读取，不覆盖正文和附件。
- 草稿必须用 `gh release view <tag>` 解析：REST 的 `GET /releases/tags/<tag>` 对真实草稿返回 `404`，旧实现会误判为“尚无 Release”而重复创建，签名汇总也会误以为没有旧索引而覆盖另一平台记录。查询失败只接受 `release not found`／`HTTP 404`；其他错误直接失败，不创建。索引附件按 `assets[].apiUrl` 下载，不用 `id`（那是 GraphQL node ID，不是 REST 数字 ID）。
- 桌面下载器只匹配 `openless-macos-*`、`openless-windows-*`，避免合并 run 后重复下载数百 MB 的 APK／HAP，且不会混入商店签名包。
- 桌面和 Android 的发布 job 仍共享签名索引互斥锁，防止读改写覆盖；锁覆盖发布段，不阻塞构建。Harmony 不写该索引，不需等待它们。编译并行度仍默认 2，不额外拆分构建矩阵。
- 一个平台失败不取消其他平台的构建；整个 Release run 会报失败，产物保留在草稿中。仍由管理员确认后发布，OSS 清单在发布之后同步，不能为了并行而先指向不存在的包。
- 桌面分支／周检和 Windows 草稿维修在 tag-only 预检跳过时仍可运行，不会顺带启动 Android／Harmony 发布。Harmony 手动维修入口及渠道校验保留。



### 单一签名 JSON

新 Release 只附加 `updater-signatures.json`，格式：

```json
{
  "schema": 1,
  "release_tag": "v2.0.2-Beta.7-tauri",
  "artifacts": {
    "OpenLess_2.0.2-Beta.7+build.example_x64-setup.exe": {
      "signature": "原始 Tauri Base64 Minisign 签名",
      "size": 123,
      "sha256": "安装包 SHA-256"
    }
  }
}
```

- 构建过程仍由 Tauri／`tauri signer sign` 生成临时独立 `.sig`，CDN 清单继续内嵌原签名，不变更密钥、签名算法或验签标准。
- 桌面与 Android 的发布 job 共用 `openless-release-assets-<tag>` 并发锁；每个发布者读入草稿上已有索引，合并自己平台，再上传同一个 JSON，不能覆盖丢失另一个平台的记录。不新增聚合 VM。
- 合并成功后只删除本机 staging 中的独立 `.sig`，不删除远端历史附件。历史草稿若已有 `.sig` 而无索引，维修时保持旧格式；已发布的 Release 拒绝修改签名索引。草稿查询走 `gh release view`（REST tag 端点对草稿返 404），索引附件按其 `apiUrl` 下载。
- 新客户端优先解析同一 Release 的索引，检查 schema、标签和精确文件名；历史 Release 仍读取独立 `.sig`。最终必须用内置公钥验证包本身与 trusted comment，不能把索引里的 SHA-256 当成来源认证。
- **旧客户端兼容限制**：已安装、只认识独立 `.sig` 的客户端仍可通过 CDN 内嵌签名更新，但无法在 CDN 完全不可用时独立发现只附加索引的新 Release。这不能靠修改未来发行文件修复；需先在 CDN 可用时升级到支持索引的新客户端。历史 Release 原有 GitHub 路径不变。

### 更新发现与下载回退

桌面和 Android 并行检查选定渠道的 CDN 清单与 GitHub Release。桌面按版本优先级选候选，优先级相同时根据有界下载探测估算时间。Android 校验渠道、设备架构及签名版本，GitHub 查询失败且 CDN 未提供可更新版本时提示无法确认最新版本。选中候选后，再确定用于同一签名包的下载来源。[桌面入口](../../openless-all/app/src-tauri/src/commands/settings.rs) · [Android 入口](../../openless-all/app/src-tauri/src/android/update_pipeline.rs)

- 即使新版本先由 GitHub 发现，也先尝试同标签、同文件名的阿里云对象。
- CDN HEAD 不成功时，403／405／重定向使用有超时、来源限制的 Range 探测；缺失、超时、网络失败则选择 GitHub。
- 映射仅允许官方 HTTPS 地址，拒绝用户信息、查询参数、错误端口、额外路径和编码后的路径穿越；`+` 与 `%2B` 对应同一原包。
- 桌面在存入 Tauri resource 前选择源。若阿里云在探测成功后中断或验签失败，前端只重试下载：后端克隆原 resource 的相同版本和签名，改成 GitHub URL。安装失败不触发换源安装重试。
- Android 对相同候选依次尝试阿里云和 GitHub，每次完整传输后均验签；超时、流中断、长度不匹配或签名失败均不能把坏包交给安装器。
- 阿里云传输总预算 120 秒，GitHub 600 秒；Android 既有整体下载与分块停滞预算仍保留。两源都失败则明确报错，不声称更新成功。

### OSS 同步

实现：`.github/workflows/mirror-release.yml` 和 `openless-all/app/scripts/mirror-release.py`。

触发：Release published／edited、手动／repository dispatch，以及每 6 小时的补偿调度。必须合并到默认分支才能启用事件和调度。只执行默认分支的工具，不用带 OSS 密钥的 job 执行 Release 标签内代码。

1. 读取已发布的最新 Stable 和 Beta；不处理草稿。网站旧任务每 15 分钟下载 3+3 个标签的方式不再沿用。
2. 镜像它们的实际附件，校验下载长度与 GitHub 提供的 SHA-256。
3. 用索引（新格式）或独立 `.sig`（历史格式）获取签名，用内置公钥运行 Minisign 验证所有七个更新目标，并检查签名中的完整版本／文件名。
4. 两个 macOS、Windows NSIS 和四个 Android ABI 必须齐全；缺包、缺签名、校验失败时不推进该渠道。
5. 先把包上传到 `releases/<tag>/`，全部成功后才替换 `updates/<channel>/` 的滚动 JSON。包保持 immutable 缓存头，清单缓存 60 秒。每个对象的 PUT 是原子的，不声称七个平台的清单能事务性同时切换。
6. 使用 `rclone copy` 而非 `sync`／`purge`，不删除历史 OSS 对象，也不修改 GitHub 附件。无需重写或单独上传每个平台的 `.sig` 才能让新版 CDN 客户端更新。

#### 配置与恢复

主仓库镜像作业需要以下仓库 Secrets：

- `OSS_BUCKET`
- `OSS_ACCESS_KEY_ID`
- `OSS_ACCESS_KEY_SECRET`

可选仓库变量 `OSS_ENDPOINT` 默认使用 `oss-cn-zhangjiakou.aliyuncs.com`。Bucket 保持私有。凭据只放在仓库 Secrets。配置变更或漏掉发布事件时，手动运行主仓库的 `Mirror release assets to OSS`，然后检查七个更新目标的对象、清单与签名。流程读取已发布版本，无需创建新标签或重建安装包。网站的部署与 `release.json` 快照独立于二进制镜像。

### 验证边界

本地离线测试覆盖两发布者任意顺序合并、重跑不丢记录、旧格式维修不迁移、已发布拒绝改写、API 错误不覆盖、索引标签／缺记录错误、七目标生成、历史签名、缺包／摘要／验签失败不推进、下载中途错误后一次 GitHub 重试。Rust 直接包含生产模块的独立 harness 测试覆盖更新发现、签名解析、源合并与 APK 验签。

真实 OSS 上传、系统安装与网络故障回退须分别在远端或设备上验收。

## Homebrew 同步

### 公开版本自动同步

两个发版流水线先把桌面和 Android 资产上传到共享 draft。管理员确认两条流水线和安装包后，通过 GitHub UI 或管理员 `gh release edit` 发布。`release:published` 自动触发 `.github/workflows/sync-homebrew-cask.yml`，仅处理已公开、latest、非 Beta 的 `vX.Y.Z-tauri`。

脚本从 ARM/Intel DMG 的真实文件名读取完整版本，检查两个架构一致且与 tag 对应，再计算 SHA-256 并更新默认分支的 `Casks/openless.rb`。tag 不带 `+build`，DMG 文件名保留日期构建号；1.x 保持 Monterey，2.x 使用与 Host 对应的 Sonoma。

权限前提：当前 `Protect #beta` 规则仅允许 OrganizationAdmin bypass，默认 `GITHUB_TOKEN` 不能直推。无人值守同步须配置仓库 Secret `HOMEBREW_CASK_ADMIN_TOKEN`，由组织管理员提供、只授权此仓库 Contents read/write；不修改分支保护，不在聊天或日志暴露值。未配置且确有更新时工作流明确失败并提示，不能把准备好文件当作已同步；cask 已被管理员更新时则幂等通过。此 Secret 是外部配置前提，不由代码自证已经存在。

手动恢复同步（不重建包）：

```sh
gh workflow run sync-homebrew-cask.yml --repo Open-Less/openless -f tag=v2.0.0-tauri
```
