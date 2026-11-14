# Release 签名索引、OSS 同步与双源更新

本次在 PR #1426 中补充单一签名 JSON、从 `aeoform/openless-workspace` 迁移 OSS 同步，并适配 PR #1399 的阿里云优先／GitHub 二进制回退。只改变配置生效后的发行；不删除或迁移已有 GitHub Release 附件。

## 单一 Release 入口，平台并行

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

这里消除的是可确认的依赖等待，不承诺尚未实际发版测量的提速百分比。

## 单一签名 JSON

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

## 阿里云优先，GitHub 托底

保留当前独立 GitHub 版本发现、CDN 清单备份、精确版本绑定和 Android 防降级检查，不倒退到只能依赖 CDN 发现版本的旧架构。

- 即使新版本先由 GitHub 发现，也先尝试同标签、同文件名的阿里云对象。
- CDN HEAD 不成功时，403／405／重定向使用有超时、来源限制的 Range 探测；缺失、超时、网络失败则选择 GitHub。
- 映射仅允许官方 HTTPS 地址，拒绝用户信息、查询参数、错误端口、额外路径和编码后的路径穿越；`+` 与 `%2B` 对应同一原包。
- 桌面在存入 Tauri resource 前选择源。若阿里云在探测成功后中断或验签失败，前端只重试下载：后端克隆原 resource 的相同版本和签名，改成 GitHub URL。安装失败不触发换源安装重试。
- Android 对相同候选依次尝试阿里云和 GitHub，每次完整传输后均验签；超时、流中断、长度不匹配或签名失败均不能把坏包交给安装器。
- 阿里云传输总预算 120 秒，GitHub 600 秒；Android 既有整体下载与分块停滞预算仍保留。两源都失败则明确报错，不声称更新成功。

## OSS 同步迁入主仓库

实现：`.github/workflows/mirror-release.yml` 和 `openless-all/app/scripts/mirror-release.py`。

触发：Release published／edited、手动／repository dispatch，以及每 6 小时的补偿调度。必须合并到默认分支才能启用事件和调度。只执行默认分支的工具，不用带 OSS 密钥的 job 执行 Release 标签内代码。

1. 读取已发布的最新 Stable 和 Beta；不处理草稿。网站旧任务每 15 分钟下载 3+3 个标签的方式不再沿用。
2. 镜像它们的实际附件，校验下载长度与 GitHub 提供的 SHA-256。
3. 用索引（新格式）或独立 `.sig`（历史格式）获取签名，用内置公钥运行 Minisign 验证所有七个更新目标，并检查签名中的完整版本／文件名。
4. 两个 macOS、Windows NSIS 和四个 Android ABI 必须齐全；缺包、缺签名、校验失败时不推进该渠道。
5. 先把包上传到 `releases/<tag>/`，全部成功后才替换 `updates/<channel>/` 的滚动 JSON。包保持 immutable 缓存头，清单缓存 60 秒。每个对象的 PUT 是原子的，不声称七个平台的清单能事务性同时切换。
6. 使用 `rclone copy` 而非 `sync`／`purge`，不删除历史 OSS 对象，也不修改 GitHub 附件。无需重写或单独上传每个平台的 `.sig` 才能让新版 CDN 客户端更新。

### 必须人工完成的切换前置条件

主仓库必须配置以下三个仓库 Secrets（本次已从历史会话恢复配置；值不写入仓库）：

- `OSS_BUCKET`
- `OSS_ACCESS_KEY_ID`
- `OSS_ACCESS_KEY_SECRET`

可选仓库变量 `OSS_ENDPOINT`，默认 `oss-cn-zhangjiakou.aliyuncs.com`。Bucket 继续私有，不将凭据放入代码、日志或普通变量。GitHub API 无法读回网站仓库已有 Secret 的值；本次恢复使用的是用户先前提供的原始配置，已通过标准输入写入主仓库 Secrets，未回显或落盘密钥。配置存在不等同于 OSS 同步验收通过。

切换顺序：

1. 在主仓库配置三个 Secrets，合并本 PR。
2. 手动运行主仓库 `Mirror release assets to OSS`，验证七个平台清单、对应对象与签名；无需新发版即可读取现有发布版。
3. 再合并网站仓库的迁移 PR，删除旧镜像 workflow 和重复生成器；保留网站部署与 release.json 快照。

网站仓库的迁移分支不会自动禁用线上旧任务，避免凭据未就绪时切断现有更新。

## 验证边界

本地离线测试覆盖两发布者任意顺序合并、重跑不丢记录、旧格式维修不迁移、已发布拒绝改写、API 错误不覆盖、索引标签／缺记录错误、七目标生成、历史签名、缺包／摘要／验签失败不推进、下载中途错误后一次 GitHub 重试。Rust 直接包含生产模块的独立 harness 测试覆盖更新发现、签名解析、源合并与 APK 验签。

真实 OSS 上传、桌面系统安装、Android 设备安装及真实网络故障注入仍需远端／设备验收；未在本地调用 OSS 写入或触发产品发行。
