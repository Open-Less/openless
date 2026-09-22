# Linux egui 依赖升级计划（含 Rust 工具链与 Vulkan 评估）

**状态：已评估、**暂缓执行**（2026-09-22 与用户确认：先做别的活，升级后续排期）。**

本文把"升级到最新依赖"的可行范围、必须改的点、真实工作量与风险一次写清，之后照单执行即可，
不需要重新调研。

---

## 一、现在用的是什么（图形栈）

| 层 | crate | 当前锁定 | crates.io 最新 stable | 作用 |
|---|---|---|---|---|
| 应用框架 | `eframe` | **0.33.3**（`linux-egui/Cargo.toml` 里 pin `=0.33.3`） | 0.36.2 | 窗口循环 + 选择渲染后端（我们开 `glow` feature） |
| UI | `egui` | **0.33.3**（pin） | 0.36.2 | 立即模式 UI |
| 窗口/输入桥 | `egui-winit` | 0.33.3 | 0.36.2 | egui ↔ winit 事件转换 |
| OpenGL 渲染器 | `egui_glow` | 0.33 | 0.36.2 | egui 的 GL 绘制后端 |
| **GL 绑定** | `glow` | **0.16** | 0.18（eframe 0.36.2 要求 `^0.17`） | GL/GLES 安全绑定；`siri_gl.rs` 直接用 |
| GL 上下文 | `glutin` | 0.32.3 | 0.32.3 ✅ 最新 | Wayland/EGL 上创建 GL context |
| 窗口 | `winit` | 0.30.13 | 0.30.13 ✅ 最新稳定（0.31.0-beta.3 为预发布） | 窗口创建 |
| Wayland | `wayland-client` 0.31.15 · `wayland-protocols-wlr` 0.3.12 | — | 均最新 | 胶囊 wlr-layer-shell |
| X11 | `x11rb` 0.13.2 · `raw-window-handle` 0.6.2 | — | 0.14.0 可升 / 0.6.2 最新 | 胶囊 X11 overlay、句柄互操作 |

**`eframe 0.36.2` 的依赖约束（实测 crates.io）**：`winit ^0.30.13`、`egui ^0.36.2`、
`egui_glow ^0.36.2`（可选）、`glow ^0.17.0`（可选）、`glutin ^0.32.3`、`wgpu ^30`（可选）。
→ 升 eframe **拿不到 winit 0.31**，更换不到 `glow 0.18`。

## 二、其余依赖：谁已最新、谁能升

| 分类 | 已是最新 ✅ | 可升级 ⬆（含 breaking） |
|---|---|---|
| 音频/桌面集成 | `cpal` 0.18.2、`arboard` 3.6.1、`dbus` 0.9.12 | `rfd` 0.16→0.17.2、`keyring` 3.6.3→4.2.0 |
| 网络/远端输入 | `tokio` 1.53.1、`hyper-util` 0.1.20、`tokio-rustls` 0.26.5、`ring` 0.17.14、`local-ip-address` 0.6.13 | `reqwest` 0.12.28→0.13.5、`axum` 0.7.9→0.8.9、`rcgen` 0.13.2→0.14.10、`x509-parser` 0.16→0.18.1、`webpki-roots` 0.26.11→1.0.9、`rustls` 0.23.44→0.23.45 |
| 基础库 | `serde`、`serde_json`、`chrono`、`uuid`、`image`、`libc`、`semver`、`log`、`simplelog`、`fs2`、`futures-util`、`time`、`tempfile`、`url`、`minisign-verify` | `sha2` 0.10.9→0.11.0、`base64` 0.22.1→0.23.1 |

（数据来源：`Cargo.lock` + crates.io API，2026-09-22 抓取。）

## 三、分批计划

### 批 0：Rust 工具链 1.95.0 → 1.98.1（≈ 0.5 天）

- 本机：`rustup update stable`（当前 1.95.0，最新 stable **1.98.1**，2026-09-01）。
- 声明：三处 `rust-version = "1.88"` → `"1.98"`：`app/crates/openless-core/Cargo.toml`、
  `app/linux-egui/Cargo.toml`、`app/src-tauri/Cargo.toml`。
