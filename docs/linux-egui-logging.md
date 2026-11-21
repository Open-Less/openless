# Linux egui 日志约定

状态：canonical；更新：2026-10-22。适用于 `openless-all/app/linux-egui`，以及被两个前端共用的 `crates/openless-core`。

日志是 Linux 用户唯一能交回来的证据：界面出错时他们既没有控制台，也不会跑 gdb。所以这份文件规定的不是文风，而是**「出问题时能不能从日志里读出结论」**。

## 1. 落在哪里

| 项 | 值 |
|---|---|
| 文件 | `$XDG_DATA_HOME/openless/logs/openless.log`（默认 `~/.local/share/OpenLess/logs/`） |
| 单文件上限 | 2 MiB |
| 历史份数 | `openless.log.1` … `openless.log.3`（合计 ≤ 8 MiB） |
| 轮转时机 | 进程启动时；**只有宿主轮转**，UI 窗口进程不轮转（否则宿主手里的句柄会指向被改名的 inode） |
| 终端 | 同时输出到 stderr，级别与文件一致 |
| 导出 | 设置 → 高级 → 调试 → 「导出错误日志」（`export_error_logs`，原子写） |
| 导出内容 | 当前 `openless.log` **加全部轮转历史**（`.3`→`.2`→`.1`→当前，时间正序），文件头写明版本、系统架构、生效级别与导出时间 |

两个进程（宿主 `openless`、界面 `openless --ui-client`）写同一个文件，排查「窗口进程怎么没了」时两端记录在同一处。

## 2. 级别

| 级别 | 用来记什么 | 判据 |
|---|---|---|
| `error` | **失败**。用户可见的能力没做到，或数据可能受损 | 「这条出现时，功能是坏的」 |
| `warn` | 降级但仍然工作：回退到默认设备、重试成功、可选能力缺失 | 「这条出现时，行为与预期不同」 |
| `info` | **状态变化**与低频存活证明（热键注册结果、模型加载、云同步相位、会话开始/结束） | 用户报「它不工作了」时，需要看到「上一次好的是哪一步」 |
| `debug` | 周期仪表：心跳、IPC 往返、逐帧/逐事件细节 | 打开后能让 `info` 的结论落到具体数值 |
| `trace` | 逐条协议/逐样本，只为复现一次具体 bug | 默认关闭，通常只用一次 |

两条硬规则：

1. **周期性的东西不许写 `info`。** 每 10 秒一条 = 8640 行/天，轮转后整份日志九成都是它，真正的错误被挤出去。这类日志写 `debug`，`info` 只留状态变化与低频存活证明。
   - 参考实现：`log_pump_heartbeat`（`linux-egui/src/main.rs`）——状态变化立刻记 `info`，否则每 5 分钟一条，其余 `debug`。
2. **`error` 只写失败，不写进度。** `log::error!("[x] stage=device_id_ok")` 这类 tracer 会让每次启动看起来都像崩了，必须降到 `debug`。

## 3. 设置里也能改

设置 → 高级 → 调试 → **日志级别**（`info` / `debug` / `trace`）：

- 选完写进 `<data_dir>/logs/level`，**重启 OpenLess 生效**（界面会提示）。
- 这是宿主本地设置，不进 Core 偏好，因此不上云、不影响 Tauri 与移动端。
- 环境变量优先于这里的设置：临时排查不该被上一次的界面选择挡住。

## 4. 不用重新编译也能改级别

```bash
# 全开
OPENLESS_LOG=debug openless

# 只把 Core 的依赖图噪声打开
OPENLESS_LOG=info,openless_core=debug openless

# 环境变量在 systemd 启动的情况下要写进服务定义
systemctl --user edit openless
# [Service]
# Environment=OPENLESS_LOG=debug
```

`OPENLESS_LOG` 优先，`RUST_LOG` 作为兼容名也认。语法：

```
<level>[,<module-prefix>=<level>]...
```

