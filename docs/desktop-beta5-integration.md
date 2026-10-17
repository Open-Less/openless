# 桌面 beta.5 整合与本地验收

> 本文记录最初整合与当时安装包。后续用户反馈发现的首录音阻塞、交互问题及本轮工作树修复见 [回归修复记录](beta5-regression-repair.md)；下列旧安装包与哈希不包含这些后续修复。

- 版本：`2.0.0-beta-a.beta.5+build.20261015-bd`（按请求保留完整大小写与后缀）。
- 合并分支：`integration/desktop-beta-a.beta.5-20261015-bd`；PR 目标：`beta`。
- 基线：`origin/beta` 的 `0454dc56`。
- 初次产品代码检查点：`5637abee8c92d1dbff6afecfdd0877771bcb8223`；后续 CI 修复限定于桌面模块编译边界、测试布局/时序及跨平台截图，没有修改三模型推理实现。
- 最终产品代码与安装包检查点：`edff8913c48afa79609cbb29eacc47a00620e89d`；本节下方校验值来自该检查点重新构建的 Release 包。
- 验收环境：2026-10-15，Apple Silicon / macOS 27.0.1。

## 合并来源与边界

| 来源 | 整合内容 |
| --- | --- |
| 当前三模型工作区 | macOS 旧引擎下架、定向清理、延迟加载、TextStream / SenseVoice / Embedding Gemma 独立启停与真实推理 |
| `appergb/合并两个分支` 的未提交桌面快照 | 首帧与启动视频移除、按需加载、扁平设置、字体与提示组件、快捷键命令列表、渠道弹簧重排、卡片删除与 WebGL 生命周期 |
| `origin/fix/macos-input-switch-crash`，PR #1142 | 输入源相关 NSEvent 转换转交主线程，避免切换输入法时在事件监听线程调用 AppKit |
| `origin/fix/linux-egui-ui`，PR #1136 | Linux 设置、QA 面板、窗口代次与迟到响应保护 |
| `origin/feature/testing-features-gate`，PR #1141 | Less Computer / Claude Console 的设备本地 Beta opt-in，撤回后的入口、热键与窗口可见性限制 |

桌面 UI 未提交快照通过独立临时 index 生成提交，再三方合并，不直接覆盖已完成的三模型实现。旧的两模型实验实现没有被重新引入。原始 UI 工作区和原始实验模型工作区的 tracked diff SHA-256 在整合前后相同。

没有合入第三方尚未审核的 PR、未提交 iOS 目录或 Android 专属功能。相对当前 `origin/beta` 无 Android / iOS 源码改动；Android workflow 仅移除随旧 macOS 模型依赖退役而失效的处理步骤。根目录原有的未跟踪 `package.json`、`pnpm-lock.yaml` 未提交。

### 冲突处理保留的语义

- Mac 的旧模型入口仍禁用，提示为「该功能已下架，请去实验性与扩展里面」；新三模型入口仍在“实验与扩展”首项。
- 三模型各自 opt-in；通用 Beta 开关只控制其原本负责的 Less Computer / Claude Console，不成为下载或加载模型的隐式授权。
- 已访问的高级页面保留草稿；未访问页面不挂载；撤回 Beta 授权后相关测试页面卸载。
- 新 UI 的紧凑模型行保留三模型、独立开关、内存/规格提示、实时 SSE 地址、进度、错误、重试、删除及渠道选择引导。说明通过支持鼠标、键盘、触屏的共享提示展示。
- 退休入口与整行提示使用非按钮外层触发器，避免 Base UI 提示按钮包住已有按钮造成无效嵌套。
- Windows Less Computer 的创建仍离开同步 IPC 线程；显示与 opt-in 复查在 UI 线程串行，结果回传调用方，不把“任务已排队”当作“窗口已成功打开”。

## 本地测试结果