- CI：`app/../.github/workflows/ci.yml` 的 **MSRV 门禁**显式钉了 `dtolnay/rust-toolchain@1.88.0`
  → 改成 `@1.98.0`；其余 job 都是 `@stable`（自动跟随），**不用改**。
- **不做** edition 2021 → 2024：`unsafe_op_in_unsafe_fn`、`gen` 关键字、RPIT 生命周期捕获、
  `static mut` 引用、`unsafe extern` 等要改，我们的 FFI/unsafe（glow GL、X11、Wayland、DBus）
  很多 → 0.5–1.5 天且纯风险、与性能无关，要单独一轮。
- 注意：升级 Rust **不会带来可感知的性能提升**，买的是安全修复 + 新 lint + 未来依赖可用性。

### 批 1：semver 内的 patch 升级（极低风险，当天）

`cargo update` + `rustls` 0.23.45；跑全量 `cargo test --locked` + 发布契约 + i18n `--check`。

### 批 2：中等风险的 breaking 升级（逐个模块、逐个回归）

`reqwest 0.13` / `axum 0.8`（路由 `/*path` → `/{*path}`）/ `rcgen 0.14` + `x509-parser 0.18`
（TLS 身份生成）/ `sha2 0.11` / `base64 0.23` / `webpki-roots 1.0` / `rfd 0.17` /
`keyring 4.2`（凭据）/ `x11rb 0.14`（胶囊 overlay）。改动点分散，可一批一验证。

### 批 3：egui 家族 0.33.3 → 0.36.2 + `glow` 0.16 → 0.17（大工程，≈ 2–3 天）

- 放开 `=0.33.3` 的 pin；`glow` 行必须同步改到 0.17（否则仓库里出现两份 glow，
  且 `siri_gl.rs` 会链到旧的那份）。
- **必须逐条复测 §五 回归清单**（都是我们历史上踩过的坑）。

### 批 4（可选）：切 Vulkan / wgpu（≈ 4–6 天 + 回归）

见下节。

## 四、Vulkan(wgpu) 移植评估

### 要改的真实体量（已按代码实测，不是拍脑袋）

| 文件 | 行数 | 内容 |
|---|---|---|
| `app/linux-egui/src/ui/frontend/siri_gl.rs` | **1090** | 唯一自写 GPU 的地方：4 个着色器（vertex + `SIRI_WAVE_FRAGMENT_SRC` / `SIRI_ORB_FRAGMENT_SRC` / `SIRI_RING_FRAGMENT_SRC`，从 `SiriGL.tsx` 移植的 GLSL）+ glow 程序/VAO/uniform/缓冲 + `egui_glow::CallbackFn` |
| `app/linux-egui/src/popup_layer.rs` | **919** | 胶囊 wlr-layer-shell surface 自建 GL 上下文（`glow::Context::from_loader_function` + `egui_glow::Painter`）+ X11 overlay 路径 |
| 其它 | — | **零** `glow::` 引用（全 crate 只有上面 2 个文件碰 GPU）：纯 egui 抽象，迁移不用动 |

工作拆解：GLSL→WGSL（4 个 shader）+ `RenderPipeline`/`BindGroup`/`UniformBuffer` +
`egui_wgpu::CallbackTrait`（2–3 天）；layer-shell surface 从 glutin/EGL+egui_glow 换成
wgpu surface（`raw_window_handle` → `wgpu::SurfaceTarget`，X11 overlay 同改）（1.5–2.5 天）；
测试与契约（in-crate 的 `siri_gl::gpu_state_guard` 等）（0.5–1 天）；打包加 `libvulkan1`
依赖与无 Vulkan 时的行为（0.5–1 天）；全量真机回归（1.5–2 天）。
**合计 ≈ 9–14 人天（含 §三 的批 0–3）。**

### ⚠️ 双后端不划算

