# Handoff: OpenLess v1.0 重设计

> 本地说出，本地落字 — 一个跨平台（macOS + Windows）的桌面端语音转写应用 UI 重设计。

---

## Overview

OpenLess 是一款跨平台桌面端语音转写工具：用户按下全局快捷键（默认 **右 Option / Right Alt**）即可在任何应用内录入，转写后文本自动写入光标位置。本次 v1.0 是从 v0.6 的全面重设计，目标：

- **降低决策成本** — 信息架构由 7 个 tab 收缩到 5 个
- **统一跨平台体验** — Mac / Win 仅在窗口顶栏不同，主体 100% 共用
- **强调本地优先** — 无云端账户、无后台同步，所有数据只在本机
- **录音由快捷键唤起** — 主界面不再有「开始录音」按钮，避免分散注意力

## About the Design Files

本目录中的 **HTML / JSX / CSS 文件是设计参考稿**，是用 React + 原生 CSS 写出来的高保真原型，用来表达预期的视觉与交互。**不是要直接拿去发布的生产代码。**

实际开发任务：在你的目标技术栈中（推测是 **Tauri + React** 或 **Electron + React**，也可能是原生 SwiftUI for macOS / WinUI for Windows）**重新实现这套 UI**。如果项目还没选好框架，**Tauri + React + TypeScript** 是这种「本地优先 + 跨平台桌面 + 体积小 + 系统快捷键」需求的最佳选择。

## Fidelity

**High-fidelity (hifi)** — 颜色、字号、间距、圆角、阴影、动画曲线全部已确定。开发时请按 `tokens.css` 里的 CSS 变量原样落地（或翻译成你 UI 框架的 token 系统）。

---

## 文件清单

| 文件 | 用途 |
|---|---|
| `App.html` | **干净的应用入口** — 直接打开看实际产品长什么样（自动检测 OS） |
| `OpenLess Redesign.html` | **设计画布** — 平铺所有平台 × 所有页面，用于评审 |
| `tokens.css` | 设计 tokens（颜色、字体、阴影、圆角、毛玻璃） |
| `chrome.jsx` | 窗口外框（macOS 红黄绿 / Windows Mica 顶栏） |
| `variants.jsx` | 主壳层 `FloatingShell` — 顶栏 + 侧栏 + 底栏 + 主内容 + 设置弹窗 |
| `pages.jsx` | 4 个主 tab 页面（概览 / 历史 / 词汇表 / 风格）+ 设置内容 |
| `capsule.jsx` | 录音胶囊（Dynamic Island 尺寸，3 种状态） |
| `icons.jsx` | 内置 SVG 图标库 |
| `data.js` | 演示数据（mock） |
| `design-canvas.jsx` | 设计画布容器（仅评审用，**生产不需要**） |
| `tweaks-panel.jsx` | 设计画布的右下浮动调参面板（仅评审用，**生产不需要**） |
| `AppIcon.png` | 应用图标 |

> **生产实现只需关注 `chrome.jsx`、`variants.jsx`、`pages.jsx`、`capsule.jsx`、`icons.jsx`、`tokens.css`。** 其余三个（`design-canvas.jsx`、`tweaks-panel.jsx`、`OpenLess Redesign.html`）只是给设计师评审用的脚手架。

---

## 设计 Tokens

### Colors

| Token | Value | 用途 |
|---|---|---|
| `--ol-white` | `#ffffff` | 卡片背景 |
| `--ol-canvas` | `#f7f7f8` | 外壳背景 |
| `--ol-surface` | `#ffffff` | 主内容卡 |
| `--ol-surface-2` | `#fafafa` | 副卡片背景 |
| `--ol-line` | `rgba(0,0,0,0.08)` | 主分割线 |
| `--ol-line-strong` | `rgba(0,0,0,0.14)` | 强调分割线 |
| `--ol-line-soft` | `rgba(0,0,0,0.05)` | 弱分割线 |
| `--ol-ink` | `#0a0a0b` | 主文字 |
| `--ol-ink-2` | `#2a2a2d` | 次级文字 |
| `--ol-ink-3` | `rgba(10,10,11,0.62)` | 辅助文字 |
| `--ol-ink-4` | `rgba(10,10,11,0.42)` | 占位 / 元数据 |
| `--ol-ink-5` | `rgba(10,10,11,0.24)` | 禁用 |
| `--ol-blue` | `#2563eb` | 主点缀色 / 当前态 |
| `--ol-blue-hover` | `#1d4ed8` | hover |
| `--ol-blue-soft` | `#eff4ff` | 蓝色背景态 |
| `--ol-blue-ring` | `rgba(37,99,235,0.22)` | focus 环 |
| `--ol-ok` / `--ol-warn` / `--ol-err` | `#16a34a` / `#d97706` / `#dc2626` | 状态色（克制使用） |

