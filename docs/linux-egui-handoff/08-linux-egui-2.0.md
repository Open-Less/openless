# Linux-egui 2.0 实施与交付记录

基准：`Open-Less/openless:beta` 的 `867a0d8ce746c64cadc1be68fead98d9838b9f15`。分支：`Linux-egui`。执行时锁定基准，后续上游变动不混入本次复刻。原 PI 工作仍在 `feat/bundled-pi-computer` 的 `75a3a2710c251e4deca453fab17a07a3fb79047b`。

## 源码对应关系

下表路径均相对于 `openless-all/app/`。以基准 React 源码为设计来源；没有运行前端界面测试、截图比对或浏览器自动化。像素一致性、真实桌面焦点及设备行为由使用者按后面的矩阵验收。

| React 2.0 来源 | egui 实现与服务入口 |
| --- | --- |
| `src/pages/Overview.tsx` | `linux-egui/src/ui/frontend/pages.rs` 概览；Core 历史、活动、凭据配置状态；异步结果按请求代次接收 |
| `src/pages/History.tsx` | 同文件历史；`host_history.rs` 播放/导出/重转写/试用润色/取消/删除；原文和最终文本分别展示 |
| `src/pages/Vocab.tsx` | 词汇启停、纠错规则、真实内置及自定义预设，统一调用 Core 持久化 |
| `src/pages/Style.tsx` | `host_styles.rs` 听写/选区风格、独立提示词、创建/编辑/复位/删除、ZIP 导入导出 |
| `src/pages/Marketplace.tsx` | `ui/frontend/marketplace.rs`、`host_marketplace.rs` 查询、排序、点赞、详情、安装、ZIP、我的发布、设备 OAuth |
| `src/pages/Translation.tsx`、`SelectionAsk.tsx`、`Corrections.tsx` | `ui/frontend/pages.rs` 翻译目标/工作语言、QA 历史开关、纠错建议接受/拒绝 |
| `src/components/SettingsModal.tsx`、`src/pages/settings/` | `ui/settings.rs` 七类设置；`host_settings.rs`、`host_models.rs`、`host_omni.rs` 提交 Core 设置事务 |
| `src/components/Onboarding.tsx` | `host_onboarding.rs` 首次启动、麦克风与桌面组件、进入服务配置 |
| `src/components/Capsule.tsx`、`TypelessCapsule.tsx` | 独立录音胶囊进程，音量、阶段及录音操作由主进程会话驱动 |
| `src/pages/QaPanel.tsx`、`SelectionPolishPreview.tsx` | `popup.rs` 协议和 `main.rs` 浮窗渲染；复用 Core QA/Selection 会话 |
| `src/pages/SelectionVoiceIntentPicker.tsx` | `host_windows.rs` 意图、预览、应用与撤回；`selection_voice.rs` 持有录音及原输入目标 |
| `src/pages/LessComputerPanel.tsx`、`LessComputerGlow.tsx` | 独立原生 viewport，输出、工具事件、审批与取消共享主 Core；状态边框动画 |

侧栏为 226 逻辑像素；设置弹窗上限 960×680，导航栏 214。窄窗口收缩导航及内容区域，页面按可用宽度调整卡片列数。`design_tokens.rs` 与 `ui/theme.rs` 统一深浅主题。`scripts/sync-egui-locales.mjs` 从 React 语言资源生成八种语言目录；Linux UI 偏好单独保存语言、字体缩放和启动引导状态，云同步沿用 Core UI 偏好字段。

## 所有权和原生边界

