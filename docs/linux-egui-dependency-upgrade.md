# Linux egui 依赖升级与 Vulkan 渲染

更新：2026-09-24。以 `linux-egui/Cargo.toml`、Cargo.lock 和当前源码为准；旧 Pi 记录和此前暂缓计划只作为线索。

## 已完成

- eframe / egui / egui-winit / egui-wgpu 从 0.33.3 升至 0.36.2；wgpu 从 27 升至 30.0.1；Glow 绑定随之升至 0.17。
- Linux 主窗口和普通 eframe 弹窗显式选择 `Renderer::Wgpu`，并把 wgpu 实例可用后端限定为 Vulkan。
- Siri 波形、圆点和选区助手环改用 egui 原生图形绘制，避免 Glow shader callback 在 WGPU 窗口中被忽略后退化成普通音量条。
- 0.36 破坏性 API 已适配：`App::ui`、`Context::run_ui`、`CentralPanel::show(&mut Ui)`、文本编辑框 frame、UI 样式按主题设置，以及 Glow layer-shell 纹理增量可变传递。
- Cargo.lock 已解析升级 62 个兼容依赖；包括 eframe/egui 图形栈、accesskit、wgpu/Naga 等。其余直接依赖未为追新而跨主版本改动。

## 仍存在的 GL 路径

自建的 Wayland wlr-layer-shell 胶囊面仍通过 `glutin`/EGL 和 `egui_glow::Painter` 提交 epaint 图元。这是独立于 eframe 主窗/普通弹窗的 surface 实现；Siri 效果本身已不再依赖 GL shader。X11 overlay 也保留现有窗口管理器路径。

若要让 layer-shell surface 也由 WGPU/Vulkan 提交，需要将 `popup_layer.rs` 的手工 Wayland surface、presentation 和帧同步整体改成 wgpu surface；这不是 egui 升级的 API 适配，需另行实现与设备验证。

## 当前验证与限制

- 已运行 `cargo fmt --manifest-path openless-all/app/Cargo.toml --package openless-linux-egui`。
- 已运行 `cargo check --manifest-path openless-all/app/Cargo.toml -p openless-linux-egui`，在 Rust 1.95.0 下通过。Cargo 缓存和 build 输出位于 `/tmp`。
- 本环境 `vulkaninfo --summary` 只枚举出 llvmpipe CPU Vulkan 设备；没有真实 GPU，因此不能据此证明硬件 Vulkan 性能、Wayland layer-shell 和发行版驱动表现。
- 未运行测试或完整安装包构建。egui 0.36 API 变更可能影响现有 GUI 测试源码，运行验证前需单独完成测试 API 迁移。
- eframe 仍依赖 winit 0.30；此版本升级不会解决 Wayland 触摸屏拖动窗口所需 serial 的缺口。

## 后续设备验收

1. 在带 Vulkan 硬件驱动的 Wayland 与 X11 主机启动主窗及三个弹窗，确认日志枚举到硬件 Vulkan adapter。
2. 检查 Siri 录音波形、处理动画和 QA 录音环在透明 surface 上的颜色、裁剪、帧率与显存/CPU 占用。
3. 检查 LayerShell 胶囊的 EGL surface、窗口位置、焦点与音频动画；当前这条 surface 仍走 GL。
4. 在无 Vulkan loader 或 adapter 时确认故障提示清晰。目前选择的是 Vulkan-only，启动时没有自动回退 GL。
