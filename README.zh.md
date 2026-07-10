<p align="center">
  <img src="openless-all/app/src-tauri/icons/128x128@2x.png" alt="OpenLess" width="128" height="128" />
</p>

<h1 align="center">OpenLess</h1>

<p align="center">
  <strong>面向 macOS 与 Windows 的开源语音输入工具</strong>
</p>

<p align="center">
  按住快捷键说话,经 AI 润色的文本会实时插入到光标处——<br/>
  以<em>你</em>选择的写作风格呈现。
</p>

<p align="center">
  <a href="https://openless.top"><strong>官网</strong></a>
  &nbsp;·&nbsp;
  <a href="https://github.com/appergb/openless/releases/latest"><strong>下载</strong></a>
  &nbsp;·&nbsp;
  <a href="README.md">English</a>
  &nbsp;/&nbsp;
  <a href="README.zh.md">中文</a>
</p>

<p align="center">
  <a href="https://github.com/appergb/openless/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/appergb/openless?style=flat-square&color=2c5282" /></a>
  <a href="https://github.com/appergb/openless/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/github/license/appergb/openless?style=flat-square&color=2f855a" /></a>
  <a href="https://github.com/appergb/openless/stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/appergb/openless?style=flat-square&color=805ad5" /></a>
  <a href="https://discord.gg/vTZHTFGFm"><img alt="Discord" src="https://img.shields.io/badge/Discord-Join-5865F2?style=flat-square&logo=discord&logoColor=white" /></a>
</p>

<p align="center">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-12%2B-1f425f?style=flat-square&logo=apple&logoColor=white" />
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-0078d4?style=flat-square&logo=windows&logoColor=white" />
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24c8db?style=flat-square&logo=tauri&logoColor=white" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-ce422b?style=flat-square&logo=rust&logoColor=white" />
</p>

<p align="center">
  💬 &nbsp;<a href="https://discord.gg/vTZHTFGFm"><strong>加入 Discord 社区</strong></a> &nbsp;·&nbsp; QQ 群&nbsp; <strong>1078960553</strong>
</p>

<br/>

<h2 align="center">赞助商</h2>

<p align="center">
  <em>OpenLess 的持续开发,离不开赞助商的慷慨支持。</em>
</p>

<p align="center">
  <a href="https://jiangmuran.com/" target="_blank" rel="noopener">
    <img src="assets/people/jiangmuran.png" width="120" height="120" alt="jiangmuran" />
  </a>
</p>

<p align="center">
  <a href="https://jiangmuran.com/" target="_blank" rel="noopener"><strong>jiangmuran</strong></a><br/>
  <sub>⭐ 特别赞助 · jiangmuran.com</sub>
</p>

<p align="center">
  特别感谢 <strong>jiangmuran</strong> 长期以来的支持,让 OpenLess 得以不断向前。
</p>

<p align="center">
  <sub>— 同时感谢 —</sub>
</p>

<p align="center">
  <a href="https://www.knin.net" target="_blank" rel="noopener">
    <img alt="悠雾云数据 (Youwu Cloud Data)" src="https://www.knin.net/upload/logo.png" height="52" />
  </a>
  <br/>
  <a href="https://www.knin.net" target="_blank" rel="noopener">悠雾云数据 · Youwu Cloud Data (knin.net)</a>
</p>

<h2 align="center">开发者</h2>

<table align="center">
  <tr>
    <td align="center" width="170">
      <a href="https://tripmc.top/" target="_blank" rel="noopener">
        <img src="assets/people/tripmc.png" width="80" height="80" alt="TRIP" /><br/>
        <strong>TRIP</strong>
      </a><br/>
      <sub>tripmc.top</sub>
    </td>
    <td align="center" width="170">
      <a href="https://chris233.qzz.io" target="_blank" rel="noopener">
        <img src="assets/people/Chris233.png" width="80" height="80" alt="Chris233" /><br/>
        <strong>Chris233</strong>
      </a><br/>
      <sub>chris233.qzz.io</sub>
    </td>
    <td align="center" width="170">
      <a href="https://github.com/Cooper-X-Oak" target="_blank" rel="noopener">
        <img src="assets/people/cooper.png" width="80" height="80" alt="Cooper" /><br/>
        <strong>Cooper</strong>
      </a><br/>
      <sub>github.com/Cooper-X-Oak</sub>
    </td>
  </tr>