| 检查 | 最终结果 |
| --- | --- |
| Core 完整测试 | 1,399 通过（1,202 单元 + 197 集成），0 失败，1 个需要固定 Codex CLI 的无关测试忽略 |
| Tauri 原生默认测试 | 533 通过，0 失败，4 个按需模型/安装用例默认忽略；真实模型另行验收 |
| 独立后端测试 | 40 通过 |
| `npm test` | 生产前端构建及 121 个前端/契约测试文件通过 |
| Chromium / WebKit 全量 UI | 84 通过，无重试；包含浅/深色截图、Beta opt-in 撤回、三模型提示、重排、取消、删除与撤销、窄屏和缩放 |
| Python worker 测试 | 7 通过 |
| 指定版本 debug `.app` 真实模型验收 | 通过，退出码 0，不带 `--download` |
| Release `.app` / DMG | 构建及签名、资源、版本、DMG 校验通过 |
| 变更组件 Prettier / `git diff --check` | 通过 |

Ubuntu CI 执行完整交互、几何、生命周期和像素比较，不跳过视觉用例。针对系统字体栅格化差异，从首轮 CI 的实际产物逐张检查并增加了八张 Linux / 浏览器独立基线，不复用 macOS 像素，也不提高容差。macOS / Windows 模型入口测试显式声明产品 OS，不依赖 CI 物理平台。

合并后的初次 UI 回归暴露了旧版断言与新边界的冲突，已修复并重跑完整套件：退休模型入口不再作为可点击选项；导航按钮的统计包含合法外层提示；并行模型下载的测试等待三个完成项再确认；拖动用例先滚动到目标并验证真实位移再松手。仅更新了经截图检查确认的四张快捷键基线，变化为 Beta 未授权时隐藏 Less Computer 后的计数/行位移；没有提高截图容差，字体截图保持原样，最后一轮不带更新快照参数通过。

### 真实模型与生命周期

使用既有隔离模型缓存与 16 kHz 单声道固定英文 WAV，没有补下载、麦克风调用或当前安装应用凭据库访问。三模型均真实输出了包含 `hello`、`test`、`system` 的识别文本，并验证：

1. 冷构造、设置/状态查询不初始化推理。
2. 安装/启用后无已加载模型；已启用且已选中的重启恢复也不加载推理。
3. 实际会话加载并识别；TextStream SSE 最终文本与原生返回一致，终态无未提交草稿。
4. 公共渠道停用释放推理；取消会话不提交草稿。
5. 关闭实验保留模型文件，停用后重启不能获得推理租约。
6. 临时旧模型升级清理定向且幂等，实验包与无关文件保留。
7. 结束后无残留 TextStream worker 或 TCP 7890 监听。

主要日志与机器可读结果：`/tmp/openless-desktop-beta5-20261015-bd/` 下的 `core.log`、`final-native-tests.log`、`final-frontend.log`、`backend.log`、`final-e2e.log`、`native-acceptance.log`、`final-release-package.log`、`final-artifacts.log`、`verified.json`。该目录可能被系统清理，本文保留主要结果。

## 首轮 CI 发现与修复

首轮 macOS checks 与 Rust 1.88 MSRV 已通过。Windows 的版本契约在 CRLF checkout 上未匹配 Cargo.lock，已统一文本换行；Android 发现 Foundry 桌面命令模块被错误编译进移动端，已像其余本地 ASR 模块一样以 `not(mobile)` 限定声明和导出，没有添加移动端空壳。Linux Core / egui 测试已通过，但运行时边界脚本未识别通过 `#[path]` 命名的独立测试模块；现改为标准 `experimental_asr/tests.rs` 测试布局，保留 TaskSpawner 门禁原有强度。