### Glass / 毛玻璃

```css
--ol-glass-bg: rgba(255, 255, 255, 0.62);
--ol-glass-bg-strong: rgba(255, 255, 255, 0.78);
--ol-glass-border: rgba(255, 255, 255, 0.7);
--ol-glass-blur: 20px;

.ol-glass {
  background: var(--ol-glass-bg);
  backdrop-filter: blur(var(--ol-glass-blur)) saturate(160%);
  border: 0.5px solid var(--ol-glass-border);
}
```

外框磨砂背景（窗口背景）：
```css
background:
  radial-gradient(120% 80% at 0% 0%, rgba(255,255,255,0.7) 0%, rgba(255,255,255,0) 60%),
  radial-gradient(100% 70% at 100% 100%, rgba(37,99,235,0.07) 0%, rgba(37,99,235,0) 55%),
  linear-gradient(180deg, rgba(245,245,247,0.92) 0%, rgba(232,232,236,0.92) 100%);
backdrop-filter: blur(40px) saturate(180%);
```

### Shadows

```
--ol-shadow-sm: 0 1px 2px rgba(15,17,22,0.04), 0 0 0 0.5px rgba(0,0,0,0.04)
--ol-shadow-md: 0 1px 2px rgba(15,17,22,0.05), 0 6px 24px -12px rgba(15,17,22,0.10), 0 0 0 0.5px rgba(0,0,0,0.04)
--ol-shadow-lg: 0 20px 60px -20px rgba(15,17,22,0.18), 0 8px 32px -16px rgba(15,17,22,0.10), 0 0 0 0.5px rgba(0,0,0,0.06)
--ol-shadow-xl: 0 40px 120px -40px rgba(15,17,22,0.30), 0 24px 60px -24px rgba(15,17,22,0.15), 0 0 0 0.5px rgba(0,0,0,0.06)
```

### Radii

| Token | 值 | 用途 |
|---|---|---|
| `--ol-r-sm` | `6px` | 小按钮、tag |
| `--ol-r-md` | `10px` | 内容卡片 |
| `--ol-r-lg` | `14px` | 大卡片 |
| `--ol-r-xl` | `18px` | （备用） |
| `--ol-r-2xl` | `22px` | （备用） |
| 窗口外框 | `20px` (mac) / `14px` (win) | 见 `chrome.jsx` |
| 内置主内容卡 | `12px` | 见 `variants.jsx` |
| `--ol-r-pill` | `999px` | 胶囊、tag |

### Typography

```
--ol-font-sans: 'Inter', 'PingFang SC', 'Microsoft YaHei', -apple-system, ..., system-ui, sans-serif
--ol-font-mono: 'JetBrains Mono', 'SF Mono', 'Cascadia Code', 'Consolas', monospace
```

字号 / 字重对照：

| 用途 | size | weight | letter-spacing |
|---|---|---|---|
| Page title | 22px | 600 | -0.02em |
| Section title | 16px | 600 | -0.01em |
| Body | 13px | 500 | 0 |
| Secondary | 12px | 500 | 0 |
| Caption / meta | 11.5px | 500 | 0 |
| Eyebrow | 10.5px | 600 | 0.08em uppercase |
| Mono (timestamp / 数据) | 11–13px | 500 | — |

启用 Inter 的字体特性：`font-feature-settings: 'cv11', 'ss01', 'ss03';`

---

## 信息架构（IA）

