# OpenLess 使用指南

[English](USAGE.md) | [简体中文](USAGE.zh.md)

## 安装

### macOS

1. 从 [Releases](https://github.com/Open-Less/openless/releases/latest) 下载 `OpenLess_<版本>_aarch64.dmg`。
2. 打开 dmg，将 OpenLess.app 拖入「应用程序」文件夹。
3. 双击启动。

### Windows

1. 从 [Releases](https://github.com/Open-Less/openless/releases/latest) 下载 `OpenLess_<版本>_x64-setup.exe`。
2. 运行安装程序，按提示完成安装。
3. 从开始菜单启动 OpenLess。

### Linux

Linux 使用原生 egui 宿主，不依赖 Tauri 或 WebKitGTK。

1. 暂未随 Release 提供安装包：Linux 打包在 L01–L12 真机验收完成前保持暂停，可先从源码构建（见 [README](README.zh.md) 的构建章节）。
2. 需要 glibc ≥ 2.39（Ubuntu 24.04 或同级发行版）。
3. 全局快捷键由 fcitx5 插件提供，须在 fcitx5 中启用 OpenLess 插件并配置按键；没有应用内更新与 AppImage。

### Android

1. 从 [Releases](https://github.com/Open-Less/openless/releases/latest) 下载手机对应的 `OpenLess_<版本>_arm64-v8a.apk`（较老的 32 位机型用 `armeabi-v7a`）。
2. 允许「安装未知来源应用」后安装。
3. 首次录音时授予麦克风权限；如需用键盘插入，请在系统设置中启用 OpenLess 键盘并设为当前输入法。
4. 应用内更新在 设置 → 关于与更新。

### iOS

1. 暂无可直接下载的 IPA。用 Xcode 从 [`openless-all/ios/`](openless-all/ios/README.zh.md) 源码构建，并用个人团队签名。
2. 构建、安装与启用键盘的完整步骤见 [iOS 指南](openless-all/ios/README.zh.md)。

### HarmonyOS（适配中）

1. 从同一个 Release 下载未签名的 `OpenLess_harmony_arm64.hap`。
2. 真机安装前必须自己签名，手机或平板会拒绝未签名的 HAP：调试证书的包名必须是 `openless.hm.com`，调试 Profile 必须包含目标设备，包名不匹配的 Profile 不能复用。用 DevEco Studio 的自动签名（登录华为账号）最省事；走命令行时把证书与 Profile 放进本地签名目录（默认 `~/.local/share/harmony/signing/local-test`，可用 `OPENLESS_HARMONY_SIGNING_DIR` 覆盖），构建会输出 `entry-default-signed.hap`。
3. 用 DevEco Studio 或 `hdc` 安装签名后的 HAP；AppGallery 分发包需人工提交。
4. 首次使用前先打开主应用完成渠道配置，输入法才会拿到可用的语音设置；目前没有更新流程。构建与签名见 [`harmony/README.zh.md`](openless-all/app/harmony/README.zh.md)。

---

## 首次配置

### macOS 权限

首次启动后，在「系统设置 → 隐私与安全」中授予以下权限：

1. **麦克风** — 允许 OpenLess 录音。
2. **辅助功能** — 允许 OpenLess 读写当前焦点输入框。授权后须**完全退出并重新启动** OpenLess，快捷键才会生效。

### Windows 权限

1. 按系统提示授予**麦克风**权限。
2. 打开 OpenLess → 「设置 → 权限与数据」，确认全局快捷键监听器状态显示为「已启动」。

### Android 权限

1. **麦克风** — 首次录音时按提示授予。
2. **输入法** — 在 系统设置 → 语言与输入法 中启用 OpenLess 键盘，并设为当前输入法后才能在其它应用里落字。
3. **可选：无障碍** — 「从纠正中学习」需要在支持的编辑器里观察文本时才会用到；不开启时可用历史详情里的「记住一个词」手动确认。

### iOS 权限

1. **麦克风**与**语音识别**在首次使用时按提示授权。
2. **键盘** — 系统设置 → 通用 → 键盘 → 键盘 → 添加新键盘 → OpenLess。
3. **完全访问** — 需要共享设置与语音控制时打开；基础打字不依赖完全访问。

### HarmonyOS 权限

1. **麦克风**与**网络**权限沿用系统声明。
2. **输入法** — 在系统设置的输入法管理中启用「OpenLess 输入法」，然后在输入框切换过去；语音结果通过系统分配的光标输入会话写入，不接入无障碍。
3. 未启用输入法时，可用复制后手动粘贴作为退路。

### 填入凭据

不会配火山 ASR 的话，先看这篇图文引导：  
[OpenLess 火山 ASR 配置](docs/volcengine-setup.md)

用讯飞实时语音转写的话，看 [OpenLess 讯飞 ASR 配置](docs/xfyun-asr.md)（AppID + API Key，设置 → AI 与模型 → 语音识别渠道 → 讯飞实时语音转写）。

打开 OpenLess → **设置**，填入以下字段：

| 字段 | 说明 |
| --- | --- |
| 火山引擎 App ID | 语音识别服务的应用 ID |
| 火山引擎 Access Token | 语音识别访问凭据 |
| 火山引擎 Resource ID | 语音识别资源 ID |
| Ark API Key | 文本润色服务的 API Key |
| Ark Model ID | 使用的模型 ID（如 `doubao-pro-32k`） |
| Ark Endpoint | 接口地址，默认 `https://ark.cn-beijing.volces.com/api/v3/chat/completions` |

保存后设置立即生效，无需重启。

火山引擎 ASR + Ark 是最短路径。设置 → AI 与模型 → 语音识别渠道 / 文字处理渠道 里还可选择讯飞实时 ASR、腾讯云混元、阿里云百炼、阶跃星辰、智谱 GLM、小米 MiMo、OpenAI 兼容端点等近三十个 ASR 渠道与二十多个润色渠道（完整清单见 [README](README.zh.md) 的「功能现状」）。凭据保存在系统凭据保险库（service = `com.openless.app`）：macOS 钥匙串、Windows 凭据管理器、Linux keyring；不会以明文写入磁盘。

---

## 基本使用

### 开始录音

按下全局快捷键（桌面端默认 macOS 右 Option、Windows 右 Control；Linux 的快捷键由 fcitx5 插件提供，需在 fcitx5 中配置；Android、iOS 与 HarmonyOS 在应用内或键盘上触发）。  
屏幕边缘会出现状态胶囊，显示「录音中」。

### 结束录音

再次按下同一快捷键。OpenLess 会：

1. 停止录音并发送音频进行转写。
2. 对转写结果按当前输出模式进行润色。
3. 将润色后的文字插入当前焦点输入框（失败时自动复制到剪贴板）。

### 取消录音

录音过程中按 `Esc`，当前录音内容会被丢弃，不做任何插入。

---

## 输出模式

在 OpenLess 主窗口的胶囊或「设置」中切换模式：

| 模式 | 说明 |
| --- | --- |
| 原文 | 直接输出转写文字，不做任何修改 |
| 轻度润色 | 修正语气词、标点、明显错字，保留原意 |
| 清晰结构（AI prompt 模式） | 把口语整理成有结构、有约束、有上下文的 prompt，适合直接喂给 ChatGPT / Claude / Cursor |
| 正式表达 | 将口语转换为正式书面语 |

---

## 词典

词典用于提高特定词汇的识别准确率（产品名、人名、专有名词等）。

1. 打开主窗口 → **词典**。
2. 点击「新建」，填入正确拼写、分类和备注。
3. 启用后，词条会作为热词注入 ASR 识别阶段，并在润色阶段辅助语义判断。

---

## 历史记录

主窗口 → **历史**，可查看所有录音记录，包括原始转写和润色结果。

---

## 更换快捷键

主窗口 → **设置 → 快捷键**，选择触发键。  
macOS 支持右侧修饰键（Option / Control / Command / Shift）；Windows 支持左右修饰键与鼠标侧键（Mouse4 / Mouse5）。

Linux 的全局快捷键由 fcitx5 插件管理，在 fcitx5 配置中绑定；Android、iOS 与 HarmonyOS 不注册系统级快捷键，使用应用内或键盘面板上的语音按钮。

---

## 常见问题

**Q: 快捷键没反应？**  
macOS：确认已授予辅助功能权限，且授权后重启过 OpenLess。  
Windows：在「设置 → 权限与数据」中检查监听器状态。

**Q: 识别结果为空或是占位文字？**  
检查火山引擎 ASR 凭据是否填写正确。填写正确后识别才能正常工作。

**Q: 文字没有插入，只是复制到了剪贴板？**  
当目标输入框不支持辅助功能写入时（如某些安全限制的应用），OpenLess 会自动回退到剪贴板复制，手动粘贴即可。

**Q: 在 Windows 玩 Minecraft 等全屏游戏时，OpenLess capsule 不弹出 / 字符无法输入？**  
这是 **Windows 操作系统层面的限制**，OpenLess 应用本身无法绕过（详见 [issue #457](https://github.com/Open-Less/openless/issues/457)）：

- **独占全屏（exclusive fullscreen）**：标准应用窗口（包括 OpenLess capsule）**不会绘制在独占全屏 DirectX/OpenGL 应用之上**。请把游戏切换到 **无边框窗口化全屏（Borderless Windowed Fullscreen）**。Minecraft：视频设置 → 全屏 关闭（保持窗口最大化即可）。
- **管理员权限不一致（UIPI）**：若游戏以管理员身份运行而 OpenLess 不是，Windows 阻止 OpenLess 接收游戏前台的按键，hotkey 完全不触发。让两者权限对齐（要么都以管理员运行，要么都以普通用户运行）。
- **游戏聊天框未打开**：识别字符通过模拟键盘事件落字。Minecraft 中必须先按 `T` 打开聊天框，OpenLess 的输入才会落到聊天里。

macOS 不存在独占全屏（所有"全屏"都是带 Spaces 的无边框窗口），所以此限制不适用。

**Q: 润色结果和预期不符？**  
尝试切换输出模式，或在词典中添加相关专有名词。

**Q: Linux 上有自动更新吗？**  
没有。Linux 提供（真机验收完成后的）deb/rpm 与源码构建，不提供 AppImage 或应用内更新，升级需手动安装新包。

**Q: 鸿蒙或 iOS 怎么更新？**  
鸿蒙目前没有更新流程，需要重新安装新的 HAP；iOS 需要从源码重新构建并签名。两者都不会收到应用内更新提示。

**Q: 怎么用本地识别，不上传音频？**  
本地模型不会自动接管你已经在用的渠道：必须自己把本地渠道设为列表最上方的启用渠道；只有该本地渠道在处理请求时，音频才不出本机。

Windows：在「设置 → AI 与模型 → 本地 ASR 模型」下载模型，再到语音识别渠道列表里添加并启用对应的本地渠道：本地 Whisper（Foundry Local）、本地 sherpa-onnx（实验性）或本地 Whisper（批量解码）。macOS：在「设置 → 实验与扩展 → 实验性本地语音模型」中开启 TextStream、SenseVoice Small 或 Embedding Gemma——模型按需下载，不随安装包分发；下载完成后应用会提示前往渠道列表。

两种平台最后都要在「设置 → AI 与模型 → 语音识别渠道」里启用该渠道并拖到最上方：请求使用第一个已启用的渠道，本地渠道不在最前时音频仍会发往原来的云端渠道。macOS 的实验模型走传统管线（ASR + LLM）；若当前是多模态模式，请先切回传统模式。

---

## 社区与支持

欢迎通过 QQ 加入用户群，反馈问题或交流使用体验：

**QQ 一群：1078960553（已满员）**  
**QQ 二群：1095794054**
