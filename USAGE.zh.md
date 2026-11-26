# OpenLess 使用指南

[English](USAGE.md) | [简体中文](USAGE.zh.md)

> 在线用户文档持续维护于 **<https://openless.top/zh/docs/>**（English: **<https://openless.top/en/docs/>**）。本指南为仓库内配套的安装、权限、日常听写与排查手册。

## 安装

各平台下载入口与安装步骤亦见官网：<https://openless.top/zh/docs/installation/>。

### macOS（14 Sonoma 及以上）

1. 前往 [GitHub Releases](https://github.com/Open-Less/openless/releases/latest) 下载 `OpenLess_<version>_aarch64.dmg`（Apple Silicon）或 `OpenLess_<version>_x64.dmg`（Intel）。
2. 打开 DMG，将 **OpenLess.app** 拖入「应用程序」文件夹。
3. 双击启动。

也可通过 Homebrew 安装正式版（开启更新提醒见 [docs/guides/homebrew.md](docs/guides/homebrew.md)）：

```bash
brew tap Open-Less/openless https://github.com/Open-Less/openless
brew install --cask openless
brew update && brew upgrade openless
```

公开版本均按 [RELEASING.zh.md](RELEASING.zh.md) 完成签名与公证；单独提供的开发构建可能使用临时签名。

### Windows（10 及以上）

1. 前往 [GitHub Releases](https://github.com/Open-Less/openless/releases/latest) 下载 `OpenLess_<version>_x64-setup.exe` 或 `OpenLess_<version>_x64_en-US.msi`。
2. 运行安装程序并按提示完成安装。
3. 从开始菜单启动 OpenLess。

### Linux

Linux 使用原生 `egui` 宿主，不依赖 Tauri 或 WebKitGTK。

1. Linux 发布包打包当前暂停（待完成真机验收），暂请按 [openless-all/README.zh.md#共享后端与-linux-宿主](openless-all/README.zh.md#共享后端与-linux-宿主) 从源码构建。
2. 需 glibc ≥ 2.39（如 Ubuntu 24.04 及同等发行版）。
3. 全局快捷键由 `fcitx5` 插件提供：在 `fcitx5` 中启用 OpenLess 插件并绑定按键。Linux 不提供 AppImage 或应用内自动更新。

### Android（8.0 及以上）

1. 前往 [GitHub Releases](https://github.com/Open-Less/openless/releases/latest) 下载 `OpenLess_<version>_arm64-v8a.apk`（老款 32 位设备选择 `armeabi-v7a`）。
2. 允许安装未知来源应用并完成安装。
3. 首次录音时授予麦克风权限；如需在其他应用中输入文字，请在系统「语言与输入法」中启用 OpenLess 键盘并设为当前输入法。
4. 应用内更新位于「设置 → 关于与更新」。

### iOS（16 及以上）

1. 目前不提供预签名 IPA，请使用 Xcode 从 `openless-all/ios/` 源码构建并用个人证书签名安装。
2. 完整构建、签名与键盘启用步骤见 [openless-all/ios/README.zh.md](openless-all/ios/README.zh.md)。

### HarmonyOS（适配中）

1. 从同一 Release 下载未签名的 `OpenLess_harmony_arm64.hap`。
2. 安装到手机/平板前需使用匹配包名 `openless.hm.com` 且包含本机的调试证书与 Profile 签名（可使用 DevEco Studio 自动签名，或将签名材料放入 `~/.local/share/harmony/signing/local-test`，可通过 `OPENLESS_HARMONY_SIGNING_DIR` 覆盖）。
3. 通过 DevEco Studio 或 `hdc` 安装；首次请先打开主应用完成语音服务配置再切换输入法。构建与签名详情见 [openless-all/app/harmony/README.zh.md](openless-all/app/harmony/README.zh.md)。

---

## 首次配置

### 系统权限

- **macOS**：在「系统设置 → 隐私与安全」中授予 **麦克风** 与 **辅助功能** 权限。授予辅助功能后必须**完全退出并重新启动** OpenLess，全局快捷键才会生效。
- **Windows**：按提示授予 **麦克风** 权限，并在「设置 → 权限与数据」确认全局快捷键监听器状态为「已启动」。
- **Android**：首次录音时授予 **麦克风** 权限，并在系统输入法设置中启用 **OpenLess 键盘**。无障碍权限为可选项，仅在启用「从纠正中学习」观察编辑器文本时需要。
- **iOS**：按提示授予 **麦克风**（及语音识别，如启用）。在「系统设置 → 通用 → 键盘 → 键盘 → 添加新键盘 → OpenLess」启用键盘，并开启 **允许完全访问** 以共享设置与语音控制。
- **HarmonyOS**：按系统声明授予 **麦克风** 与 **网络** 权限，在系统输入法管理中启用「OpenLess 输入法」即可（无需无障碍权限）。

### 配置语音与模型服务

打开 OpenLess「设置 → AI 服务与模型」，根据需求任选一条管线。各服务商的详细申请与填写步骤请以官网文档为准：

- **官网配置文档**：<https://openless.top/zh/docs/setup/>（English: <https://openless.top/en/docs/setup/>）
- **仓库快速入口**：[docs/guides/speech-services.md](docs/guides/speech-services.md)

支持三条识别与润色管线：
1. **传统模式（ASR + 可选 LLM）**：使用火山引擎、阿里云百炼、讯飞实时语音转写（RTASR）、腾讯云混元、OpenAI 兼容端点或本地模型完成语音转写，再按需交由 LLM 去口癖、改口修正或翻译。
2. **多模态模式（单模型听写）**：由 Google Gemini、OpenAI Audio、阿里云 Qwen-Omni 等支持音频输入的模型一步完成转写与整理。
3. **DeepSeek 大统一**：使用 DeepSeek 官方 API Key 统一承接语音听写、问答、原生联网搜索与回答朗读。

所有 API 密钥均保存在操作系统凭据保险库（service = `com.openless.app`：macOS 钥匙串、Windows 凭据管理器、Linux Secret Service、iOS Keychain），不以明文写入磁盘配置。

---

## 日常使用

语音输入技巧与润色前后对照示例亦见官网：<https://openless.top/zh/docs/voice-input/>。

### 录音与连续听写

- **开始与结束**：按下全局快捷键（默认 macOS 右 Option、Windows 右 Control；Linux 在 `fcitx5` 中绑定；移动端在应用内或键盘触发）。支持 **切换式** 与 **按住说话** 两种方式，亦支持有线耳机线控 `MediaPlayPause`。
- **连续输入与后台异步队列**：前一段录音一结束即转入后台异步队列（冻结原目标应用与光标上下文），你可立刻说下一段，处理完成后按顺序写回光标处。
- **随时取消**：在录音、润色或插入任意阶段按 `Esc`，立即丢弃当前片段。
- **安全落字**：优先直接写入当前焦点输入框；遇到受限应用无法直写时自动复制到剪贴板，确保内容不丢失。

### 输出模式、风格包与 Marketplace

可在主窗口、胶囊或设置中切换输出模式：

| 模式 | 说明 |
| --- | --- |
| **原文** | 直接输出转写文字，不经过 LLM 改写 |
| **轻度润色** | 去除口癖与语气词、修正标点与口述改口，保留原意与语气 |
| **清晰结构（AI Prompt）** | 将松散口语整理为带背景、约束与目标的结构化提示词，可直接发给 ChatGPT / Claude / Cursor |
| **正式书面** | 改写为规范、得体的书面表达，适合邮件与汇报 |

你还可以在主窗口创建自定义 **风格包**、配置按应用自动切换规则，或在内置 **Marketplace** 一键安装社区风格包。润色与改写参考样例见 [docs/references/polish-reference-examples.sample.jsonl](docs/references/polish-reference-examples.sample.jsonl)。

### 随便问与选区改写

桌面端提供独立「随便问」窗口与选区改写能力。写回原输入框前会先核实目标上下文；无法确认安全写回时，结果会保留在窗口中供手动复制。

### 个人词典与光标上下文

- **个人词典**：在主窗口 → **词典** 中添加人名、产品名与术语。已启用词条会自动作为 ASR 热词（渠道支持时）并注入润色提示词。
- **从纠正中学习（实验性，默认关闭）**：在「设置 → 实验与扩展」开启后，可在支持的编辑器里观察你对落字结果的修正并弹窗询问是否记住；所有建议均须手动确认，不静默学习、不上传。
- **光标上下文（默认关闭）**：在「设置 → 隐私 → 数据存储」开启后，听写时会读取光标前后数百字符（macOS / Windows UIA / Android 键盘 `InputConnection`），辅助模型区分同音字。密码框、安全输入、密码管理器与终端始终自动排除。

### 历史记录、手机远程输入与加密云同步

- **历史记录**：在主窗口 → **历史** 查看每次录音的转写原文与润色结果。
- **手机远程输入**：在电脑端开启远程输入，按 [手机远程输入指南](docs/guides/remote-input.md) 核对本机 HTTPS 证书指纹并输入配对码，即可用手机录音并把文字写入电脑光标处。
- **加密云同步**：在「设置 → 云服务」登录 GitHub、阅读范围说明后开启；偏好设置、已登记密钥与历史文本先在本机端到端加密再上传，请妥善保存同步密码。

### 更换快捷键

在主窗口 → **设置 → 快捷键** 中选择触发键：macOS 支持右侧修饰键（Option / Control / Command / Shift），Windows 支持左右修饰键与鼠标侧键（Mouse4 / Mouse5），Linux 在 `fcitx5` 中绑定。

---

## 常见问题

**Q: 快捷键没反应？**  
macOS：确认已在系统设置中授予辅助功能权限，并且**完全退出后重启**过 OpenLess。  
Windows：在「设置 → 权限与数据」确认全局快捷键监听器已启动。

**Q: 识别结果为空或提示未配置？**  
在「设置 → AI 服务与模型」核对当前启用渠道的 API 凭据并点击测试，详见官网 <https://openless.top/zh/docs/setup/>。

**Q: 文字没有直接插入，只是复制到了剪贴板？**  
当目标应用限制辅助功能或输入法注入时，OpenLess 会自动回退到剪贴板，手动按 `Cmd+V` / `Ctrl+V` 粘贴即可。

**Q: 在 Windows 玩 Minecraft 等全屏游戏时，胶囊不弹出或无法输入？**  
这是 Windows 系统机制限制（详见 [issue #457](https://github.com/Open-Less/openless/issues/457)）：
- **独占全屏**：普通窗口无法绘制在独占全屏 DirectX/OpenGL 应用上方，请将游戏切换为 **无边框窗口化全屏**。
- **权限不一致（UIPI）**：若游戏以管理员运行而 OpenLess 未以管理员运行，系统会拦截按键事件；请保持两者权限一致。
- **未打开聊天框**：请先按游戏内快捷键（如 Minecraft 按 `T`）打开输入框再说话。

**Q: 怎么使用本地识别，让音频不出本机？**  
先下载本地模型（Windows：「设置 → AI 与模型 → 本地 ASR 模型」；macOS：「设置 → 实验与扩展 → 实验性本地语音模型」），再在「语音识别渠道」列表里启用该本地渠道并**拖到最上方**（请求始终使用排在第一位的已启用渠道；若当前处于多模态模式，请先切回传统模式）。

**Q: Linux、iOS 或鸿蒙有应用内自动更新吗？**  
没有。Linux 需重新源码构建或手动安装新包；iOS 需从源码重新编译签名；鸿蒙需重新安装签名后的新 HAP。

---

## 社区与支持

欢迎加入 QQ 交流群反馈问题或分享风格包：

- **QQ 一群**：`1078960553`（已满员）
- **QQ 二群**：`1095794054`