```
v0.6（旧）                          v1.0（新）
┌─ 首页                             ┌─ 概览     · Provider 状态 + 今日数据 + 最近识别
├─ 历史记录                         ├─ 历史     · 双栏工作区（原文 vs 润色）
├─ 词汇表                  →        ├─ 词汇表   · ASR 热词 + LLM 上下文，命中计数
├─ 风格                             ├─ 风格     · 4 种润色预设，可启停
├─ 配置 ────┐                       └─ 设置（弹窗触发）
├─ 设置 ────┤── 合并 → 单一「设置」     ├─ 录音
└─ 帮助中心 ┘                          ├─ 提供商
                                       ├─ 快捷键
                                       ├─ 权限
                                       └─ 关于
```

**「设置」不再是 sidebar tab**，而是底栏齿轮按钮唤起的**居中模态弹窗**，宽 720px，高 540px。

---

## 屏幕（Screens）

总览：4 个主 tab × 2 个平台 + 设置弹窗 × 2 = **10 个独立屏幕**。Mac 和 Win 的差异**仅在窗口顶栏**。

### 公共骨架（FloatingShell）

每个屏幕都嵌套在以下骨架内：

```
┌──────────────────────────────────────────────────┐  ← 外框（毛玻璃）
│ ● ● ●                                            │     ↑ macOS：红黄绿浮于左上角，无独立标题栏
│ ┌─Sidebar─┐ ┌──── Main 白色卡片（圆角 12px）────┐│
│ │ Logo    │ │                                   ││
│ │         │ │   <Page />                        ││
│ │ 概览    │ │                                   ││
│ │ 历史    │ │                                   ││
│ │ 词汇表  │ │                                   ││
│ │ 风格    │ │                                   ││
│ │         │ │                                   ││
│ │ 快捷键  │ │                                   ││
│ │ 提示    │ │                                   ││
│ │ BETA    │ │                                   ││
│ └─────────┘ └───────────────────────────────────┘│
│ [👤] [✉️] [⚙️] [?]              v1.0.0 · 检查更新│  ← 底栏（图标行）
└──────────────────────────────────────────────────┘
```

| 区域 | 尺寸 | 背景 | 备注 |
|---|---|---|---|
| 外框 | 1240 × 800 (默认 mock 尺寸) | 毛玻璃磨砂 | 圆角 mac 20 / win 14 |
| Sidebar | 188px 宽 | 半透明灰 (`linear-gradient(180deg, rgba(247,247,250,0.85), rgba(247,247,250,0.5))`) | 无右侧分割线 |
| Main 白卡 | 弹性宽 | `var(--ol-surface)` | 圆角 12px，外间距 6/8/6/0 (T/R/B/L) |
| 底栏 | 高 44px | 透明（贴在外框磨砂上）| 4 个图标按钮 + 版本信息 |
| Mac 顶栏 | **0px**（无独立栏） | — | 三点按钮 `position: absolute; top: 13; left: 14`，浮于外框毛玻璃上 |
| Win 顶栏 | 36px | 半透明 | logo + 标题 + min/max/close |

**SidebarItem** active：`background: var(--ol-blue-soft); color: var(--ol-blue);` 圆角 8px。

### Screen 1 · 概览 / Overview (`pages.jsx` `Overview`)

**目标**：用户打开应用首先看到的页面，要在 3 秒内传达「我现在能不能正常使用」+「我今天用了多少」。

**布局**（垂直堆叠）：

1. **PageHeader** — 标题 `今日概览` + eyebrow `DASHBOARD` + 副标题 `本地说出，本地落字。下面是你今日的口述节奏与系统状态。`
2. **快捷键提示卡**（蓝色 soft 背景 + 蓝色 ring）—「按 **右 Option** 开始录音」
3. **Provider 状态行** — ASR、LLM、本地存储 三张并排状态卡
4. **今日数据指标** — 字符数、片段数、平均延迟、累计时长 四张数字卡（使用 mono 字体）
5. **最近识别** — 列表，每行：时间戳 (mono) · 风格 tag · 时长 · 预览文字（首 60 字符）

### Screen 2 · 历史 / History (`pages.jsx` `History`)

**布局**：双栏

- **左栏 360px**：会话列表
  - 顶部 filter chips: `全部 / 今天 / 本周 / 本月`
  - 列表行：时间戳 + 风格 tag + 时长 + 预览
  - 选中行：`background: var(--ol-blue-soft); border-left: 2px solid var(--ol-blue);`
- **右栏弹性**：选中会话的详情
  - 头部：时间戳 + 风格 + 时长 + 操作按钮（重新润色 / 复制 / 删除）
  - **原文** vs **润色后** 双栏对比，可切换风格重新生成
  - 底部：词汇表命中提示（如有）

