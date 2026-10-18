# macOS 本地 ASR 下架与实验模型验收

> 这是桌面分支整合前的历史验收记录，产物版本为 `2.0.0-Beta.4+build.20260930`。后续构建会覆盖相同的 `.app` 路径；当前 `2.0.0-beta-a.beta.5+build.20261015-bd` 的结果与校验值以 [桌面整合验收](desktop-beta5-integration.md) 为准。

验收时间：2026-10-15（Apple Silicon，macOS 27.0.1）。代码位于当前未提交工作区；不是已发布版本的验收声明。功能与复现入口见 [实验性本地语音模型](experimental-local-asr.md)。

## 结果

| 检查 | 结果 |
| --- | --- |
| `cargo test --locked -p openless-core` | 1,376 项通过（1,179 单元 + 197 集成），0 失败；另有 1 项依赖固定 Codex CLI 的无关测试默认忽略 |
| Tauri `cargo test --locked … --lib -- --test-threads=1` | 529 项通过，0 失败；4 个按需运行的模型/安装测试默认忽略，相关真实推理与安装另行验证 |
| 独立 `src-tauri/backend-tests` | 40 项通过，0 失败 |
| `npm test` | 生产前端构建及 117 个前端/契约测试文件通过 |
| Chromium / WebKit 设置页验收 | 共 6 项通过，覆盖旧入口禁用与指定悬浮提示、三模型入口、后台下载提示、Windows 原入口不变 |
| Python worker 协议/SSE 单元测试 | 7 项通过，无模型加载或麦克风调用 |
| SenseVoice 真实推理 | 英文、普通话固定 WAV 均通过 |
| Embedding Gemma 真实推理 | 英文固定 WAV 通过 |
| TextStream 真实推理 | 独立 Python/MLX worker、可修订预览、SSE 和最终文本验收通过 |
| 最新 debug `.app`，不补下载的隔离验收 | 通过，退出码 0 |
| Release `.app` / DMG | 构建、ad-hoc 签名验证、DMG 校验和及布局验证通过 |
| `git diff --check` / 新实验模块 Rust 格式检查 | 通过 |

完整 Core、原生、前端测试中的初次失败已修复，以上统计来自最后一次成功结果，不把早期失败日志当作通过证据。旧引擎的空壳缓存、兼容 facade 和不可达调用路径也已删除，不再通过空实现维持旧运行接口。

## 真实模型、进程与文件验证

测试使用 `/tmp/openless-asr-acceptance` 隔离目录以及 6.925 秒、16 kHz 单声道 PCM WAV，由系统 TTS 合成后交给 OpenLess 的真实原生适配器。**没有调用麦克风、系统凭据库，或安装/清理当前用户的 OpenLess 应用数据。**

- SenseVoice、TextStream 使用固定版本下载，并通过大小和 SHA-256 校验。下载中断后从断点恢复，没有重下已完成的模型。
- Gemma 测试包从已有文件只读复制到隔离目录，全部 26 项文件通过固定清单校验；源应用数据未修改。
- 实验开启、文件安装和渠道注册结束后，原生模型句柄仍为空，TextStream worker 未启动。
- 已开启且已选中模型的重启恢复、反复状态查询不会加载模型。
- 实际录音会话才加载模型；三模型识别结果均包含 `hello`、`test`、`system`。
- TextStream 的 SSE 终态为 `type: stream`、`done: true`，`draft` 为空，`finalized` 与 OpenLess 返回的最终文本一致；录音中确实产生替换型预览，不靠最后一个草稿充当最终结果。
- 通过真实 Core 渠道开关验证释放，不只调用测试用的释放函数；实验开关和模型文件不因停用渠道被删除。
- 再次启动会话后取消，完成接口返回错误，未提交草稿；关闭实验开关后保留文件，重新启动仍为停用状态，无法获取推理租约。
- 在真实 `.app` 内对临时旧模型/中断目录执行清理；重复执行仍安全，实验包和无关文件保留。符号链接、渠道退休与元数据边界另外由 Core 回归覆盖。
- 验收结束后，没有残留 TextStream worker 或 TCP 7890 监听。

