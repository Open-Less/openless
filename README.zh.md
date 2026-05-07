<p align="center">
  <img src="openless-all/app/src-tauri/icons/128x128@2x.png" alt="OpenLess" width="160" />
</p>

<h1 align="center">OpenLess</h1>

<p align="center">
  <strong>开源语音输入，支持 macOS 和 Windows。</strong><br/>
  按一次快捷键说话，AI 润色后的文字直接落到当前光标。
</p>

<p align="center">
  <a href="https://openless.top"><strong>🌐 官方网站 — openless.top</strong></a>
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

<p align="center">
  <strong>赞助商</strong>
</p>

<p align="center">
  <a href="https://www.knin.net" target="_blank" rel="noopener">
    <img alt="悠雾云数据" src="https://www.knin.net/upload/logo.png" height="48" />
  </a>
  &nbsp;&nbsp;
  <a href="https://jiangmuran.com/" target="_blank" rel="noopener">
    <img alt="jiangmuran" src="assets/people/jiangmuran.png" width="48" height="48" />
  </a>
  <br/>
  <a href="https://www.knin.net" target="_blank" rel="noopener">悠雾云数据 — www.knin.net</a>
  &nbsp;·&nbsp;
  <a href="https://jiangmuran.com/" target="_blank" rel="noopener">jiangmuran — jiangmuran.com</a>
</p>

<p align="center">
  <strong>开发者</strong>
</p>

<p align="center">
  <a href="https://tripmc.top/" target="_blank" rel="noopener">
    <img alt="TRIP" src="assets/people/tripmc.png" width="80" height="80" />
  </a>
  &nbsp;&nbsp;
  <a href="https://chris233.qzz.io" target="_blank" rel="noopener">
    <img alt="Chris233" src="assets/people/Chris233.png" width="80" height="80" />
  </a>
  &nbsp;&nbsp;
  <a href="https://github.com/Cooper-X-Oak" target="_blank" rel="noopener">
    <img alt="Cooper" src="https://github.com/Cooper-X-Oak.png?size=160" width="80" height="80" />
  </a>
  <br/>
  <a href="https://tripmc.top/" target="_blank" rel="noopener">TRIP — tripmc.top</a>
  &nbsp;·&nbsp;
  <a href="https://chris233.qzz.io" target="_blank" rel="noopener">Chris233 — chris233.qzz.io</a>
  &nbsp;·&nbsp;
  <a href="https://github.com/Cooper-X-Oak" target="_blank" rel="noopener">Cooper — github.com/Cooper-X-Oak</a>
</p>

---

