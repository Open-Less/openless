# OpenLess 使用指南

## 安装

### macOS

1. 从 [Releases](https://github.com/appergb/openless/releases/latest) 下载 `OpenLess_<版本>_aarch64.dmg`。
2. 打开 dmg，将 OpenLess.app 拖入「应用程序」文件夹。
3. 双击启动。

### Windows

1. 从 [Releases](https://github.com/appergb/openless/releases/latest) 下载 `OpenLess_<版本>_x64-setup.exe`。
2. 运行安装程序，按提示完成安装。
3. 从开始菜单启动 OpenLess。

---

## 首次配置

### macOS 权限

首次启动后，在「系统设置 → 隐私与安全」中授予以下权限：

1. **麦克风** — 允许 OpenLess 录音。
2. **辅助功能** — 允许 OpenLess 读写当前焦点输入框。授权后须**完全退出并重新启动** OpenLess，快捷键才会生效。

### Windows 权限

1. 按系统提示授予**麦克风**权限。
2. 打开 OpenLess → 「设置 → 权限」，确认全局快捷键监听器状态显示为「已启动」。

### 填入凭据

不会配火山 ASR 的话，先看这篇图文引导：  
[OpenLess 火山 ASR 配置](docs/volcengine-setup.md)

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

---

## 基本使用

### 开始录音

按下全局快捷键（默认 macOS 右 Option，Windows 右 Control）。  
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
macOS 支持右侧修饰键（Option / Control / Command / Shift）；Windows 支持右 Control。

---

## 常见问题

**Q: 快捷键没反应？**  
macOS：确认已授予辅助功能权限，且授权后重启过 OpenLess。  
Windows：在「设置 → 权限」中检查监听器状态。

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

---

## 社区与支持

欢迎通过 QQ 加入用户群，反馈问题或交流使用体验：

**QQ 群：1078960553**