### Screen 3 · 词汇表 / Vocab (`pages.jsx` `Vocab`)

**布局**：

1. PageHeader — `词汇表`
2. 说明卡：词汇表会同时作为 ASR 热词 + LLM 上下文使用
3. 添加输入框（左：词条，右：发音 / 解释，最右：保存按钮）
4. 已有词汇 chip 网格：每个 chip 显示「词条 + 命中次数」，hover 出现删除按钮

### Screen 4 · 风格 / Style (`pages.jsx` `Style`)

**布局**：

1. PageHeader — `风格`
2. 4 张预设卡片网格（2×2 或 1×4）：
   - **清晰** clear · 默认开
   - **简洁** concise
   - **专业** professional
   - **会议纪要** meeting
3. 每张卡片：标题 + 一句话描述 + 启停 toggle + 「设为默认」
4. 选中态：蓝色边 (`border: 1px solid var(--ol-blue)`)
5. 底部 Prompt 编辑器（mono 字体），可自定义风格

### Screen 5 · 设置弹窗 / Settings Modal (`pages.jsx` `Settings`)

**触发**：底栏齿轮图标点击

**外观**：
- 居中弹窗，宽 720 / 高 540（屏幕大于 1100×700 时；否则适应）
- 圆角 14px
- 阴影 `--ol-shadow-xl`
- 背景 `var(--ol-surface)`
- 遮罩 `rgba(0,0,0,0.18)` + `backdrop-filter: blur(8px)`

**内部布局**：左右分栏

- **左 200px**：sub-nav（录音 / 提供商 / 快捷键 / 权限 / 关于 + 帮助中心 / 版本说明）
- **右弹性**：所选 section 的表单内容
- **顶部**：标题 + 关闭按钮（×）

---

## 录音胶囊 / Recording Capsule (`capsule.jsx`)

**位置**：屏幕顶部居中（模仿 macOS Dynamic Island 的视觉位置），不是嵌在应用内部。

**尺寸**：约 `220 × 38px`（视状态略变化）

**入场动画**：

```css
@keyframes capsule-in {
  from { opacity: 0; transform: translate(-50%, -8px) scale(.7); }
  to   { opacity: 1; transform: translate(-50%, 0) scale(1); }
}
/* 350ms cubic-bezier(.2, .9, .3, 1.1) */
```

### 三种状态

| 状态 | 触发 | 视觉 |
|---|---|---|
| **录制中** | 按下右 Option | 深色 pill `[×] 红色呼吸点 think 计时 [×]` |
| **转写中** | 松开 / 再次按下 | think 文字闪光（白→透明 L→R 扫光），底部灰色进度条从左填到右 |
| **完成** | 转写结束 | 蓝色 pill + ✓ + 「已插入 N 字符」，0.4s 后淡出 |

**Mac & Windows 完全相同**（深色 pill 是统一视觉锚点，跨平台一致）。

**关键样式**：

- 背景 `linear-gradient(180deg, #1f1f23 0%, #0a0a0b 100%)`
- 圆角 `999px`（pill）
- 文字 `#ffffff` / `rgba(255,255,255,0.7)`
- 红点 `#ff453a`，呼吸 `@keyframes pulse { 50% { opacity: 0.4 } }` 1.2s ease-in-out
- 完成色 `var(--ol-blue)` 背景

---

## Interactions & Behavior

| 交互 | 行为 |
|---|---|
| **全局快捷键** | 默认右 Option (mac) / 右 Alt (win)。可在「设置 → 快捷键」修改。**这是录音的唯一入口**——主界面不再有按钮。 |
| **Sidebar 切换** | 点击导航项 → 切换 main 内容。无过渡动画（瞬间切换）。 |
| **设置弹窗** | 底栏齿轮点击 → 弹窗淡入（200ms）+ 遮罩淡入。Esc 或点击遮罩关闭。 |
| **历史会话选中** | 左栏点击 → 右栏即时切换，无加载态（数据本地）。 |
| **风格预设切换** | 点击卡片 → 即时变为「当前默认」。可同时启用多个，但只有 1 个是默认。 |
| **词汇表添加** | 输入回车 → chip 飞入动画（180ms scale + fade）。 |
| **窗口控制** | mac：三点 hover 显示 ×/-/⤢ 符号。win：right-side 三键 (- ☐ ✕)。 |