- 渲染只产生 `FrontendAction`；宿主通过异步队列调用 Core。设置字段合并到最新 revision，原生重绑失败恢复旧配置；Omni 凭据不写入 UI 状态文件。
- 主窗口隐藏或切页不销毁 Core。QA、选区预览、胶囊通过版本化 JSONL 与主进程共享会话，迟到的旧会话操作被拒绝。Less Computer 审批不依附当前页面。
- 桌面桥使用 `org.openless.Desktop1`，协议版本 1，提供热键边沿、前台身份、工作区、定位和恢复。X11 使用 x11rb/XInput2/RandR；GNOME 使用 Shell 扩展；KDE 使用 KWin 脚本和 KGlobalAccel 服务。桌面服务重启后重新注册当前绑定。
- fcitx5 插件在原 IC 中保留选区、周边文本及字符偏移；写入与撤回前检查焦点、目标和完整快照。`ContextSnapshot` 按版本和目标读取应用信息，可禁止读取文本，密码字段不返回文本。
- `context.rs` 注入 `HostContextAdapter` / `EditObservationAdapter`，fcitx5 与 AT-SPI 读取均有大小和时间界限；观察绑定会话代次及目标，迟到捕获不能覆盖新目标。无可用文本接口时保留 Core 的失败/剪贴板回退语义。
- 录音归档沿用 Core WAV/历史格式和保留策略。录音静音保存实际输出设备和原状态，正常停止、取消、错误及释放时恢复。播放只管理自身 `paplay` 子进程。
- AppImage 下载在校验 SHA-256 和仓库固定 minisign 公钥后，以同目录原子替换提交；失败恢复旧 inode，取消在提交开始前生效。deb/rpm 由系统包管理器更新。