</table>

---

OpenLess 是一款跨平台(macOS 与 Windows)语音输入应用,是 [Typeless](https://www.typeless.com/)、[Wispr Flow](https://wisprflow.ai)、[Lazy](https://heylazy.com)、Superwhisper 等商业工具的完全开源替代品。官网:[openless.top](https://openless.top)。

把光标放在任意文本框中——ChatGPT、Claude、Cursor、Notion、邮件草稿、聊天框——按下一个全局快捷键,然后开口说话。OpenLess 会录音、转写,按你选定的模式润色文本,并将结果插入到光标处。如果插入被阻止,文本会改为复制到剪贴板,你说过的话不会丢失。

与只产出逐字转写稿的听写工具不同,OpenLess 的核心能力是 **AI Prompt 模式**:你自由地说,它来补充结构、整理你的约束条件,生成一段包含上下文的提示词,可直接粘贴进 ChatGPT、Claude 或 Cursor。

## 从工具到基础设施

真正高级的系统都在做同一件事:**把今天的沟通,变成明天的默认规则。** 互联网赢,不是因为 HTTP 更快,而是因为没人再讨论数据怎么传;Git 赢,不是因为代码更优,而是因为团队不用每天协调版本;Stripe 赢,不是因为支付更快,而是因为开发者不用再协调银行。

OpenLess 做的不是“更快的听写”,而是**消灭“想法 → 干净文字”之间的每一次协调**。过去,把脑子里的话落成能用的文字,你要一遍遍临时决定:这段用什么语气?这个名字怎么拼?落到哪个输入框、复制还是粘贴?模型的钥匙放哪、谁有权限?OpenLess 把这些反复的临时协商,逐层**沉降**成默认规则:

- **语气** 沉降成**风格包**——命名、可切换,热键一按即生效,不必每次重新决定。
- **专有名词** 沉降成**词典热词**——ASR 端识别、润色端语义提示,不必逐次纠正。
- **文字落点** 沉降成**唯一插入协议**——按住 → 说 → 落在光标处,失败自动回退剪贴板。
- **模型与凭据** 沉降进**操作系统保管库**——开屏授权一次,此后永不重谈。

这就是**开屏即授权完基础设施**:首次启动,你只授权一次麦克风、辅助功能与云端凭据;从那一刻起,“开口说话 → 光标处出现干净文字”不再是一次次的操作协商,而是一条你可以忘记它存在的默认协议。衡量 OpenLess 的价值,不在于它快了多少,而在于它替你**消灭了多少协调**。

## ✨ 更新亮点

下面两项能力,把过去每天都要重复的协调,进一步沉降成了默认规则:

- 🎨 **风格包市场(Style Pack Marketplace)。** OpenLess 不再只内置一种固定的“润色”语气。你可以用自定义系统提示词构建自己的**风格包**,用快捷键在它们之间切换,并**一键安装社区分享的风格包**——也可以发布自己的与他人分享。当风格与你的具体任务高度契合(冷启动邮件、commit message、小红书文案、正式报告、团队语气)时,产出的文本不只是更干净,而是*明显更好*,因为模型终于在按你真正想要的方式写作。
- ⚡ **流式插入。** 文本现在会随润色**逐字符**写入光标,而不必等待完整结果生成。感知延迟大幅下降,听写几乎和思考一样快——当某个应用无法接受流式按键时,它会自动回退为一次性粘贴。

## 一个实际例子

按住快捷键,说:

> 嗯…那个,我要给客户回个邮件,就是上次他们提的那个方案嘛,我们内部讨论了一下觉得大方向没问题,但是有几个细节要改,第一个就是交付时间太紧了我们希望延两周,第二个是预算那块他们报的价格比我们预期高了大概百分之二十能不能谈一下,然后整体合作意向是积极的让他们放心,语气客气一点别太生硬

松开快捷键。片刻之后,你的输入框里出现:

```text
给客户回邮件,关于上次他们提的方案,我们内部讨论后认为大方向没问题,但有几个细节需要调整:

1. 交付时间
   (a) 目前交付时间太紧,希望延长两周。
2. 预算
   (a) 他们的报价比我们预期高了大约 20%,能否再谈一下？
整体合作意向是积极的,语气上请客气一点,不要太生硬。
```

无需任何修改——直接粘贴给 ChatGPT 或 Claude,让它替你把邮件写完。这就是核心理念:**用嘴写提示词,比打字更快、更干净。**

## 为什么 OpenLess 选择开源

最接近的替代品都是订阅制 SaaS:按月付费、无法自带模型、音频上传到厂商、你的词典和习惯都存在它们的账户里。

OpenLess 追求同样的终端体验,但:

- **完全开源、本地优先。** 代码就在本仓库,你的所有数据都留在自己的机器上。
- **自带云端凭据。** Volcengine 流式 ASR + Ark / DeepSeek 兼容的 chat completions,不绑定任何厂商。
- **为 AI 提示词而调优。** 结构化模式会把零散的口语重塑为带有上下文、约束与诉求的提示词,可直接粘贴进 ChatGPT、Claude 或 Cursor。
- **它不会替你作答。** 模型只负责清理你的文字。如果你说“这个应用还需要哪些功能?”,它会原样返回为一个干净的问句——而不会给你列一份功能清单。那种事,去问 AI 本体。

## 适用场景

- **为 ChatGPT / Claude / Cursor / Gemini 写提示词**——口述需求,OpenLess 将其转为结构化、细节充分的提示词。
- **起草邮件、规格说明、较长的 Slack 或微信消息**——去除口头语、修正标点、整理段落。
- **代码注释、commit message、PR 描述**——把脑子里的想法直接落到光标处。
- **任何**你必须产出文字、却又不想打字的场景。

## 风格包与市场

**风格包**是一种带有专属系统提示词的、命名的输出风格。你不再被限定于唯一的内置“润色”语气,而是可以精确塑造口语的呈现方式——风格越贴近你的真实任务,效果就越好。

- **创建与自定义。** 在**风格(Style)**页面新增一个风格包,写入自定义系统提示词(例如“简洁的工程 commit message”“温暖的客服回复”“带 emoji 的小红书文案”)。用快捷键切换当前生效的风格包。
- **从社区安装。** 打开**市场(Marketplace)**,浏览、搜索并一键安装他人分享的风格包,并为好用的点赞。
- **发布自己的。** 用 GitHub 身份登录(设置 → Marketplace),然后在风格页选择**发布到 Marketplace**。上传内容会经过审核后才公开展示。

市场由 OpenLess 自有的、经过审核的后端提供服务,因此目录是经过策展的,而非放任的大杂烩。

## 项目定位

OpenLess 只做一件事:**把语音变成可用的书面文字(尤其是 AI 提示词),并落在当前光标处。**

- 它不回答问题、不执行任务、不分析你的项目。
- 它不积累对话上下文;每次听写都是一次独立的清理请求。
- 流程是:语音 → 转写 → 清理 → 插入到光标,失败时回退到剪贴板。
- 其余的一切(模式、词典、历史、菜单栏、首页报告)都为这唯一的路径服务。

## 对比

| 工具 | 形态 | OpenLess 的不同之处 |
| --- | --- | --- |
| [Typeless](https://www.typeless.com/) | 闭源 macOS / Windows / iOS,订阅制 | 开源;显式的 AI 提示词模式;自带 ASR + LLM;数据与词典留在本机 |
| [Wispr Flow](https://wisprflow.ai) | 闭源 macOS / Windows,订阅制 | 开源;自带 ASR + LLM;文本处理规则透明 |
| [Lazy](https://heylazy.com) | 闭源的笔记 / 速记工具 | 不是笔记容器——直接插入到任意输入框 |
| [Superwhisper](https://superwhisper.com) | 闭源 macOS,订阅制 | 开源;目前云端 ASR,本地 ASR 在路线图中 |

## 当前状态(v1.3.6)

下面每一项,都是一层已经沉降为默认、你授权一次之后就不必再操心的能力——这就是开屏之后你所站立的基础设施:

- Tauri 2 后端(Rust)+ React/TypeScript 前端。macOS 12+、Windows 10+。
- 🎨 **风格包市场**——在应用内的 Marketplace 浏览、安装、点赞社区**风格包**,并发布自己的(每个包一套自定义系统提示词,可用快捷键切换)。由经过审核的市场后端支撑;上传内容公开前会经过审核。
- ⚡ **流式插入**——润色后的文本逐字符写入光标以降低感知延迟,并带有自动的一次性粘贴回退。可在 设置 → 录音 中切换。
- **切换式与按住说话(push-to-talk)** 两种录音模式,外加 **MediaPlayPause 触发**,让有线耳机的线控也能开始 / 停止录音。`Esc` 可在任意阶段取消,包括润色与插入。
- **云端 ASR**:Volcengine 流式 ASR、OpenAI Whisper 兼容的批量 ASR、Apple Speech(macOS)。
- **本地 ASR**:通过 vendored 的 `Open-Less/qwen-asr` 内置 Qwen3-ASR(0.6B / 1.7B);Windows 上的 Foundry Local Whisper 变体。
- **润色提供方**:Ark / DeepSeek / OpenAI / Doubao / Anthropic 兼容的 chat completions,以及你自带的任意 OpenAI 兼容端点。
- **四种输出模式**:原文、轻度润色、结构化(**AI 提示词模式**)、正式。另有一个**翻译快捷键**,将语音直接转换为所配置的目标语言([#43](../../issues/43))。
- **选区问答面板**——一个独立快捷键打开浮动面板,针对任意应用中被高亮选中的文本进行语音问答([#118](../../issues/118))。
- **主窗口**:概览 / 历史 / 词典 / 风格 / 市场 / 设置。常驻托盘图标,以及一个浮于屏幕、并跟随你正在输入的显示器的迷你状态胶囊(多显示器)。
- **本地模型管理**——在设置中管理本地 ASR 模型在磁盘上的存储。
- **多语言界面**——设置 → 语言 可在 简体中文 / 繁體中文 / English / 日本語 / 한국어 之间切换(首次启动自动检测)。
- **应用内自动更新**——设置 → 关于 → 检查;通过 Tauri updater 插件提供签名的更新产物。
- **Beta 频道(可选加入)**——设置 → 关于 → 加入 Beta 频道,可下载最新预发布版本进行手动安装。Beta 版本绝不会自动推送给 Stable 用户(见[贡献流程](#贡献流程))。
- **分发渠道**——从 [Releases](../../releases) 直接下载 DMG/EXE、Homebrew Cask(`brew install --cask openless`)、Windows 安装包。
- **单实例锁**——防止两个 OpenLess 进程争抢同一个快捷键边沿。
- 词典条目作为 Volcengine ASR 的 `context.hotwords` 注入,并在润色时作为语义提示;命中次数按会话累计。
- 平台原生全局快捷键:macOS 上为 CGEventTap,Windows 上为低级键盘钩子(`WH_KEYBOARD_LL`)。

## 下载与安装(终端用户)

前往 [Releases](../../releases) 下载:

- **macOS**:`OpenLess_<version>_aarch64.dmg`(Apple Silicon)或 `OpenLess_<version>_x64.dmg`(Intel)。打开后将应用拖入 `/Applications`,**然后在终端执行一次以下命令,以绕过 Gatekeeper 的“已损坏”提示**(该构建为 ad-hoc 签名,未经 Apple 公证):
  ```bash
  xattr -cr /Applications/OpenLess.app
  ```
- **Windows**:`OpenLess_<version>_x64-setup.exe`——运行安装程序。
- **Android**（GitHub Release 提供 release APK；Actions artifact 提供 debug 冒烟包）:
  - **真机推荐**:`OpenLess_<version>_arm64-v8a.apk`
  - **旧款 32 位 ARM**:`OpenLess_<version>_armeabi-v7a.apk`
  - **64 位模拟器**:`OpenLess_<version>_x86_64.apk`
  - **32 位模拟器**:`OpenLess_<version>_x86.apk`
  - 应用内更新（设置 → 关于）读取 `latest-android-{arch}.json`；Beta 用户在高级设置加入 Beta 渠道。
  - 调试包:`OpenLess-android-debug-{abi}-*.apk`（workflow_dispatch 产物）。
  - 不确定时执行 `adb shell getprop ro.product.cpu.abi`，下载对应 ABI 的包。
- **macOS(Homebrew)**:
  ```bash
  brew tap appergb/openless https://github.com/appergb/openless
  brew install --cask openless
  xattr -cr /Applications/OpenLess.app

  # 升级到最新版本
  brew update && brew upgrade openless
  ```

首次启动时,请授予应用所请求的权限。

**macOS:**
1. 授予麦克风访问权限。
2. 授予辅助功能(Accessibility)访问权限。
3. **退出并重新打开应用**——辅助功能权限需重启后才生效。
4. 打开设置,填入你的 Volcengine ASR + Ark 凭据。

**Windows:**
1. 在提示时授予麦克风访问权限。
2. 打开 设置 → 权限,确认全局快捷键监听已激活。
3. 在设置中填入你的 Volcengine ASR + Ark 凭据。

完整的终端用户指南见 [USAGE.md](USAGE.md)。

## 从源码构建(开发者)

活跃的代码库位于 `openless-all/app/`(Tauri 2 + Rust + React/TS)。macOS 构建会链接一个 vendored 的 C 语言 ASR 引擎([`Open-Less/qwen-asr`](https://github.com/Open-Less/qwen-asr),fork 自 `antirez/qwen-asr`),它作为 git 子模块位于 `src-tauri/vendor/qwen-asr/`,因此首次克隆时需初始化子模块。

```bash
# 仅首次克隆——拉取 vendored 子模块
git submodule update --init --recursive

cd "openless-all/app"
npm ci

# 开发:Vite 运行于 :1420 + Tauri 外壳
npm run tauri dev

# macOS 发布构建(签名、安装、重置 TCC)
./scripts/build-mac.sh
INSTALL=0 ./scripts/build-mac.sh   # 仅构建,跳过安装

# 不完整编译的 Rust 类型检查
cargo check --manifest-path src-tauri/Cargo.toml

# 前端 TS 检查
npm run build
```

日志:`~/Library/Logs/OpenLess/openless.log`(macOS)/ `%LOCALAPPDATA%\OpenLess\Logs\openless.log`(Windows)。

**Windows 构建**——MSVC 与 GNU/MinGW 两条路线见 [`openless-all/README.md`](openless-all/README.md)。

## 贡献流程

OpenLess 采用双频道分支模型。

- **`beta`**——**Beta 频道**。默认分支与集成缓冲区;所有进行中的开发都汇入这里。Beta 构建可能存在,但**不会推送给普通用户**——只有显式加入 Beta 频道的人才能拿到。
- **`main`**——**Stable 频道(正式版)**。始终可发布;所有人默认获得的构建。

```text
your fork / topic branch
        │  (test locally on your target platform first)
        ▼
   PR → beta  ← AI review (one pass, advisory only)
        │     ← maintainer lightweight glance (scope, cross-module impact)
        ▼
       merged into beta
        │  (periodically, after a two-platform smoke build)
        ▼
       merged into main  →  tag `v<version>-tauri`  →  release CI → Stable users
```

经验法则:

- **向 `beta` 提 PR,绝不要向 `main` 提。** GitHub 对新 PR 已默认将基准分支设为 `beta`。
- **在提 PR 之前,先在你的目标平台上验证改动**——构建通过是必要条件,人工验证是必需的。
- **AI review 每个 PR 只跑一次,且仅供参考。** 不要围着它打转;请运用自己的判断。
- **AI 返工轮次保持精简(1–2 轮)。** 如果某处修复迟迟搞不定,去找真人,或带着全新上下文重来——在这里多轮的 AI 来回往往弊大于利。
- **Beta 的工作不得泄漏到 Stable。** `main` 只接受来自 `beta` 的合并,由维护者在一次成功的双平台冒烟构建之后执行。不允许直接向 `main` 推送。
- **Stable 发布从 `main` 切出**,通过推送 `v<version>-tauri` 标签完成——见下方的维护者发布清单。

Beta 发布的分发(手动下载、可选加入):应用内的更新器始终读取 Stable 清单,因此普通用户绝不会通过自动更新拿到 Beta 构建。想尝试 Beta 的用户打开 **设置 → 关于**,启用“加入 Beta 频道”,再从应用从 GitHub 获取的链接处手动下载最新的 Beta 安装包。标签约定:`v<version>-beta-tauri` 产出 Beta 发布(标记为 GitHub pre-release;清单写为 `latest-{tgt}-{arch}-beta.json`),而 `v<version>-tauri` 产出 Stable 发布。两份清单文件互不重叠,因此 Stable 用户的更新器源不会拾取到 Beta 发布。

## 凭据

凭据存放在操作系统的凭据保管库中(service = `com.openless.app`):macOS Keychain、Windows 凭据管理器,或 Linux keyring。旧的明文 JSON 文件仅作为迁移来源被读取,并在成功写入保管库后删除:

```text
macOS / Linux: ~/.openless/credentials.json
Windows:       %APPDATA%\OpenLess\credentials.json
```

新的凭据写入不会持久化明文密钥。本仓库不包含任何 API key、token 或私有端点。

你需要准备:

- **Volcengine 流式 ASR**:APP ID、Access Token、Resource ID。
- **Ark 润色**:API Key、Model ID、Endpoint。Ark 默认端点为 `https://ark.cn-beijing.volces.com/api/v3/chat/completions`。

## 文本处理原则

OpenLess 的润色模型只重塑文本。它不回答问题、不执行任务、不分析你的项目。每次听写都是一次独立请求,提示词中明确告知模型:

- 该输入与任何先前对话相隔离。
- 原始转写稿是待清理的文本,而非待回答的问题。
- 即使输入中包含问题或命令,也不要回复或执行。
- 只输出清理后的文本——不要“以下是清理后的版本”之类的开场白。

例如,当用户说“这个应用还需要哪些功能”时,正确的输出是:

```text
这个应用还需要哪些功能?
```

……而不是一份缺失功能的清单。

长期参考改写以 `raw → polished → rule` 三元组存储,待向量库接入后,将作为相似示例参考被检索(绝不作为对话上下文)。见 [docs/polish-reference-corpus.md](docs/polish-reference-corpus.md) 与 [Examples/polish-reference-examples.sample.jsonl](Examples/polish-reference-examples.sample.jsonl)。

## 词典

词典负责处理你的专有名词、产品名、人名以及新词。目前支持:

- 手动添加正确拼写、分类与备注。你无需维护错误拼写或上下文提示。
- 启用的条目作为 Volcengine ASR 的 `context.hotwords` 发送,以便在转写时被正确识别。
- 条目同样注入润色提示词:模型逐句判断是否替换。如果“Cloud”在上下文中明显指 AI 产品 `Claude`,就会被纠正;如果它确实指云计算,则保持原样。
- 应用会从你的历史中自动学习候选纠正(如 `Claude`、`ChatGPT`、`OpenLess`),并在之后向你推荐。

主窗口组织为 首页 / 历史 / 词典 / 设置。点击“新建”时,词典页会打开一个独立的编辑窗口。首页展示总听写时长、总字数、平均每分钟字数、估算节省的时间,以及词典参与统计。

## 架构

活跃实现为 Tauri 2(`openless-all/app/`)。发布分为两个频道:**Stable**(`v<v>-tauri` 标签,为所有用户自动更新)与 **Beta**(`v<v>-beta-tauri` 标签,GitHub pre-release,由可选加入的用户手动下载)。每个发布标签都由 CI 产出签名的更新产物。

**Tauri 后端(Rust)**——每个模块仅依赖 `types.rs`:

```
types.rs         Pure value types: DictationSession, PolishMode, HotkeyBinding, errors
hotkey.rs        Global hotkey (CGEventTap on macOS, WH_KEYBOARD_LL on Windows, rdev on Linux)
recorder.rs      Mic → 16 kHz mono Int16 PCM, RMS callback
asr/             Volcengine streaming ASR (WebSocket) + Whisper HTTP
polish.rs        OpenAI-compatible chat completions (Ark / DeepSeek / etc.)
insertion.rs     AX focused-element → clipboard + Cmd+V → copy-only fallback
persistence.rs   History / preferences / vocab JSON + platform credential vault
permissions.rs   TCC checks (Accessibility / Microphone)
coordinator.rs   State machine: Idle → Starting → Listening → Processing
commands.rs      Tauri IPC surface
```

**React 前端(`src/`)**——状态通过 Recoil atoms(`pages/_atoms.tsx`)管理;快捷键能力与绑定通过 `HotkeySettingsContext`;所有后端调用都经由 `lib/ipc.ts`。

听写流水线:`hotkey edge → Recorder.start + ASR.openSession → [audio frames] → hotkey edge → Recorder.stop + ASR.sendLastFrame → Polish → Insert → History.save`。

不变式与模块接线规则见 [CLAUDE.md](CLAUDE.md)。

## 路线图

已规划但尚未发布:

- 听写翻译模式:按住一个独立快捷键,用你的语言说,插入为目标语言([#43](../../issues/43))。
- 跨会话风格记忆:润色随时间学习用户的语气([#46](../../issues/46))。
- 片段(Snippets,尚无 UI 或触发逻辑)。
- 历史增强:复制按钮、搜索、重新润色、重新插入。
- “粘贴上次结果”快捷键。

## 维护者发布清单

OpenLess 提供两个发布频道。分支名即频道名(见[贡献流程](#贡献流程))。

### 通用准备(两个频道)

- 在**全部五个**文件中提升版本号:`package.json`、`package-lock.json`(根级 + `packages.""` 下的嵌套条目)、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`,以及 `Cargo.lock`(查找 `name = "openless"` 块)。否则 CI 的 `Verify version sync` 步骤会使构建失败。
- 运行 `INSTALL=0 ./scripts/build-mac.sh`,确认 `.app` 能启动。
- 在干净的机器上做冒烟测试:权限流程、快捷键、录音、ASR、润色、插入,以及剪贴板回退。
- 确认 `TAURI_SIGNING_PRIVATE_KEY` 以及(macOS 所需的)Apple 签名 / 公证密钥已在仓库中配置。
- Android tag 发版还需配置:`ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_ALIAS`、`ANDROID_KEY_PASSWORD`（release APK 签名；minisign 仍用 `TAURI_SIGNING_PRIVATE_KEY`）。

### Beta 频道 — `v<v>-beta-tauri`

1. 通过 PR 评审把改动落到 `beta` 分支。
2. **在 `beta` 上**推送标签:`git tag v<v>-beta-tauri && git push origin v<v>-beta-tauri`。
3. CI 会把该 GitHub Release 标记为 `Pre-release`,并上传 `latest-{tgt}-{arch}-beta.json` 与 `latest-android-{arch}-beta.json` 更新清单。Stable 用户的 `releases/latest` 跳转不受影响。
4. 在合适的渠道(issue 讨论串、QQ 群)公告:可选加入的 Beta 用户可从 设置 → 关于 → 加入 Beta 频道 获取。

### Stable 频道 — `v<v>-tauri`

1. 在 Beta 发布充分沉淀后,将 `beta → main` 合并(或直接运行一次最终的双平台冒烟构建)。
2. **在 `main` 上**推送标签:`git tag v<v>-tauri && git push origin v<v>-tauri`。
3. CI 发布一个正常的 GitHub Release,并上传 `latest-{tgt}-{arch}.json` 与 `latest-android-{arch}.json`(无 `-beta` 后缀)。所有 Stable 用户都会通过应用内更新器获得更新。

### 发布后验证(始终执行)

执行 [`CLAUDE.md` → Branch & release-channel workflow → Channel distribution](CLAUDE.md) 中的 5 步清单,并额外核对 Android:

1. Release 页面含 `latest-android-aarch64.json`（Stable 无 `-beta` 后缀混用）。
2. Beta pre-release 含 `latest-android-aarch64-beta.json`,URL 指向具体 tag。
3. 真机:设置 → 关于 → 检查更新 → 下载 → 系统安装器 → 版本号递增。
4. Beta 开关:高级 → 加入 Beta → 手动/自动检查 Beta manifest。
5. 原始端点与 mirror 端点均可访问。

## 致谢

OpenLess 衷心感谢它的赞助商、开发者与贡献者,以及更广泛的 LinuxDo 社区。

我们感谢赞助商让项目的持续工作成为可能,也感谢开发者与贡献者对 OpenLess 的构建、评审与改进。

OpenLess 同样认可并感谢 LinuxDo 社区开放、务实、对开发者友好的氛围。围绕 OpenLess 的许多想法、讨论与早期反馈,都受到了 LinuxDo 所代表的更广阔开源精神的启发。

本致谢不意味着任何官方背书或隶属关系。

## 许可证

OpenLess 基于 [MIT 许可证](LICENSE) 发布。