最新 debug bundle 验收记录：`/tmp/openless-asr-acceptance/deliverable-acceptance.log`、`deliverable-acceptance.exit`（0）。结果收据为同目录的 `passed.json`；汇总为 `verified-tests.json`。临时目录可能被系统清理，本文保留主要结果及产物校验值。

固定英文 WAV SHA-256：

```text
d41257872b65e283ad4b54809e2cdda7160bd733b4cfc9a7dc13150669710843
```

三模型输出（标点差异正常，不代表任意真实语音都能达到同样效果）：

```text
SenseVoice: Hello, this is a test of the speech recognition system; the local model should recognize this sentence.
Gemma: Hello, this is a test of the speech recognition system. The local model should recognize this sentence.
TextStream: Hello. This is a test of the speech recognition system. The local model should recognize this sentence.
```

最新 debug bundle 主程序 SHA-256：

```text
4a6f0acf28e7e90cf1fe1037b050fe63daf252975e50d19b82c59e2ac8183855
```

debug 模式的整包哈希校验开销显著，以上测试不作为发布版首字延迟/实时率基准。它证明真实模型与生命周期可用，不承诺所有语言、长录音、设备或口音的识别质量。

## 可交付产物

从仓库根目录查看：

- Release 应用：`openless-all/app/src-tauri/target/release/bundle/macos/OpenLess.app`
- DMG：`openless-all/app/src-tauri/target/release/bundle/dmg/OpenLess_2.0.0-Beta.4+build.20260930_aarch64.dmg`
- debug 验收应用：`openless-all/app/src-tauri/target/debug/bundle/macos/OpenLess.app`

Release 使用 `INSTALL=0` 的正式打包脚本生成，没有覆盖 `/Applications`，没有重置当前应用权限。Release `.app` 约 78 MiB，DMG 35,168,578 字节。

验证项目：

- `codesign --verify --deep --strict` 通过，含麦克风 entitlement。
- Info.plist 仍有麦克风说明，不再声明旧系统语音识别权限。
- Info.plist 与 Mach-O `LC_BUILD_VERSION` 均要求 macOS 14.0；Gemma 自身另行限制 macOS 15+。
- 不包含旧 `mlx.metallib`，主程序未链接旧 MLX/Whisper/Speech.framework。
- 旧 `--openless-mlx-worker` 启动参数仅返回下架提示，不创建推理 worker。
- DMG CRC 校验、Retina 背景、窗口/图标布局及 Applications 链接验证通过。

Release 主程序 SHA-256：

```text
c4802ab6dd64842ecd1209751ab990df5a31650e6a8facf792ec16cccea412c0
```

DMG SHA-256：

```text
0a1de42b40f2cd6583321edee77821e1605ea38ea788e3d1760ab4ca2f43b43c
```

## 验证范围之外

- 本机没有 Developer ID / Apple 公证凭据。本次产物为 **ad-hoc 签名的本地测试包，不是已公证的公开分发包**；浏览器下载仍可能触发 Gatekeeper。
- 三模型的进程级推理验收使用 debug `.app` 的隔离入口；Release 另行完成构建、签名、资源和安装镜像检查，未对已安装用户配置启动正式 UI。
- Windows 入口由浏览器回归覆盖，相关共享业务逻辑由 Core 回归覆盖；本机没有 Windows 运行环境。尝试交叉编译时缺少 `x86_64-w64-mingw32-gcc`，因此没有把 Windows 原生构建、SDK 或真实设备行为标成通过。
- 未进行真实麦克风、其他应用文本插入、旧版用户真实安装升级，或所有 macOS 版本/Intel 设备的验收。升级自动清理逻辑已实现并用隔离旧数据验证，但当前安装应用的数据未被清除。
- 构建仍有现有的弃用/FFI 声明和 window-vibrancy 重复符号警告；未为这次 ASR 改动顺便重构无关窗口/热键代码。
