# OpenLess 内置 PI 后端

基于官方 `@earendil-works/pi-coding-agent` **0.85.1** SDK，固定依赖和 `package-lock.json`。运行时随安装包附带 Node.js（至少 22.19.0）、完整 npm 依赖和 `openless-computer`，用户无需全局安装 PI、Node、Python。此目录为独立后端，不参与前端打包。

```powershell
npm ci --ignore-scripts
node index.mjs --health
npm test
```

## 配置模型

私有目录优先取 `OPENLESS_PI_AGENT_DIR`（兼容 `OPENLESS_PI_HOME`），否则分别为：

- Windows：`%APPDATA%/OpenLess/pi`
- macOS：`~/Library/Application Support/OpenLess/pi`
- Linux：`${XDG_CONFIG_HOME:-~/.config}/openless/pi`

将 `config.example.json` 复制为该目录下的 `config.json`，填模型 ID、兼容 OpenAI 的 API URL 和 `apiKeyEnv` 对应环境变量。也可用 `apiKey` 字段直接保存自己的密钥；不要将私人配置提交仓库。官方 provider 可用 `{"provider":"openai","model":"模型ID","apiKeyEnv":"OPENAI_API_KEY"}`，不指定 `baseUrl`。`supportsImages` 为 `false` 的纯文本模型无法解读截图；桌面控制应选择支持图像和工具调用的模型。

支持 PI 标准私有 `auth.json`、`models.json` 和 provider 环境变量，例如 `OPENAI_API_KEY`、`ANTHROPIC_API_KEY`。SDK 在此版本以 `ModelRuntime` 管理认证，代替旧 `AuthStorage`。不会读取全局 `~/.pi/agent` 配置，也不会自动加载项目或用户的扩展、技能和 shell。请求的 `model` 优先于 `config.json`；省略则用配置模型或首个已认证模型。`models.json` 按官方 PI 格式可配置多个 provider。

`--capabilities` 返回实际私有路径；`--list-models` 每行返回一个 `provider/model`，包括已配置兼容服务；`--version` 无副作用；`--health` 校验 SDK 可加载，不连接模型或桌面。

## 宿主协议

启动 `node index.mjs --request`，向 stdin 写一个 UTF-8 JSON 对象，可带 LF，随后关闭 stdin。prompt、输入文本、API 密钥均不得放入命令行参数。

```json
{"type":"prompt","session_id":"host-run-id","prompt":"查看当前屏幕","cwd":"绝对工作目录","model":"provider/model","permission_mode":"plan","allowed_tools":[],"disallowed_tools":[],"session_persistence":true,"continue_session":false,"timeout_secs":300}
```

可选 `extra_system_prompt`、`continuation_context`。stdout 仅输出每行一个 JSON：

```json
{"type":"started","session_id":"host-run-id"}
{"type":"delta","text":"正在查看"}
{"type":"tool_use","id":"tool-1","name":"computer_screenshot","input":{}}
{"type":"tool_result","id":"tool-1","name":"computer_screenshot","is_error":false}
{"type":"complete","text":"完整输出","session_id":"host-run-id","cost_usd":0}
```

失败返回 `{"type":"error","message":"..."}` 且退出状态 1。`tool_result` 不重复输出图片和文件内容。宿主可忽略未知事件。若保持 stdin 打开，可传 `{"type":"cancel"}`；SIGINT、SIGTERM 同样取消，宿主仍需用 Unix 进程组 / Windows Job 管理整棵进程树。Computer 子进程不使用 detached 或 shell，继承父进程的组/Job。进程完成后退出；会话可保存到私有目录并按工作目录继续。

## Computer 工具与权限

`OPENLESS_COMPUTER_BIN` 必须为安装包内 helper 的绝对路径。注册 `computer_capabilities`、`computer_displays`、`computer_screenshot`、`computer_move`、`computer_click`、`computer_scroll`、`computer_key`、`computer_type_text`。通过 helper stdin/EOF 传 JSON，不经 shell。截图以 PI 的 image content 直接传模型；坐标与返回截图保持一致，不额外乘缩放因子。

`plan` 只读；`acceptEdits` 允许桌面动作及工作目录内的文件编辑；无审批通道的 `default` 和历史 `bypassPermissions` 收敛为 `plan`。桌面输入会影响其他应用，工作目录限制仅针对文件工具。文件工具提供 `read`、`ls`、`find`、`write`、`edit`，以 UTF-8 操作，拒绝工作目录外路径、符号链接、凭据文件及启动配置写入。系统 shell 工具不注册。`allowed_tools`/`disallowed_tools` 支持工具名、`Computer`/`computer_*`、旧 `Read`/`Write` 形式及参数化文件路径拒绝规则；拒绝优先。

本包自有测试不调用真实模型和桌面，包括 JSONL/UTF-8、权限与路径限制、截图 content、流式事件、失败/取消及真实 SDK 会话初始化。

官方参考：[SDK](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sdk.md)、[模型配置](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/models.md)。以锁定 npm 包实际导出的 API 为准。
