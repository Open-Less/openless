# Linux 渲染引擎选型（2026-11 实测）

## 结论

| 方案 | 引擎 | 能否和 Windows/macOS 视觉一致 | 本机可用 |
|---|---|---|---|
| Tauri（2.x，Linux） | **WebKitGTK** | ✗ 与我们现在这套完全相同 | 是 |
| Tauri 3.0 | — | — | **不存在**（最新 2.x，仓库锁 2.12.4） |
| GTK + WebKitGTK（现状） | WebKitGTK 2.54 | ✗ | 是 |
| **Qt6 WebEngine** | **Chromium** | ✓ | **是**（spike 已通过，见下） |
| CEF | Chromium | ✓ | 可以，但要 vendoring ~250MB |
| Electron | Chromium | ✓ | 可以，但 layer-shell 无解、~200MB |

**关键事实**：Tauri 在 Linux 上的 webview 后端就是 WebKitGTK
（`wry 0.55.1` 的 Linux 依赖是 `webkit2gtk`）。所以「换 Tauri」不会改变渲染，
它只是换了一层 Rust 外壳 —— 我们现在用的正是同一套 GTK + WebKitGTK。

## Qt6 WebEngine spike（已通过）

- 系统里 **运行时已装**：`libqt6webenginewidgets6` / `libqt6webenginecore6` /
  `libqt6webenginequick6` = 6.11.2+dfsg-4；`fcitx5-frontend-qt6` 5.1.16 也在。
  （只差 `qt6-base-dev` / `qt6-webengine-dev` 这两个头文件包。）
- 头文件用 `apt-get download` + `dpkg-deb -x` 解到
  `/data-win/openless-cache/qt6-prefix`，不装系统包也能编译。
- 60 行 `spike.cpp`（QApplication + QWebEngineView）加载我们的
  `linux-web/dist`：日志 `title=OpenLess`、`loadFinished ok=1`，
  前端 JS 正常执行（桥报「GTK 宿主桥尚未就绪」是预期的，Qt 宿主要用
  `QWebChannel` 那条）。

### 迁移代价（相对现在的 GTK 宿主）

1. 窗口层从 Rust(gtk-rs) 换成 **C++/Qt**：QWebEngineView + QWebChannel。
2. Rust 侧 `dispatch()` 保留不动，改为**本地 IPC 服务**（现有
   `$XDG_RUNTIME_DIR/openless-web.sock` 正好复用），C++ 宿主转发命令。
3. 前端桥：`gtkBridge.ts` 那套 `window.webkit.messageHandlers` 换成
   `QWebChannel`（`qrc:///qtwebchannel/qwebchannel.js`），命令面 135 条不受影响。
4. 胶囊：Qt 侧用 `LayerShellQt`（`liblayershellqtinterface6` 本机已装），
   或继续用独立 GTK 进程画胶囊。
5. 打包：依赖加 `libqt6webenginecore6`、`libqt6webenginewidgets6`、
   `fcitx5-frontend-qt6`（约 +120MB 安装体积）。

### 未验证项

- Wayland 下 Qt WebEngine 的 IME 提交（fcitx5-frontend-qt6 已装，但没实测输入法落字）。
- 透明/layer-shell 胶囊在 Qt 下的表现。
- Chromium 在 Qt 里的首帧时间（WebKit 现在是约 1.5s）。
