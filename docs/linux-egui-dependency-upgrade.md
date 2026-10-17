# Linux egui 依赖升级与 Vulkan 渲染

更新：2026-09-27。以 `linux-egui/Cargo.toml`、Cargo.lock 和当前源码为准；旧 Pi 记录和此前暂缓计划只作为线索。

## 已完成

- eframe / egui / egui-winit / egui-wgpu 从 0.33.3 升至 0.36.2；wgpu 从 27 升至 30.0.1；Glow 绑定随之升至 0.17。
- Linux 主窗口和普通 eframe 弹窗显式选择 `Renderer::Wgpu`，并把 wgpu 实例可用后端限定为 Vulkan。
- Siri 波形、圆点和选区助手环改用 egui 原生图形绘制，避免 Glow shader callback 在 WGPU 窗口中被忽略后退化成普通音量条。
- 0.36 破坏性 API 已适配：`App::ui`、`Context::run_ui`、`CentralPanel::show(&mut Ui)`、文本编辑框 frame、UI 样式按主题设置，以及 Glow layer-shell 纹理增量可变传递。
- Cargo.lock 已解析升级 62 个兼容依赖；包括 eframe/egui 图形栈、accesskit、wgpu/Naga 等。其余直接依赖未为追新而跨主版本改动。
- Wayland wlr-layer-shell 胶囊面已从 `glutin`/EGL + `egui_glow::Painter` 迁到 wgpu/Vulkan（`popup_layer.rs` 自建 surface、presentation 与帧同步），OpenGL 渲染路径整体移除；`glutin`、`egui_glow` 及其 EGL 依赖不再出现在二进制里。
- 包依赖随之从 `libegl1` / `libwayland-egl1`（rpm: `libglvnd-egl` / `libwayland-egl.so.1`）换成 `libvulkan1`（rpm: `vulkan-loader`），并 `Recommends: mesa-vulkan-drivers`。契约测试 `linux-egui-release-contract.test.mjs` 会同时断言 Vulkan loader 在依赖里、EGL 依赖不再回来。

## 渲染路径：全部 Vulkan

主窗、普通弹窗、layer-shell 胶囊现在共用 `src/wgpu_device.rs::device_descriptor()`：Vulkan-only 后端、`max_texture_dimension_2d = 8192`、`MemoryHints::MemoryUsage`。

最后一项是必须的：wgpu 28 起 `MemoryHints` 默认 `Performance`，wgpu-hal 会按 device 128–256 MB 的内存块向驱动申请，每个渲染进程因此固定多占约 180 MiB（宿主、主窗、每个弹窗各一份）。本机 GTX 1660 SUPER 实测：胶囊 460×180 从 198 MiB 降到 18 MiB，开设置面板的主窗从 209 MiB 降到 47 MiB，帧率不变（29.8 fps / 72.7 fps）。

## 当前验证与限制

- 已运行 `cargo fmt --manifest-path openless-all/app/Cargo.toml --package openless-linux-egui`。
- 已运行 `cargo check --manifest-path openless-all/app/Cargo.toml -p openless-linux-egui`，在 Rust 1.95.0 下通过。Cargo 缓存和 build 输出位于 `/tmp`。
- 本环境 `vulkaninfo --summary` 只枚举出 llvmpipe CPU Vulkan 设备；没有真实 GPU，因此不能据此证明硬件 Vulkan 性能、Wayland layer-shell 和发行版驱动表现。
- 未运行测试或完整安装包构建。egui 0.36 API 变更可能影响现有 GUI 测试源码，运行验证前需单独完成测试 API 迁移。
- eframe 仍依赖 winit 0.30；此版本升级不会解决 Wayland 触摸屏拖动窗口所需 serial 的缺口。

## 后续设备验收

1. 在带 Vulkan 硬件驱动的 Wayland 与 X11 主机启动主窗及三个弹窗，确认日志枚举到硬件 Vulkan adapter。
2. 检查 Siri 录音波形、处理动画和 QA 录音环在透明 surface 上的颜色、裁剪、帧率与显存/CPU 占用。
3. 检查 LayerShell 胶囊的位置、透明背景、焦点与音频动画，确认 Vulkan surface 换帧与 `PreMultiplied` alpha 正常。
4. 在无 Vulkan loader 或 adapter 时确认故障提示清晰。目前选择的是 Vulkan-only，启动时没有自动回退 GL。