OpenLess 是一个跨平台（macOS & Windows）语音输入应用，对标 [Typeless](https://www.typeless.com/)、[Wispr Flow](https://wisprflow.ai)、[Lazy](https://heylazy.com)、Superwhisper 等商业语音输入工具的 **完全开源** 替代品。官方网站：[openless.top](https://openless.top)。

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

## 当前状态（v1.2）

- Tauri 2 + Rust 后端 + React/TS 前端；macOS 12+，Windows 10+。
- **切换式 + 按住说话** 双模式录音；任意阶段按 `Esc` 都能取消（包括润色 / 插入中）。
- 接入火山引擎流式 ASR + OpenAI Whisper 兼容批式 ASR；Ark / DeepSeek / OpenAI 兼容 Chat Completions 进行润色。
- 4 种输出模式：原文、轻度润色、清晰结构（**AI prompt 模式**）、正式表达。
- 主窗口按「概览 / 历史 / 词典 / 风格 / 设置」组织；托盘图标常驻；浮动状态胶囊。
- **中英双语 UI** — 设置 → 语言 切换简体中文 / English（首启按系统语言自动）。
- **应用内自动更新** — 设置 → 关于 → 检查按钮；CI 用 Tauri updater 签名 manifest，客户端校验后下载安装。
- **单实例锁** — 防止两份 OpenLess 进程并存争抢同一热键边沿。
- 词典条目作为 Volcengine ASR `context.hotwords` 注入 + 润色语义提示，每次会话累计命中数。
- 平台原生全局快捷键：macOS 使用 CGEventTap，Windows 使用低层键盘钩子（`WH_KEYBOARD_LL`）。

## 下载与安装（普通用户）

到 [Releases](../../releases) 下载对应平台的安装包：
- **Windows**：`OpenLess_<版本>_x64-setup.exe` — 运行安装程序
- **macOS**：`OpenLess_<版本>_aarch64.dmg` — 打开后拖入「应用程序」
- **macOS(brew install)**:
```bash
brew tap appergb/openless https://github.com/appergb/openless
brew install --cask openless
xattr -cr /Applications/OpenLess.app

# Upgrade to the latest version
brew update && brew upgrade openless
```

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

不会配火山 ASR 的话，直接看这篇图文引导：
[OpenLess 火山 ASR 配置](docs/volcengine-setup.md)

完整使用步骤见 [USAGE.md](USAGE.md)。

## 从源码构建（开发者）

当前活跃代码库在 `openless-all/app/`（Tauri 2 + Rust + React/TS）。macOS 构建会链接一份 vendored 的本地 ASR 引擎（[`antirez/qwen-asr`](https://github.com/antirez/qwen-asr)），以 git submodule 形式挂在 `src-tauri/vendor/qwen-asr/`，首次 clone 后必须先拉子模块。

```bash
# 首次 clone 后拉取子模块
git submodule update --init --recursive

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

## 贡献流程

OpenLess 采用 **Beta / 正式版** 双渠道分支模型。

- **`beta`** —— **Beta 渠道（开发版）**。默认分支，也是日常集成缓冲区；所有进行中的开发都先落到这里。Beta 渠道可以直接出包，但**不会推送给普通用户**——只有主动切换到 Beta 渠道的用户才会拿到 Beta 包。
- **`main`** —— **正式版渠道（Stable）**。始终保持可发布状态，普通用户默认拿到的就是这条线上的版本。

```text
你的 fork / topic 分支
        │  （先在目标平台本地自测通过）
        ▼
   PR → beta   ← AI Review（一次性，仅供参考）
        │     ← 维护者轻量过一眼（范围、跨模块影响）
        ▼
       合入 beta
        │  （定期或里程碑节点，跑双端冒烟测试）
        ▼
       合入 main  →  打 tag `v<版本>-tauri`  →  Release CI → 推给正式版用户
```

核心规则：

- **PR 一律打到 `beta`，不要直接打到 `main`。** GitHub 上新建 PR 的 base 已默认是 `beta`。
- **开 PR 前先在目标平台跑通功能** —— build 绿是底线，必须做人工验证。
- **AI Review 每个 PR 只跑一轮，结果仅供参考。** 不要围绕它反复改，最终判断权在贡献者和维护者手里。
- **AI 改 Review 意见控制在 1–2 轮。** 卡住了直接换人工或重开对话上下文，避免多轮 AI 越改越乱。
- **Beta 不能溢出到正式版。** `main` 只接收来自 `beta` 的合并，由维护者在双端冒烟测试通过后执行；任何人不要直接 push `main`。
- **正式版 Release 从 `main` 切出**，通过推送 `v<版本>-tauri` tag 触发，详见下方"维护者：发布检查"。

Beta 包的分发（opt-in，尚未接入）：Beta 包面向主动加入 Beta 渠道的用户；当前 App 内 updater 把所有 release 都当作正式版处理，后续会在设置页加入"加入 Beta 渠道"开关，并把 updater endpoint 按渠道拆开。

## 凭据

凭据保存在系统凭据库（service = `com.openless.app`）：macOS Keychain、Windows Credential Manager 或 Linux keyring。旧版明文 JSON 只作为迁移来源读取，成功写入系统凭据库后会被删除：

```text
macOS / Linux: ~/.openless/credentials.json
Windows:       %APPDATA%\OpenLess\credentials.json
```

新的凭据写入不会继续保存明文 secrets。仓库本身不包含任何 API Key、Token 或 Endpoint 之外的私有信息。

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

当前活跃实现是 Tauri 2（`openless-all/app/`）。自动更新走 Tauri updater 插件；CI 在每次 `v*-tauri` tag 自动签名 updater artifact + manifest。

**Tauri 后端（Rust）** — 各模块只依赖 `types.rs`：

```
types.rs         纯值类型：DictationSession, PolishMode, HotkeyBinding, 错误类型
hotkey.rs        全局快捷键（macOS: CGEventTap，Windows: WH_KEYBOARD_LL，Linux: rdev）
recorder.rs      麦克风 → 16 kHz 单声道 Int16 PCM，RMS 回调
asr/             火山引擎流式 ASR（WebSocket）+ Whisper HTTP
polish.rs        OpenAI 兼容 chat-completions（Ark / DeepSeek 等）
insertion.rs     AX focused-element → 剪贴板 + Cmd+V → 仅复制兜底
persistence.rs   历史记录 / 偏好设置 / 词典 JSON + 系统凭据库
permissions.rs   TCC 权限检查（辅助功能 / 麦克风）
coordinator.rs   状态机：Idle → Starting → Listening → Processing
commands.rs      Tauri IPC 接口
```

**React 前端（`src/`）** — 状态通过 Recoil atoms（`pages/_atoms.tsx`）管理；快捷键能力和绑定通过 `HotkeySettingsContext` 获取；所有后端调用走 `lib/ipc.ts`。

录音 → 转写 → 润色 → 插入流水线：`快捷键触发 → Recorder.start + ASR.openSession → [音频帧] → 再次触发 → Recorder.stop + ASR.sendLastFrame → Polish → Insert → History.save`。

详细的不变量和模块接线规则见 [CLAUDE.md](CLAUDE.md)。

## 规划中

尚未发布的功能：

- 口述翻译模式：长按独立热键说一种语言、自动插入目标语言（[#43](../../issues/43)）。
- 跨会话风格记忆：polish 逐渐学习用户的语气习惯（[#46](../../issues/46)）。
- 常用片段 Snippets：尚无 UI 和触发逻辑。
- 历史增强：复制按钮、搜索、重新润色、重新插入。
- 粘贴上一条结果快捷键。
- 多屏定位：胶囊跟随焦点所在屏幕显示。

## 维护者：发布检查

- 同步更新 `openless-all/app/package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 中的版本号。
- 运行 `INSTALL=0 ./scripts/build-mac.sh`，确认 `.app` 可启动。
- 在干净 macOS 机器上验证：权限引导、快捷键、录音、ASR、润色、插入、剪贴板兜底。
- 推送 `v<version>-tauri` tag → CI 构建并签名 updater artifact + macOS `.dmg` + Windows `.msi`。需要 repo secret `TAURI_SIGNING_PRIVATE_KEY`（对应 `tauri.conf.json` 中的 pubkey）才能签名 updater 包。

## 致谢

OpenLess 在此真诚感谢三类群体：赞助者、开发者/贡献者，以及 LinuxDo 社区。

感谢赞助者对项目持续推进的支持；感谢开发者和贡献者在开发、评审与改进中的长期投入。

OpenLess 也认可并感谢 LinuxDo 社区开放、务实、对开发者友好的氛围。OpenLess 的许多想法、讨论和早期反馈，都受到了 LinuxDo 所代表的开源交流精神的启发。

此致谢不代表官方背书或隶属关系。

## 许可

MIT
