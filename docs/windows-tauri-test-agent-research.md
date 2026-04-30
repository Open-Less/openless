# Windows Tauri 测试 Agent / Workflow 调研

## 背景

OpenLess 是 Tauri v2 + React/Vite + Rust 后端的桌面应用。Windows 真机问题主要集中在：

- 启动首屏：空边框、白屏、前端首帧前窗口过早显示。
- 系统能力：全局热键、麦克风隐私权限、剪贴板、前台输入框插入。
- 本地状态：凭据读写、历史记录、设置保存。
- 人工输入：物理热键无法用普通 synthetic SendInput 可靠替代。

## 可复用方案

### 1. 官方 tauri-driver + WebDriver

来源：

- https://v2.tauri.app/develop/tests/webdriver/
- https://github.com/tauri-apps/webdriver-example

适合做 CI 基线：

- 启动 Tauri 应用。
- 检查窗口出现、DOM 内容、按钮点击、设置页导航。
- Windows CI 可配 `msedgedriver`，Linux CI 可配 `webkit2gtk-driver + xvfb`。

参考 workflow：

- `tauri-apps/webdriver-example/.github/workflows/webdriver-v2.yml`
- 该 workflow 在 `ubuntu-latest` 和 `windows-latest` 上安装 `tauri-driver`，Windows 侧安装 `msedgedriver`，再分别跑 selenium / webdriverio 测试。

建议落地：

- 先选 WebdriverIO，生态和断言更贴近前端团队。
- 新增 `openless -all/app/webdriver/`，覆盖：
  - 应用启动后 1 秒内出现 OpenLess UI。
  - 设置页提供商字段能读出已存在凭据的“非空状态”。
  - 打开设置页不会把未修改字段写回空值。
  - 权限页 Windows 文案不出现 macOS 辅助功能授权提示。

### 2. tauri-plugin-playwright

来源：

- https://docs.rs/crate/tauri-plugin-playwright/0.1.0

适合做更接近 Playwright 的 E2E：

- 在 Tauri app 内嵌控制 server。
- 使用 Playwright API 做页面级断言。
- 对前端团队迁移成本较低。

风险：

- 需要引入 Tauri plugin，测试入口和生产入口要隔离。
- 目前生态成熟度低于官方 WebDriver。

建议落地：

- 暂不作为第一阶段 CI 基线。
- 等 WebDriver 跑通后，再评估是否用它补截图、网络、前端状态断言。

### 3. Tauri MCP / AI Agent 调试插件

来源：

- https://github.com/P3GLEG/tauri-plugin-mcp
- https://github.com/dirvine/tauri-mcp

适合做 agent 辅助调试：

- 截图。
- 窗口管理。
- DOM 读取。
- 鼠标/键盘输入。
- localStorage 检查。

风险：

- 需要在 app 中接入调试插件，必须确保只在 dev/test 构建启用。
- 不适合直接放进 production bundle。

建议落地：

- 可以做 `devtools/agent` 分支实验。
- 目标是让 Codex/Claude/Cursor 能直接看 Tauri 窗口截图和 DOM，降低“用户肉眼测试”的比例。

### 4. TestDriver AI

来源：

- https://testdriver.ai/vscode
- https://github.com/testdriverai/testdriverai

适合黑盒探索：

- 用自然语言描述流程。
- 支持桌面应用、Windows、GitHub Actions。
- 能生成测试报告/视频。

风险：

- 外部服务/账号/成本依赖。
- 对本项目当前开源 CI 基线不应作为唯一门禁。

建议落地：

- 作为 nightly 或人工触发探索测试，不作为 PR 必过的第一层。
- 可覆盖“打开 OpenLess Dev、进入设置、检查凭据字段非空、按热键后胶囊状态变化”等高层流程。

## 推荐实施顺序

1. 保留现有 PowerShell smoke：构建、启动、进程响应、hotkey listener 日志。
2. 增加 WebDriverIO 基线：窗口、DOM、设置页、凭据字段非空状态、Windows 文案。
3. 增加 Windows 手动门禁脚本：物理热键、真实 ASR、Notepad fallback、麦克风隐私开关。
4. 评估 Tauri MCP：给 agent 提供截图/DOM/输入能力，减少人工描述。
5. 评估 TestDriver AI：做黑盒探索和视频报告。

## 第一批必须补的测试

- 启动首屏不能先显示空窗口边框。
- Windows 启动不等待麦克风 input stream 探测。
- 设置页凭据字段加载完成前 blur 不会保存空值。
- 设置页打开后不修改字段，`credentials.json` 不发生变化。
- `get_credentials` 与 `read_credential` 对同一文件返回一致状态。
- 右 Control 默认热键文案在概览、历史、设置中一致。
- Windows 权限页不显示 macOS 辅助功能授权引导。