- 模块前缀按 `::` 边界匹配，**最长匹配优先**（`openless_core::asr=warn` 会盖住 `openless_core=debug`）。
- 任何无法识别的片段会让整条规则退回默认 `info`：宁可多记，不可静默少记。
- 默认 `info`。发布版不需要用户改任何东西就能看到状态变化。

启动时的第一行会写明生效的过滤与是否轮转过：

```
2026-10-22T23:40:12.423+08:00 [INFO] openless_linux_egui::logging: Linux egui logger ready: level=debug rotated=false path=/home/x/.local/share/OpenLess/logs/openless.log
```

## 5. 行格式

```
<本地时区 RFC3339> [<LEVEL>] [<target>] [<thread>] <文件:行> <消息>
```

- `target` 一直在（模块路径），定位「哪一层打的」不用猜。
- 线程名与源码位置只在 `debug` 及以上出现，避免 `info` 行变长。
- 时间戳是**用户本地时区**。历史版本写 UTC（结尾 `Z`），用户对着自己的操作时间找不到对应行。

消息里的固定前缀是约定，用方括号小写，便于 `grep '\[fcitx\]'`：

| 前缀 | 范围 |
|---|---|
| `[pump]` | 宿主循环存活 |
| `[hotkey]` | 热键注册与分发 |
| `[fcitx]` | fcitx5 addon 通信 |
| `[ui-host]` / `[ui-client]` | 两个进程各自的 IPC 侧 |
| `[dictation]` | 一次听写的生命周期（Core） |
| `[insert-diagnostics]` | 文本注入 |
| `[audio]` / `[audio-cue]` / `[audio-mute]` | 采集与提示音 |
| `[style]` | 风格包 |
| `[remote-input]` | 手机/远程输入 |
| `[capture]` | 截图与选区 |

## 6. 一次听写在日志里长什么样

```
[dictation] start session_id=… target=Dictation asr=dashscope insert=true translate=false silence_after_ms=None
[audio] capture started backend=pipewire device=… format=F32 rate=48000Hz channels=1
[transcript] partial emitted session=…                      ← 仅 debug
[asr] session_id=… provider=dashscope label=… elapsed_ms=1840 chars=42
[asr-diagnostics] session_id=… asr_chars=42 asr_hash=… corrected_chars=42 …
[polish-diagnostics] session_id=… stage=decision uses_polisher=true …
[insert-diagnostics] session_id=… attempt=1 operation=… chars=42 hash=…
[dictation] session … failed: code=… message=…             ← 失败时（warn）
```

## 7. 定位一次听写

同一次听写里的多行用 `session_id` 串起来（`[dictation]`、`[insert-diagnostics]` 都带）。排查顺序：

```bash
grep 'session_id=<id>' ~/.local/share/OpenLess/logs/openless.log*
```

失败原因现在会同时给出 `code` 与 `message`（见 `mark_dictation_failed`）。历史版本只有 `code`，因此「录音停止 / 转写 / 润色」这一步光看日志分不出来。

## 8. 不要记录的东西

- 任何凭据：API key、token、password、私钥、keystore 内容。日志会被用户原样贴到 issue 里。
- 用户听写的正文。需要时可记长度或哈希（`[insert-diagnostics]` 就是这么做的：`chars=` / `hash=`）。
- 完整 `data_dir` 之外的路径通常没必要；`data_dir` 本身要记，因为「日志写在哪」是第一个问题。

`scripts/egui-logging-contract.test.mjs` 会拒绝形如 `api_key=` / `token=` / `password=` 的日志格式化参数，以及 `stage=` 级别的 `error`。

## 9. 改动这份约定时

- 新增周期日志 → 必须是 `debug`，或带节流。
- 新增 `info` → 必须能回答「状态变了什么」。
- 新增 `error` → 必须对应一次真实的失败分支。
- 跑 `node scripts/egui-logging-contract.test.mjs`。