UI 的两个 Linux WebKit 时序问题改为测量确定的动画时间轴及浏览器真实滚动停稳，而非假设 CI 必须在固定毫秒内提供足够帧数。文档提交后的复核又发现旧探针可能漏掉短动画，且会暂停、回拨应用自己的动画；现于动画创建时捕获，先验证其自然结束和图层释放，再用独立副本测量实际 DOM 中间状态，不再干预业务动画的时钟/finished 生命周期。该探针在本机双浏览器连续 20 次通过，业务代码与安装包未改。八张 Linux 基线来自 [首轮 CI](https://github.com/Open-Less/openless/actions/runs/38675183147) 的实际渲染，已检查字体、表单、计数、布局和深浅主题。

最终产品代码 `edff8913` 的 [完整 CI](https://github.com/Open-Less/openless/actions/runs/38677199261) 已全部通过：Windows checks、macOS checks、macOS Rust 1.88 MSRV、Linux Core/egui 与 deb/rpm 检查、Android cargo check、Chromium/WebKit UI，以及改动范围检查。它们是 GitHub runner 的编译/自动测试证据，不等同于真实设备、安装升级或公开公证分发验收。

## 本地安装包

相对仓库根目录：

- 应用：`openless-all/app/src-tauri/target/release/bundle/macos/OpenLess.app`
- 安装镜像：`openless-all/app/src-tauri/target/release/bundle/dmg/OpenLess_2.0.0-beta-a.beta.5+build.20261015-bd_aarch64.dmg`

构建命令在 `openless-all/app` 执行，显式使用 `INSTALL=0`，未覆盖 `/Applications/OpenLess.app` 或重置现有权限：

```sh
APPLE_SIGNING_IDENTITY=- INSTALL=0 CARGO_BUILD_JOBS=2 ./scripts/build-mac.sh
```

检查结果：

- Release Info.plist 的 `CFBundleShortVersionString`、`CFBundleVersion` 均精确等于指定版本；npm、Tauri、Rust 与两个 lockfile 由版本契约共同核对。
- Info.plist / Mach-O 最低系统版本均为 macOS 14.0；Gemma 自身仍要求 macOS 15+。
- `codesign --verify --deep --strict` 通过，含麦克风 entitlement / 说明，不包含旧系统语音识别权限声明。
- 主程序不链接旧 MLX、Whisper、Speech.framework；bundle 无旧 Metal 资源或模型 `.runtime`。
- 旧 worker 参数仅返回下架信息。
- DMG 的 CRC、Retina 背景、图标、窗口布局和 Applications 链接验证通过。

Release 主程序：74,479,344 字节，SHA-256：

```text
d73474fbc663d072b9810d58a5e7a4f1d760a3465ce9d048f696ad28564f1a5b
```

DMG：28,887,027 字节，SHA-256：

```text
765bf5bb13b3e5e2e408d4f564b660f14764474e8d73cbf7abcc13b70adbb762
```

debug 验收主程序 SHA-256：

```text
2014860f896169c6a402e52b924b41a3d9f49e9b2f571617ba6ebeb469674930
```

## 未宣称通过的部分

- 本地包为 **ad-hoc 签名，未做 Apple 公证**，不等同于已获 Gatekeeper 认可的公开发行包。
- 真实推理来自指定版本 debug `.app` 的隔离验收入口；Release 完成构建和产物验证，但没有用真实用户配置启动正式 UI 或进行真实麦克风/其他应用输入验收。
- 本机没有 Windows 环境；Windows 编译、后端自动测试和 MSRV 已由 CI 通过，仍未进行真实 Windows 设备上的麦克风、SDK 推理或目标应用输入验收。
- Linux host 依赖仅在 Linux target 中声明，直接在 macOS 上运行其测试会因 CPAL / Wayland 等缺失而失败；Docker daemon 未运行。Linux 原生构建、Core/egui 自动测试和 deb/rpm 检查已由 CI 通过，不记为本机或 Linux 桌面实机验收。
- 已有 FFI 声明、弃用和 window-vibrancy 链接警告仍存在，不把消除这些无关警告当成本次发布的完成条件。
