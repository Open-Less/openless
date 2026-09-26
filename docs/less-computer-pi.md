# Less Computer：内置 PI 后端

桌面安装包包含 PI SDK、独立 Node.js 运行时和原生 `openless-computer` 工具。用户不需要另外安装 PI、Node.js、Python 或 Computer MCP。已有外部 Claude Code / OpenCode / Codex / dsh 配置仍可使用；新安装的默认 Agent 后端为 `pi-bundled`。

## 使用

1. 在设置中启用 Less Computer，后端选择 **PI**。
2. 配置模型服务凭据。可使用标准模型服务环境变量（例如 `OPENAI_API_KEY`），也可使用下面的封装 PI 配置。模型必须支持图像输入，才能理解桌面截图。
3. 选择“允许操作桌面与编辑文件”，打开 Less Computer 面板，输入或说出任务。选择“只读”时，仅允许查看文件和截图。
4. 运行中按 Esc 或使用取消按钮可停止当前任务及其子进程。关闭会话会清除续聊上下文。

模型名称使用 `provider/model` 格式；留空使用封装 PI 的配置。没有有效凭据时会返回配置错误，不会报告桌面任务成功。

## 模型配置

封装 PI 使用自己的配置目录，可用 `OPENLESS_PI_AGENT_DIR` 指定绝对路径。默认目录：

| 系统 | 目录 |
| --- | --- |
| Windows | `%APPDATA%\OpenLess\pi` |
| macOS | `~/Library/Application Support/OpenLess/pi` |
| Linux | `${XDG_CONFIG_HOME:-~/.config}/openless/pi` |

在该目录创建 UTF-8 编码的 `config.json`，例如使用 OpenAI 兼容服务：

```json
{
  "provider": "openless",
  "model": "your-vision-model",
  "baseUrl": "https://your-provider.example/v1",
  "api": "openai-completions",
  "apiKeyEnv": "OPENAI_API_KEY"
}
```

凭据可通过环境变量提供；`config.json` 也支持 `apiKey` 字段，该字段会以明文存储在本机，请仅使用个人可读的目录，不要将真实配置放进源码或安装包。完整配置约定及标准 PI `models.json` / `auth.json` 支持见 [PI runtime 文档](../openless-all/app/pi-backend/README.md)。安装包不含模型凭据，也不会自动选择工作目录里的扩展或脚本。

## 平台范围

| 平台 | 原生操作 | 首次运行条件 |
| --- | --- | --- |
| Windows | 截图、显示器查询、鼠标、滚轮、按键、Unicode 文本 | 普通用户交互式桌面；不能跨越 UAC 安全桌面 |
| macOS | 同上 | 系统设置中允许屏幕录制与辅助功能 |
| Linux X11 | 同上 | 可访问当前 X11 会话 |
| Linux Wayland | 当前不提供桌面输入控制 | 返回明确的不支持信息；可切换到 X11 会话 |

本次内置运行时针对 Windows、macOS 和 Linux 桌面发行版。Android / iOS 不打包桌面运行时。平台条件由原生工具报告，工具失败会返回错误，不会退化成执行任意 shell 指令。

## 构建和分发

在 `openless-all/app` 中执行：

```sh
npm ci
node scripts/prepare-pi-backend.mjs
npm run build
```

准备脚本下载固定版本 Node 并验证固定 SHA-256，使用 lockfile 安装 PI 生产依赖，编译原生 Computer 工具，再进行无桌面操作的健康检查。只有检查通过才写入 `src-tauri/resources/pi-backend`。后续构建按源码指纹复用缓存，首次构建需要网络。

macOS / Windows 的 Tauri 开发和发布构建会自动调用准备脚本。安装资源包含：

```text
pi-backend/
  node / node.exe
  openless-computer / openless-computer.exe
  NODE-LICENSE
  manifest.json
  runtime/
    index.mjs
    package.json
    node_modules/
    src/
```

Windows 资源位于应用可执行文件旁；macOS 位于 `Contents/Resources/pi-backend`；Linux 安装到 `usr/lib/openless/resources/pi-backend`。Linux 独立打包脚本和发布 workflow 会包含同一套资源及必要动态库。请在目标系统和 CPU 架构的构建机运行，不能把 Windows 的 npm 原生依赖复制进 macOS 或 Linux 安装包。

Windows 构建需要 MSVC C++ Build Tools 与 Windows SDK。Linux 原生工具需要 X11 / PipeWire / Wayland / GBM 开发库；完整依赖在 Linux 发布 workflow 中维护。macOS 构建会为 Node 添加 JIT 所需 entitlement，并对嵌套可执行文件签名。

Windows 一键构建完整安装包（同时编译输入法组件）：

```powershell
./scripts/build-windows-pi.ps1
```

三平台后端测试和独立后端构建由 `.github/workflows/pi-computer.yml` 提供；工作流只生成构建产物，不发布版本。

## 开发验证

```sh
cargo test -p openless-core --lib
cargo test -p openless-computer
node --test scripts/prepare-pi-backend.test.mjs
npm --prefix pi-backend test
npm run build
```

`openless-computer --capabilities`、`runtime/index.mjs --health` 不截图、不点击、不输入，可用于安装包健康检查。后端请求通过 stdin JSON 传递，用户任务和凭据不进入命令行参数。原生工具的具体动作协议见其 [README](../openless-all/app/crates/openless-computer/README.md)。