`siri_gl.rs` / `popup_layer.rs` 是**直接写 glow API** 的，保留 glow 作为回退 = 维护两套 GPU
实现 → 第 3、4 项翻倍，总 **13–20 人天**。要切就一次切干净（代价：deb 多一个 `libvulkan1`）。

### 收益判断（切之前先看数据）

唯一的真 GPU 负载就是 Siri 光效那 3 个 fragment shader；egui 的 2D UI 只是少量纹理四边形。
**这类负载下 Vulkan 相对 OpenGL 的收益通常可以忽略**，真实卡顿更可能来自帧调度 / 字体栅格化 /
纹理上传。→ **先做 profiling**（现有 glow 版测 Siri 光效每帧耗时与掉帧点），有数据再决定；
若决定切，先做**最小验证**（只把 Siri 光效改成 wgpu 跑在 egui 0.36 上，≈ 1 天）再全量铺开。

### MSRV（不拦）

`wgpu 30` = 1.87 · `egui-wgpu 0.36.2` = 1.95 · `naga 26` = 1.82 · `egui/eframe 0.36.2` = **1.95**；
本机 rustc 1.95.0 已满足，批 0 升到 1.98.1 更宽松。

## 五、回归清单（egui 升级必查，历史踩坑）

1. **标题栏拖动/聚焦门槛**：`egui-winit-0.33.3/src/lib.rs:1403-1409` 对 `StartDrag` 有
   `window.has_focus()` 前置；我们在 `ui/frontend/layout.rs` 用「同帧补发 `Focus`」绕过
   （提交 `8379b288`）。升级后若上游已修 → 删掉绕过逻辑。
2. **弹层图层顺序**：设置弹窗沿用 `Order::Foreground`（ComboBox 弹出层也是 Foreground，
   `egui-0.33.3/src/containers/popup.rs:150`）；**遮罩 / 点击拦截 / 卡片必须同一个 `Area`**
   （`area.rs:549`，Area 被按下会 `move_to_top`）；resize 条带 `Order::Tooltip`。
   → 复测设置页、市场详情弹窗、风格编辑器弹窗、历史确认框（提交 `8f0da28d`、`4119369f`、`40b2c5f8`）。
3. **ComboBox / Popup API**：0.32+ 有改版史 → 逐个迁移 + 复测所有下拉。
4. **字体注册**：`NotoSansCJK-Medium.ttc|2|100` 注册为 `openless-medium`（`e8633948`，走
   fontconfig 索引）→ 复测字重。
5. **winit 变更**：`WindowEvent`/`ViewportCommand` 语义；Wayland 下 `set_visible` 仍是空实现
   （我们靠「窗口进程退出」实现隐藏）→ 复测主窗显隐与三弹窗。
6. **`siri_gl.rs` 的 glow API**：glow 0.16 → 0.17 的 breaking 面（Context/Buffer/VertexArray/
   draw 调用）。
7. **门禁**：`cargo fmt` · `cargo test --locked`（含 `localization_contract`）· 5 个 node 契约
   （`linux-egui-tauri-free-contract` / `linux-egui-release-contract` / `tauri-linux-free-contract` /
   `workflow-concurrency-contract` / `selection-polish-panel-contract`）· `sync-egui-i18n.mjs --check`。

## 六、不在本计划内 / 已知限制

- **触摸屏拖窗**：修在 winit `#4683`（已合入 master、未发版）；`eframe 0.36.2` 仍依赖
  `winit ^0.30.13` → **升 eframe 也拿不到**，要等 winit 0.31 正式版 + eframe 跟进。
  用户已否决 winit 补丁方案。
- **胶囊避让 KDE 面板**：维持 wlr-layer-shell 官方做法（`Anchor::Bottom` +
  `exclusive_zone(-1)` + `margin.bottom = 12`）；探测方案已回退（`b0219050`），不再尝试。
- **XWayland 兜底**：用户否决（不允许拿 XWayland 当 Wayland 兜底）。
- 文档与时序：crates.io 版本/MSRV 数据抓取于 2026-09-22，执行前重新核对一次最新版。
