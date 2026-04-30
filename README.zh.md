<p align="center">
  <img src="Resources/AppIcon.png" alt="OpenLess" width="160" />
</p>

<h1 align="center">OpenLess</h1>

<p align="center">
  <strong>开源语音输入，支持 macOS 和 Windows。</strong><br/>
  按一次快捷键说话，AI 润色后的文字直接落到当前光标。
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh.md">中文</a>
</p>

<p align="center">
  <a href="https://github.com/appergb/openless/releases/latest"><img alt="release" src="https://img.shields.io/github/v/release/appergb/openless?style=flat-square&color=2c5282" /></a>
  <a href="https://github.com/appergb/openless/blob/main/LICENSE"><img alt="license" src="https://img.shields.io/github/license/appergb/openless?style=flat-square&color=2f855a" /></a>
  <img alt="macOS" src="https://img.shields.io/badge/macOS-12%2B-1f425f?style=flat-square" />
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-0078d4?style=flat-square" />
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24c8db?style=flat-square" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-ce422b?style=flat-square" />
  <img alt="Stars" src="https://img.shields.io/github/stars/appergb/openless?style=flat-square&color=805ad5" />
</p>

<p align="center">
  <strong>加入 QQ 群：1078960553</strong>
</p>

---

OpenLess 是一个跨平台（macOS & Windows）语音输入应用，对标 [Typeless](https://www.typeless.com/)、[Wispr Flow](https://wisprflow.ai)、[Lazy](https://heylazy.com)、Superwhisper 等商业语音输入工具的 **完全开源** 替代品。

把光标放在 ChatGPT、Claude、Cursor、Notion、邮件、聊天框任意输入框里，按一次全局快捷键说话——OpenLess 会录音、转写、按你选的模式润色，把结果直接插入光标位置。插入失败时会自动复制到剪贴板，尽量保证「你说过的话不丢」。

不像那些只把语音转成「逐字稿」的输入法，OpenLess 的核心模式是 **AI prompt 模式**：你只管乱讲，它自动补上结构、列出约束、整理出有上下文的 prompt，复制粘贴就能直接喂给 ChatGPT / Claude / Cursor。

## 一个具体的例子

按住快捷键，对着 OpenLess 说：

> 嗯…就是…我想让那个 ChatGPT 帮我写个 SQL，从 orders 表里查上个月的订单，按客户分组，金额倒序，要前十个吧

松开快捷键，一秒后你的输入框里出现的是：

```text
请帮我写一段 SQL，要求如下：

- 从 `orders` 表查询上个月的订单。
- 按客户分组。
- 按金额倒序排序。
- 只返回前 10 条。
```

不需要修改，直接 Enter 就能问 GPT。这就是 OpenLess 想做的事：**让你用嘴写 prompt，比用键盘还快还清楚。**

## 为什么开源 OpenLess

类似工具大多是商业 SaaS：每月订阅、不能自带模型、转写音频会上传到厂商服务器、词典和习惯沉淀在对方账户里。

OpenLess 想做的是同一类体验，但是：

- **完全开源、本地优先**。代码在仓库里，所有数据写在你的机器上。
- **自带云凭据**。火山引擎 ASR + Ark / DeepSeek 兼容 chat-completions，不强绑某家。
- **专门为 AI prompt 优化**。「清晰结构」模式会把零散口语补成有上下文、有约束、有要求的 prompt，复制粘贴就能直接喂给 ChatGPT / Claude / Cursor。
- **不会替你回答**。模型只整理你的话，不会把「我们这个应用还有哪些功能没做？」变成一份功能清单——只会补成一句通顺的问题，让你拿去问真正的 AI。

## 适用场景

- 给 ChatGPT / Claude / Cursor / Gemini 写 prompt：口述一段需求，OpenLess 自动整理成结构化、有细节的 prompt。
- 写邮件、写需求文档、写微信/Slack 长消息：去口癖、补标点、按段落整理。
- 写代码注释、commit message、PR 描述：把脑子里的想法直接落到光标位置。
- 任何「我懒得打字，但又必须输出书面文字」的场景。

## 项目方向

OpenLess 只做一件事：**把语音变成可用的书面文字（尤其是 AI prompt），落到当前光标位置。**

- 不做问答、不做任务执行、不做项目分析。
- 不做对话上下文累积，每次输入都是独立的整理请求。
- 输入语音 → 转写 → 整理 → 插入当前输入框。失败时复制到剪贴板。
- 围绕这条主路径完善体验：模式选择、词典、历史、菜单栏、首页报告。

## 对标参考

| 工具 | 形态 | OpenLess 的差异 |
| --- | --- | --- |
| [Typeless](https://www.typeless.com/) | 闭源 macOS / Windows / iOS，订阅制 | 开源；专门暴露 AI prompt 模式；自带 ASR + LLM 凭据；数据和词典留在本机 |
| [Wispr Flow](https://wisprflow.ai) | 闭源 macOS / Windows，订阅制 | 开源；自带 ASR + LLM 凭据；提示词处理原则透明可改 |
| [Lazy](https://heylazy.com) | 闭源笔记/捕获工具 | 不做笔记容器，专做「插入到任意输入框」 |
| [Superwhisper](https://superwhisper.com) | 闭源 macOS，订阅制 | 开源；目前云端 ASR 优先，本地 ASR 在 roadmap |

## 当前状态（v1.1）

- Tauri 2 + Rust 后端 + React/TS 前端；macOS 12+，Windows 10+。
- 切换式录音：按一次开始，再按一次结束；录音中按 `Esc` 取消。
- 接入火山引擎流式 ASR 和 Ark / DeepSeek 兼容 Chat Completions 进行润色。
- 4 种输出模式：原文、轻度润色、清晰结构（**AI prompt 模式**）、正式表达。
- 主窗口按「概览 / 历史 / 词典 / 风格 / 设置」组织；托盘图标常驻；浮动状态胶囊。
- 词典条目注入 ASR 热词和润色语义提示。
- 平台原生全局快捷键：macOS 使用 CGEventTap，Windows 使用低层键盘钩子（`WH_KEYBOARD_LL`）。

## 下载与安装（普通用户）

到 [Releases](../../releases) 下载对应平台的安装包：
- **macOS**：`OpenLess_<版本>_aarch64.dmg` — 打开后拖入「应用程序」
- **Windows**：`OpenLess_<版本>_x64-setup.exe` — 运行安装程序

首次启动需要授予权限：

**macOS：**
1. 授予麦克风权限。
2. 授予辅助功能权限。
3. **退出 OpenLess 并重新打开**（辅助功能授权需要重启才对全局快捷键生效）。
4. 打开「设置」，填入火山引擎 ASR 和 Ark 凭据。

**Windows：**
1. 按提示授予麦克风权限。
2. 打开「设置 → 权限」确认全局快捷键监听器已启动。
3. 在「设置」中填入火山引擎 ASR 和 Ark 凭据。

完整使用步骤见 [USAGE.md](USAGE.md)。

## 从源码构建（开发者）

当前活跃代码库在 `openless-all/app/`（Tauri 2 + Rust + React/TS）。

```bash
cd "openless-all/app"
npm ci

# 开发模式：Vite at :1420 + Tauri shell
npm run tauri dev

# macOS release 构建（签名、安装、重置 TCC）
./scripts/build-mac.sh
INSTALL=0 ./scripts/build-mac.sh   # 只构建，不安装

# Rust 类型检查（不做完整编译）
cargo check --manifest-path src-tauri/Cargo.toml

# 前端 TS 检查
npm run build
```

日志路径：`~/Library/Logs/OpenLess/openless.log`（macOS）/ `%LOCALAPPDATA%\OpenLess\Logs\openless.log`（Windows）。

**Windows 构建** — MSVC 和 GNU/MinGW 两种路线详见 [`openless-all/README.md`](openless-all/README.md)。

**Swift（仅用于 Sparkle 遗留版本发布）：**

```bash
swift build && swift test
./scripts/build-app.sh
```

## 凭据

凭据保存在本机 Keychain（service = `com.openless.app`）。开发期同时维护一份明文 JSON 兜底，用于在 Keychain 不可用时回退：

```text
~/.openless/credentials.json   # 0600，目录 0700
```

仓库本身不包含任何 API Key、Token 或 Endpoint 之外的私有信息。

需要配置的字段：

- 火山引擎 ASR：APP ID、Access Token、Resource ID。
- Ark 润色：API Key、Model ID、Endpoint。

## 提示词处理原则

OpenLess 的润色模型只做文本整理，不做问答、不做任务执行、不做项目分析。每次语音输入都会作为独立请求发送，提示词会明确告诉模型：

- 本次输入与历史对话隔离。
- 原始转写只是待整理文本。
- 即使原文里有问题或命令，也不要回答或执行。
- 只输出整理后的正文，不添加“我整理如下”等引导语。

例如用户说：“我们这个应用还有哪些功能没有完成”，正确输出应是：

```text
我们这个应用还有哪些功能没有完成？
```

而不是直接替用户列出清单。

竞品文本和长期改写样例会按“原始文本 -> 目标整理结果 -> 改写规律”的方式沉淀，后续接入向量数据库后，只检索相似改写样例作为参考，不把样例当作当前对话上下文。规范见 [docs/polish-reference-corpus.md](docs/polish-reference-corpus.md)，示例见 [Examples/polish-reference-examples.sample.jsonl](Examples/polish-reference-examples.sample.jsonl)。

## 词典

词典用于处理用户自己的专有名词、产品名、人名和新词。当前支持：

- 手动添加正确词、分类和备注；暂不要求用户维护易错词或上下文点。
- 将启用词条作为火山 ASR `context.hotwords` 注入，优先在识别阶段识别正确。
- 将词典包裹后注入后期润色模型，明确告诉模型根据整句语义自动判断：如果 `Cloud` 在当前语境下明显指向 AI 产品 `Claude`，就修正为 `Claude`；如果确实是在说云服务 Cloud，则保留原词。
- 从历史输出中自动学习类似 `Claude`、`ChatGPT`、`OpenLess` 的候选正确词，后续作为 ASR 热词和后期语义判断候选。

主窗口按「首页 / 历史记录 / 词典 / 设置」组织；词典页点击“新建”会弹出独立编辑窗口，首页会展示口述时长、总字数、平均每分钟字数、估算节省时间和词典参与记录。

## 架构概览

当前活跃实现是 Tauri 2（`openless-all/app/`）。`Sources/` 下的 Swift/SwiftPM 代码是遗留版本，仍为老用户提供 Sparkle 自动更新。

**Tauri 后端（Rust）** — 各模块只依赖 `types.rs`：

```
types.rs         纯值类型：DictationSession, PolishMode, HotkeyBinding, 错误类型
hotkey.rs        全局快捷键（macOS: CGEventTap，Windows: WH_KEYBOARD_LL，Linux: rdev）
recorder.rs      麦克风 → 16 kHz 单声道 Int16 PCM，RMS 回调
asr/             火山引擎流式 ASR（WebSocket）+ Whisper HTTP
polish.rs        OpenAI 兼容 chat-completions（Ark / DeepSeek 等）
insertion.rs     AX focused-element → 剪贴板 + Cmd+V → 仅复制兜底
persistence.rs   历史记录 / 偏好设置 / 词典 JSON + Keychain 凭据
permissions.rs   TCC 权限检查（辅助功能 / 麦克风）
coordinator.rs   状态机：Idle → Starting → Listening → Processing
commands.rs      Tauri IPC 接口
```

**React 前端（`src/`）** — 状态通过 Recoil atoms（`pages/_atoms.tsx`）管理；快捷键能力和绑定通过 `HotkeySettingsContext` 获取；所有后端调用走 `lib/ipc.ts`。

录音 → 转写 → 润色 → 插入流水线：`快捷键触发 → Recorder.start + ASR.openSession → [音频帧] → 再次触发 → Recorder.stop + ASR.sendLastFrame → Polish → Insert → History.save`。

详细的不变量和模块接线规则见 [CLAUDE.md](CLAUDE.md)。

## 规划中

尚未发布的功能：

- 本地 ASR（Whisper 接入进行中；当前仅支持火山引擎云端）。
- 常用片段 Snippets：尚无 UI 和触发逻辑。
- 历史增强：复制按钮、搜索、重新润色、重新插入。
- 粘贴上一条结果快捷键。
- 多屏定位：胶囊跟随焦点所在屏幕显示。

## 维护者：发布检查

- 同步更新 `openless-all/app/package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 中的版本号。
- 运行 `INSTALL=0 ./scripts/build-mac.sh`，确认 `.app` 可启动。
- 在干净 macOS 机器上验证：权限引导、快捷键、录音、ASR、润色、插入、剪贴板兜底。
- 推送 `v<version>-tauri` tag → CI 构建并签名 macOS `.dmg` + Windows `.msi`。
- **Swift 遗留发布**（面向 Sparkle 老用户）：`./scripts/release.sh <version>` — 详见 [CLAUDE.md](CLAUDE.md)。

## 许可

MIT