---

## State Management

**最简实现**用 React `useState` + `useReducer` 即可，无需 Redux / Zustand。

需要的全局状态：

```ts
type AppState = {
  currentTab: 'overview' | 'history' | 'vocab' | 'style';
  settingsOpen: boolean;

  // Settings
  settings: {
    asrProvider: 'whisper-local' | 'openai-whisper' | ...;
    llmProvider: 'ollama' | 'openai' | 'claude' | ...;
    hotkey: string;          // e.g. 'RightOption'
    defaultStyle: 'clear' | 'concise' | 'professional' | 'meeting';
    enabledStyles: Set<...>;
  };

  // Data
  vocab: { word: string; note?: string; hits: number }[];
  history: { id; ts; durationMs; styleUsed; rawText; polishedText }[];
  metrics: { charsToday; segmentsToday; avgLatencyMs; totalDurationToday };

  // Recording (driven by global hotkey listener)
  recording: { state: 'idle' | 'recording' | 'transcribing' | 'done'; startTs?: number; chars?: number };
};
```

**所有数据本地持久化**。Tauri 用 `tauri-plugin-store` / `sqlite`；Electron 用 `electron-store` / `better-sqlite3`。**不要**做云同步。

---

## Assets

- **`AppIcon.png`** — 应用图标（项目自带，建议生产替换为多尺寸 `.icns` / `.ico`）
- **`Inter` 字体** — Google Fonts CDN（`tokens.css` 顶部 `@import`）。生产建议下载到 assets 内打包，避免离线时无字体。
- **`PingFang SC` / `Microsoft YaHei`** — 系统字体，无需打包。
- **`JetBrains Mono`** — Mono 字体，建议同样打包到 assets。
- **图标** — 全部 SVG inline（见 `icons.jsx`），不依赖外部图标库。

---

## 跨平台实现要点

| 关注点 | macOS | Windows |
|---|---|---|
| 窗口装饰 | 无系统标题栏（`titleBarStyle: 'hiddenInset'`），自绘三点按钮 | 用 Mica 透明 + 自绘三键控制 |
| 全局快捷键 | `CGEventTap` / Tauri `globalShortcut` | `RegisterHotKey` / Tauri `globalShortcut` |
| 麦克风权限 | TCC 弹窗（Info.plist `NSMicrophoneUsageDescription`） | UWP 兼容 / 直接 WASAPI |
| 顶部胶囊位置 | 屏幕顶端居中下方 ~14px | 顶部任务栏正下 ~14px |
| 字体回退 | `'PingFang SC'` | `'Microsoft YaHei'` |

---

## 开发步骤建议（给 Claude Code）

1. **如果项目还不存在**：用 `npm create tauri-app@latest` 初始化 Tauri + React + TS 项目。
2. **复制 `tokens.css`** 到 `src/styles/`，全局引入。
3. **逐个翻译 JSX 组件到 TS + 生产组件**：
   - `chrome.jsx` → `<WindowChrome>` (考虑用 Tauri `tauri-plugin-window-decorations`)
   - `variants.jsx::FloatingShell` → `<AppShell>`
   - `pages.jsx` 的 4 个页面 → 4 个 route
   - `pages.jsx::Settings` → 弹窗组件
   - `capsule.jsx` → 独立透明窗口（Tauri 多窗口）覆盖在屏幕顶部
4. **数据层**：先用本地 mock（参考 `data.js`），再接 SQLite。
5. **快捷键**：用 Tauri `globalShortcut::register` 注册右 Option。
6. **录音**：用 Web `MediaRecorder` API 即可（Tauri 支持），转写本地用 whisper.cpp 绑定，远程用 OpenAI API。

---

## 评审参考

打开 **`OpenLess Redesign.html`** 可以看到所有平台 × 所有页面在一张设计画布上的对照图。**`App.html`** 是干净的应用入口，自动按访问者 OS 切换 Mac / Win 顶栏。

如有疑问，所有 UI 决策的依据都在 `tokens.css` + `chrome.jsx` + `variants.jsx` + `pages.jsx` 这 4 个文件里直接可读，不存在「文档没写清楚」的隐含规则。
