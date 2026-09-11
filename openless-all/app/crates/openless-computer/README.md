# OpenLess Computer 原生 helper

`openless-computer` 是封装的 PI 后端随安装包分发的本地程序。它使用
[Enigo 0.6.1](https://docs.rs/enigo/0.6.1/enigo/) 注入键盘和鼠标事件，使用
[XCap 0.9.8](https://github.com/nashaofu/xcap/tree/v0.9.8) 截图。
helper 不执行 shell 命令、不读取或修改剪贴板、不下载其他程序。

## 启动和传输

PI 设置 `OPENLESS_COMPUTER_BIN` 为安装包中 helper 的绝对路径，以无 shell
子进程启动，将单个 **UTF-8 JSON 对象写入 stdin 后关闭 stdin**。一次调用只处理
一个请求，输出一行 JSON 到 stdout 后退出。日志不应写入 stdout。父进程应设置
超时、输出上限并处理取消；不要以 `detached` 模式启动。

手动检查可使用 `openless-computer --capabilities`；也支持
`openless-computer --request '{"action":"capabilities"}'`。
实际文本和键盘输入应走 stdin，避免写入进程命令行。

成功（退出码 `0`）：

```json
{"ok":true,"data":{"action":"key"}}
```

失败（退出码 `1` 为原生操作失败，`2` 为参数或 JSON 不合法）：

```json
{"ok":false,"error":{"code":"invalid_request","message":"clicks must be 1 or 2"}}
```

调用方应在非零退出时仍解析 stdout 错误 JSON。常见原生错误码为
`permission_denied`、`unsupported_session`、`no_display`、`monitor_not_found`、
`display_error`、`capture_error`、`input_unavailable` 和 `input_error`。
错误后先确认桌面状态；输入操作可能已部分生效，不应自动重复执行。

## 请求协议 v1

所有对象拒绝未知字段，最大请求长度为 128 KiB。下表中的 `?` 表示可省略。

| action | 其余字段 | 行为 |
| --- | --- | --- |
| `capabilities` | 无 | 无副作用的能力、会话限制和权限预检 |
| `displays` | 无 | 列出当前所有显示器 |
| `screenshot` | `monitor_id?: uint32` | 捕获指定显示器，默认主显示器 |
| `move` | `x: int32, y: int32, monitor_id?: uint32` | 移动鼠标到截图坐标 |
| `click` | `x: int32, y: int32, monitor_id?: uint32, button?: "left"/"right"/"middle", clicks?: 1/2` | 移动后点击，默认左键单击 |
| `scroll` | `amount: int32, axis?: "vertical"/"horizontal"` | 在当前鼠标位置滚动；默认纵向 |
| `key` | `key: string, modifiers?: string[]` | 按下并释放一个键，可同时持有修饰键 |
| `type_text` | `text: string` | 使用 Unicode 输入文本，支持中文和英文 |

`amount` 必须在 `-100..100` 内且不能为零，正值向下／向右，负值向上／向左；
单位是系统滚轮刻度，不是截图像素。

`text` 必须为 1 至 32768 个 UTF-8 字节，不能包含 NUL；原生 Unicode 输入通常
不依赖当前中文输入法。目标应用自行决定文本接收行为。`type_text` 响应仅返回
字符数，不回显输入内容。

`key` 接受单个 Unicode 字符（保留大小写）或命名键（忽略大小写）：
`enter` / `return`、`tab`、`space`、`escape` / `esc`、`backspace`、`delete`、
`home`、`end`、`pageup` / `page_up`、`pagedown` / `page_down`、
`up` / `arrowup`、`down` / `arrowdown`、`left` / `arrowleft`、
`right` / `arrowright`、`f1` 至 `f12`。

`modifiers` 为不重复的 `ctrl`、`alt`、`shift`、`meta`。
macOS 的 `meta` 是 Command；Windows 是 Windows 键；Linux 是 Super。
例如复制为 `{"action":"key","key":"c","modifiers":["ctrl"]}`，
macOS 使用 `meta`。每次请求都按下后释放，修饰键逆序释放，错误路径也会尝试释放。
不提供跨请求持有键或鼠标按钮的能力。

## 显示器和坐标

显示器对象：

```json
{"id":1,"name":"Display 1","x":-1920,"y":0,"width":1920,"height":1080,"scale_factor":1.0,"is_primary":false}
```

`id` 是当前系统显示器 ID，重新连接显示器后可能变化。`displays` 返回
`{"coordinate_space":"monitor-local-pixels","displays":[...]}`。

`screenshot` 的 data 结构：

```json
{
  "image_base64":"<PNG 的标准 Base64，不含 data URL 前缀>",
  "mime_type":"image/png",
  "width":1920,
  "height":1080,
  "source_width":1920,
  "source_height":1080,
  "coordinate_space":"monitor-local-pixels",
  "monitor":{"id":1,"name":"Display 1","x":0,"y":0,"width":1920,"height":1080,"scale_factor":1.0,"is_primary":true},
  "displays":[]
}
```

鼠标请求的 `(x, y)` 始终是**所选显示器返回截图内的像素坐标**，左上角为
`(0, 0)`，右下界不包含 `width` / `height`。调用方应使用截图的 `monitor.id`；
省略 `monitor_id` 选择主屏，没有主屏标记时选择第一个显示器。不要将显示器桌面
偏移 `monitor.x/y` 加到鼠标请求里，也不要额外乘以 `scale_factor`。

Windows 截图和坐标采用已启用每显示器 DPI 感知的桌面像素，负桌面坐标副屏通过
原生 `SetCursorPos` 定位。X11 从 RandR 读取精确桌面像素，避免 XCap 对
`Xft.dpi` 换算后的舍入误差。macOS 截图规范化到显示器逻辑点宽高，Retina 图像
会缩小；`source_width/height` 保留捕获源尺寸，以保证截图像素与鼠标坐标一致。
显示器最大规范化面积为 4000 万像素，PNG 最大为 32 MiB（编码后在 PI 的
48 MiB 响应上限内），超过时返回错误并提示降低显示器分辨率。

## 平台范围

- **Windows 10/11**：在已登录的交互桌面中运行。普通权限进程不能控制提升权限
  的窗口，不能控制 UAC 安全桌面或登录屏幕。
- **macOS**：截图需要“屏幕录制”权限，输入需要“辅助功能”权限。
  在“系统设置 → 隐私与安全”中授权 OpenLess / openless-computer 后重新启动
  OpenLess。helper 只做权限预检并返回明确错误，不会自行弹出权限窗口。
- **Linux X11**：要求已登录的 X11 会话、有效 `DISPLAY` 和 XTest 扩展。
  Linux 的 `XDG_SESSION_TYPE=wayland` 或非空 `WAYLAND_DISPLAY` 会明确返回
  `unsupported_session`，即使存在 XWayland 的 `DISPLAY` 也不会声称能控制完整
  Wayland 桌面。无桌面环境返回 `no_display`。

`--capabilities` 无需显示器权限，既不截图也不建立输入连接；返回的
`supported` 只表示平台／会话在实现范围内，`availability:"not_probed"`
表示实际桌面是否可用需要操作时检测。macOS 权限状态为 `granted` / `required`；
其他平台为 `not_checked`。

## 构建和后端测试

在 `openless-all/app` 中运行：

```text
cargo build --locked --release -p openless-computer
cargo test --locked -p openless-computer
```

Windows 需要 MSVC C++ Build Tools 和 Windows SDK；macOS 需要 Xcode Command
Line Tools。Linux 构建依赖包括 `clang`、`pkg-config`、`libxkbcommon-dev`、
`libxcb1-dev`、`libpipewire-0.3-dev`、`libwayland-dev`、`libgbm-dev`，
XCap 的 Linux 捕获依赖会链接 PipeWire / Wayland 库，即使运行时只启用 X11 会话。
Enigo 使用 Rust X11 后端，不依赖 `xdotool` 或 `libxdo`。

测试仅覆盖协议拒绝规则、中文文本、输入大小限制、坐标边界／负桌面偏移、
会话判断、合成 Retina 图像的 PNG 坐标规范化和错误响应契约，不会移动鼠标、
按键或捕获桌面。
