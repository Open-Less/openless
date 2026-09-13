# 云同步服务端交接

状态：canonical（客户端合同 v1 已实现并锁定）；更新：2026-09-10。读者：云同步服务端的实现与部署者。客户端实现见 [`cloud_sync.rs`](../openless-all/app/crates/openless-core/src/cloud_sync.rs)，DTO 见 [`cloud_sync_types.rs`](../openless-all/app/crates/openless-core/src/cloud_sync_types.rs)；数据范围与字段表见 [官方云同步](cloud-sync.md)。服务器端代码由服务端负责人编写，本文只约定客户端发出的请求与它接受的响应。

## 服务地址（客户端已内置）

- 基地址：`https://apic.openless.top:9443`，接口路径 `/me/sync`，完整地址 `https://apic.openless.top:9443/me/sync`。
- 与风格包市场后端 `https://apic.openless.top` 同一台服务器，但使用独立端口 **9443**，不经过市场后端的 443 路由。服务端需在该端口提供 TLS（可由 nginx 独立 `server` 块终止后反代到本机同步进程，同步进程本身只听 HTTP，如 `127.0.0.1:9443` 内部端口由部署自定）。
- 客户端常量 `CLOUD_SYNC_BASE_URL` 在 [`marketplace.rs`](../openless-all/app/crates/openless-core/src/marketplace.rs)，主机名必须与市场地址一致，端口如需调整须同步修改该常量并重跑合同测试。
- 客户端强制：正式地址必须 HTTPS；仅回环地址（127.0.0.1、localhost）允许 HTTP，用于本机联调。请求不携带用户名/密码。

## 客户端请求行为

所有方法（GET/PUT/DELETE）行为一致：

| 项 | 值 |
| --- | --- |
| 认证 | `Authorization: Bearer <GitHub OAuth access token>`；服务端应每次经 GitHub `GET /user` 校验，只使用 numeric id 归属数据 |
| 重定向 | 客户端使用 `Policy::none`，任何 3xx 一律报错；服务端不得重定向 |
| 超时 | 30 秒 |
| Content-Type | PUT/DELETE 请求体为 `application/json` |
| 请求体上限 | 2 MiB（超过返回 413） |
| 重试 | 客户端不自动重试任何方法；GET 网络失败视为服务不可用，PUT/DELETE 网络失败视为"结果未知" |
| 缓存 | 响应应带 `Cache-Control: no-store` |

## 接口语义

合同版本 `schemaVersion: 1`。快照对象：

```json
{"schemaVersion":1,"revision":2,"updatedAt":"2026-09-10T08:00:00+00:00","payload":{…}}
```

- `revision` 从 0 开始，非负且不超过 `9007199254740991`（JavaScript 安全整数上限）。`revision=0` 当且仅当 `payload=null` 且 `updatedAt=null`（从未上传或已删除且无历史）。删除不清零版本：删除后返回递增版本和 `payload:null`。
- `updatedAt` 为服务端生成的 RFC3339 时间戳；`revision>0` 时必须存在且可解析。
- `payload` 为完整替换，不做字段级合并；四个集合/对象 `dictionary`、`corrections`、`stylePacks`、`preferences` 必须同时出现。

### GET /me/sync — 读取

首次使用返回 `200` 与空快照：`{"schemaVersion":1,"revision":0,"updatedAt":null,"payload":null}`。这不是错误，客户端据此显示"暂无云端备份"。

### PUT /me/sync — 整体上传

请求：

```json
{"schemaVersion":1,"baseRevision":2,"payload":{…完整 payload…}}
```

- `baseRevision` 是该设备最后一次读取的版本。服务端必须原子比较：`baseRevision == 当前版本` 才写入，成功后版本递增、刷新 `updatedAt`，返回 `200` 与完整新快照（含已保存 payload）。
- `baseRevision` 不匹配返回 `409`，响应体：

```json
{"error":"revision_conflict","message":"…","revision":3,"updatedAt":"…"}
```

客户端从该响应读取 `revision` 字段作为 `currentRevision` 提示用户刷新；冲突响应不得包含私有 payload。
- `baseRevision=0` 且云端确无记录时创建首份快照（版本 1）。
- `schemaVersion` 不是 1 返回 `400`（`unsupported_schema_version`）。

### DELETE /me/sync — 清空云端

请求 `{"baseRevision":2}`；成功返回递增版本、`updatedAt` 更新、`payload:null` 的快照。对不存在（`revision=0`）的快照执行删除且 `baseRevision=0` 时，按合同创建版本 1 的空记录。本机数据由客户端自行保留，服务端只管云端。

## 响应校验（服务端返回给客户端的内容同样受限）

客户端用与上传完全相同的严格 DTO 校验每个响应：camelCase、拒绝未知字段（`deny_unknown_fields`）、限额同请求。响应体读取上限为 2 MiB + 1 KiB。`payload` 各集合限额：词典 10000 条、纠正规则 2000 条、风格包 200 个；ID 1–128 个 ASCII 字母数字及 `._-:`（不允许单独 `.`/`..`，集合内唯一）；图标为标准 base64 PNG，解码后 ≤ 64 KiB、宽高 1–1024 像素；完整字段与字节限制见 [官方云同步](cloud-sync.md) 与 3-backend 仓库 `docs/cloud-sync.md`（合同同源）。

## 状态码 → 客户端行为

| 服务端返回 | 客户端表现 |
| --- | --- |
| 200 | 正常解析快照 |
| 401 / 403 | 判定 GitHub 登录失效，要求重新登录 |
| 404 / 405 / 501 | "官方同步服务暂不可用"（服务未部署时 UI 的默认表现，不得被当成空备份） |
| 409 | 版本冲突，提示刷新云端状态后重新选择 |
| 400 / 413 / 415 及其他 4xx/5xx | 通用失败提示，附 HTTP 状态码，本机数据不变 |
| 网络失败/超时 | GET：服务不可用；PUT/DELETE：结果未知，要求先刷新状态 |

400 应使用 JSON 错误体（`{"error":"invalid_payload"}` 等），客户端不解析其内容，仅按状态码处理。

## 服务端参考实现

3-backend 仓库已含一份与本文同合同的参考实现（`backend/src/routes/sync.rs`、迁移 `0006_user_cloud_sync.sql`、文档 `docs/cloud-sync.md`），尚未部署到生产；可部署、改造或重写，但对外行为须满足本文全部条款。

## 联调与验收清单

1. 无服务时访问 `GET https://apic.openless.top:9443/me/sync` 应 404，客户端显示"官方同步服务暂不可用"（回归基线）。
2. 有效 GitHub token：GET 返回空快照 → PUT 保存 → GET 读回一致 → 再 PUT 带旧 `baseRevision` 收到 409 → DELETE 清空后快照 `payload:null` 且版本递增。
3. TLS 证书对 `apic.openless.top` 有效；HTTP 明文访问 9443 不得 200。
4. 超过 2 MiB 的 PUT 收到 413；未知字段收到 400。
5. 应用内验证：设置 → 权限与数据 → 云同步，完成备份/恢复/删除各一次，恢复后词典、纠正规则、风格包与偏好生效，API 密钥与设备设置不变。

客户端合同测试（本机假服务）与生产地址固定测试随 `cargo test -p openless-core --locked` 运行；服务端部署验收需另行按平台执行。
