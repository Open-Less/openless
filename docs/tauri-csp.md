# Tauri CSP 边界

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。

桌面 WebView 的 CSP 定义在 `openless-all/app/src-tauri/tauri.conf.json`（`app.security.csp`），当前值为：

```
default-src 'self' customprotocol: asset:
script-src 'self'
style-src 'self' 'unsafe-inline' https://fonts.googleapis.com
font-src 'self' https://fonts.gstatic.com
img-src 'self' asset: http://asset.localhost blob: data: https://github.com https://avatars.githubusercontent.com
connect-src 'self' ipc: http://ipc.localhost http://localhost:1420 ws://localhost:1420
media-src 'self' data: asset: http://asset.localhost blob:
object-src 'none'
base-uri 'none'
form-action 'none'
frame-ancestors 'none'
```

## 放开项与用途

| 指令 | 用途 |
| --- | --- |
| `default-src 'self' customprotocol: asset:` | 仅打包产物与 Tauri asset 协议 |
| `script-src 'self'` | 脚本只允许应用自带前端产物（Tauri 构建时注入 nonce/hash） |
| `style-src` + Google Fonts | 界面样式与在线字体 |
| `img-src` 的 github/avatars | GitHub OAuth 头像（`GithubLoginModal`）；`asset:/asset.localhost/blob:/data:` 服务于本地资源与音频波形 |
| `connect-src` 的 `ipc:/ipc.localhost` | Tauri IPC；`localhost:1420` 仅为 Vite 开发/HMR |
| `media-src` | 录音回放与提示音 |

`object-src`、`base-uri`、`form-action`、`frame-ancestors` 全部关闭。

## 修改规则

新增外部来源必须先确认用途与最小化范围（只加到对应指令），不得放宽 `object-src/base-uri/form-action/frame-ancestors`；改动随 `npm test` 与安全审查一起验证。