接口来源：[GNOME 扩展文档](https://gjs.guide/extensions/)、[KWin 脚本接口](https://develop.kde.org/docs/plasma/kwin/api/)、[AT-SPI Text](https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/iface.Text.html)。GNOME 42–44 与 45+ 分别提供旧模块和 ES module 入口。

## 安装和构建

构建环境：WSL Ubuntu 22.04、Rust 1.94.0、Node.js 22、x86_64。编译需要 ALSA、DBus、OpenSSL、X11/XInput2/RandR、Wayland/xkbcommon、fcitx5 开发包、OpenBLAS、Qt5/KF5GlobalAccel、CMake/Clang。打包另需 fpm 1.16.0、appimagetool、patchelf、rpm、Noto CJK 字体。

```bash
cd openless-all/app
npm ci
npm run build
node ../scripts/sync-egui-locales.mjs
node ../scripts/linux-desktop/gnome/build.mjs
cmake -S ../scripts/linux-fcitx5-plugin -B ../scripts/linux-fcitx5-plugin/build -DCMAKE_BUILD_TYPE=Release
cmake --build ../scripts/linux-fcitx5-plugin/build --parallel
ctest --test-dir ../scripts/linux-fcitx5-plugin/build --output-on-failure
cmake -S ../scripts/linux-desktop/kde -B ../scripts/linux-desktop/kde/build -DCMAKE_BUILD_TYPE=Release
cmake --build ../scripts/linux-desktop/kde/build --parallel
git submodule update --init --depth 1 -- src-tauri/vendor/qwen-asr
make -C src-tauri/vendor/qwen-asr blas CFLAGS_BASE="-Wall -Wextra -O3 -ffast-math -mtune=generic"
cargo check --locked -p openless-linux-egui
cargo test --locked -p openless-core
cargo test --locked -p openless-linux-egui --lib --test host_contract
cargo build --locked --release -p openless-linux-egui
OPENLESS_LINUX_VERSION=2.0.0-Beta.1 APPIMAGE_EXTRACT_AND_RUN=1 bash scripts/package-linux-egui.sh
bash scripts/verify-linux-egui-packages.sh
```

产物位于 `$CARGO_TARGET_DIR/linux-egui-packages`，未设置该变量时为 `app/target/linux-egui-packages`。包含 Linux ELF、deb、rpm、AppImage、桌面集成 tar.gz 及 SHA256SUMS。Qwen 固定子模块为 `b00b789b17051aea61e9717458171100662318a4`，不下载模型作为安装包内容。

```bash
# Ubuntu / Debian：apt 同时安装声明的运行依赖
sudo apt install ./OpenLess-Linux-egui-2.0.0-Beta.1-x86_64.deb
# Fedora / RPM 系统
sudo dnf install ./OpenLess-Linux-egui-2.0.0-Beta.1-x86_64.rpm
# AppImage 仍需要宿主安装并启用 fcitx5，以及 PulseAudio 或 PipeWire-Pulse。
chmod +x OpenLess-Linux-egui-2.0.0-Beta.1-x86_64.AppImage
./OpenLess-Linux-egui-2.0.0-Beta.1-x86_64.AppImage
```

在设置 → 关于与更新中安装/启用/卸载桌面组件；系统包也提供 `openless-desktop-integration install|enable|uninstall`。独立组件包解压后运行 `bash linux-desktop/install.sh install`，在实际桌面用户会话中执行。GNOME 第一次安装通常需注销再登录后启用；更新正在使用的 fcitx5 `.so` 后需重启 fcitx5 或重新登录。KDE helper 及私有 Qt 库复制到用户目录，AppImage 退出后仍可启动。

普通 CI 产物是**未签名的候选包**。SHA256SUMS 校验传输完整性；正式 AppImage 自动更新必须由仓库发布流水线提供 `LINUX_EGUI_MINISIGN_SECRET_KEY` 签名，客户端拒绝未签名包。没有生成或替换上游签名私钥。

## 自动检查记录

执行日期：2026-09-11。以下检查在 WSL Ubuntu 22.04 / Rust 1.94.0 执行，React 构建使用 Windows Node.js 22。

| 检查 | 最终结果 |
| --- | --- |
| `npm run build` | TypeScript + Vite 通过，19.83 秒；没有运行界面测试或浏览器 |
| `cargo test --locked -p openless-core` | 957 项通过，1 项既有忽略 |
| `cargo clippy --locked -p openless-core --all-targets -- -D warnings` | 通过；兼容当前 Clippy 的错误类型、布尔式和测试初始化告警已修复 |
| `cargo test --locked -p openless-linux-egui --lib --test host_contract` | 125 项库测试 + 4 项宿主契约通过 |
| `cargo check --locked -p openless-linux-egui --all-targets` | 全部目标编译通过；不运行 egui 界面测试 |
| fcitx5 CMake / CTest | 编译通过，1 项输入目标契约通过，包括目标失效、焦点恢复与密码字段拒绝 |
| KDE helper / Qwen CMake、Make | Qt5/KF5 helper 和固定 Qwen 子模块编译通过 |
| Core/Linux 依赖、密钥表面、测试隔离、运行时边界、Linux 公共接口 | 全部通过 |
| `cargo build --locked --release -p openless-linux-egui` | 最终 Linux x86_64 Release 通过；egui 宿主仍有 35 条未使用代码告警 |
| `package-linux-egui.sh` / `verify-linux-egui-packages.sh` | deb、rpm、AppImage 和独立组件包生成通过；SHA-256、ELF 依赖、桌面元数据、fcitx5/Qwen/字体/Qt 组件及解包检查通过 |

原生测试覆盖快捷键冲突与恢复、目标和观察结果过期、取消竞态、原输出设备静音恢复（含部分生效后失败）、凭据锁定、设置事务、下载取消、签名/校验、原子更新与失败回滚。测试不调用真实云端发布/删除操作，不替代桌面验收。

本地产物目录为 `openless-all/app/target/linux-egui/linux-egui-packages/`，未提交二进制到 Git。包中保留私有 Qt/KF5 等共享库的许可文件；deb/rpm 的包版本使用 `2.0.0~Beta.1`，确保 Beta 排在正式 `2.0.0` 之前。文件名和应用版本仍为 `2.0.0-Beta.1`。

| 文件 | 字节数 |
| --- | ---: |
| `openless-linux-egui` | 50,904,256 |
| `OpenLess-Linux-egui-2.0.0-Beta.1-x86_64.deb` | 45,802,718 |
| `OpenLess-Linux-egui-2.0.0-Beta.1-x86_64.rpm` | 45,788,946 |
| `OpenLess-Linux-egui-2.0.0-Beta.1-x86_64.AppImage` | 75,007,168 |
| `OpenLess-desktop-integration-2.0.0-Beta.1-x86_64.tar.gz` | 28,358,066 |

本地 AppImage SHA-256：`637ca3d5a45640634ed4316c689a0e0420a937d07227233dbc0582f5fd21328c`。全部文件摘要随包保存在 `SHA256SUMS`。云端独立重建的字节数和摘要以对应 Actions artifact 内的 `SHA256SUMS` 为准。CI 入口为 `.github/workflows/check-linux-egui.yml`，复用打包工作流；云端运行链接及产物链接记录在 PR。

## 人工验收矩阵

全部单元格初始为待验收，不能由 WSL 编译结果替代。

| 环境 | 界面/缩放 | 按下/释放/重绑 | 焦点/选区/撤回 | 音频/静音 | 托盘/安装/更新 |
| --- | --- | --- | --- | --- | --- |
| GNOME 42 / X11 | 待验收 | 待验收 | 待验收 | 待验收 | 待验收 |
| GNOME 42 / Wayland | 待验收 | 待验收 | 待验收 | 待验收 | 待验收 |
| GNOME 46 / X11 | 待验收 | 待验收 | 待验收 | 待验收 | 待验收 |
| GNOME 46 / Wayland | 待验收 | 待验收 | 待验收 | 待验收 | 待验收 |
| Plasma 5.27 / X11 | 待验收 | 待验收 | 待验收 | 待验收 | 待验收 |
| Plasma 5.27 / Wayland | 待验收 | 待验收 | 待验收 | 待验收 | 待验收 |
| Plasma 6 / X11 | 待验收 | 待验收 | 待验收 | 待验收 | 待验收 |
| Plasma 6 / Wayland | 待验收 | 待验收 | 待验收 | 待验收 | 待验收 |

1. 对照锁定 React 源码逐页检查八个主页面、七类设置、渠道/模型/Omni/手机输入/云同步子页；深浅主题、八种语言、字体和窗口缩放；颜色、图标、圆角、阴影、间距、滚动和弹窗状态。
2. 检查按住/单击/自动模式、左右修饰键、组合键、风格直达与全部 Less Computer 热键；与桌面现有快捷键冲突应失败并保留旧绑定；重启应用、fcitx5、桌面组件后再测。Wayland 未启用组件时只能依赖 fcitx5 客户端事件；不算通过全局热键验收。
3. 在 GTK、Qt、浏览器文本框中选择重复文本、切换焦点、关闭目标窗口、修改原选区、取消处理；任何过期目标不得写入新应用。检查 QA、语音意图、预览编辑、应用、撤回及无文本接口时反馈。
4. 用真实麦克风测试开始/停止/取消/异常/退出；录音时切换默认音频输出，确认恢复的是原设备原状态。检查归档保留、播放、导出、重转写及删除。
5. 锁定系统钥匙环，测试保存/连接失败；检查密钥不出现在状态文件和诊断日志；关闭上下文权限后确认不读取文档文本。
6. 切页、隐藏主窗口并触发浮窗；在 Less Computer 等待审批时隐藏再显示窗口，确认审批和取消不丢失。双实例启动应转发到原会话。
7. 测试双屏、负坐标、不同缩放、面板保留区域；浮窗应出现在原目标所在屏幕。重点验收 GNOME 42 的释放事件及 KDE 修饰键表示差异。
8. 分别安装/卸载 deb、rpm、AppImage 桌面组件；检查 autostart、托盘环境能力及通知。签名发布环境验证中断下载、签名错误、空间不足、原子安装和失败恢复；未签名候选包不能计为签名发布通过。

## 贡献归属

选择性复用了 [PR #1055](https://github.com/Open-Less/openless/pull/1055) 的设计 tokens（sim，`b87ca21764f6022b5ca493b0f53f5939f75c7aca`），以及 [PR #1060](https://github.com/Open-Less/openless/pull/1060) 的 egui 页面、浮窗协议、音频/托盘/更新等基础（aeoform，`0303488e863c8641f69084600f830c3f40f43b0f`）。本分支按锁定 beta 重新接入 Core、保留基线 Qwen 能力，并补充桌面桥、上下文观察、真实页面操作、打包与检查。提交保留对应 Co-authored-by。
