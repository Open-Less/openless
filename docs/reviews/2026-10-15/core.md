# PR #1197：Core / native / egui 独立只读审查

审查基线：`0454dc56..0947c05a`，仓库 `/Users/trip/orca/workspaces/openless/修复-Matel存在的部分问题`。下文行号对应 `0947c05a`；本次没有修改仓库源码、配置或凭据，没有安装程序、下载模型、主动录音、调用收费 API、启动应用或创建子 Agent，也没有执行 Cargo / 完整构建。

## 结论与优先级

发现 3 个生命周期问题，均由 **`16a73b9d`** 引入。优先修复 R1 的渠道所有权判断，其次修复 R2 的子进程取消句柄和 R3 的损坏缓存清理；未发现能够在本次约束内确认的永久互锁，也没有把“多模态无输出”未经实测地归因于本分支。

| 编号 | 优先级 | 问题 | 核验强度 |
|---|---|---|---|
| R1 | P2 | 禁用/删除非活动同类型 ASR 渠道，会释放活动渠道共享的 runtime | 原函数提取后的 spy-port 探针已复现 release 决策；native 后果沿源码追踪 |
| R2 | P2 | TextStream 在 EOF 后等待进程退出时丢失取消句柄，可留下仍存活的 worker | 提取原 `Process` 实现、以自有 shell/sleep 子进程确定性复现；不是 MLX 真机复现 |
| R3 | P2 | Windows Foundry 的损坏/缺文件缓存无法冷删除，且删除成功后丢失 receipt | 静态路径确认；未运行 Windows SDK / 原生 UI |

这里的 P2 表示应修复的条件性回归，不表示所有普通录音都会失败。R2 的成立条件特意保留为“stdout 已 EOF，但进程仍存活”，不能把该边界探针写成真实 MLX teardown 已发生挂起。

## R1 — 非活动渠道编辑也会释放活动渠道的共享 runtime

**定位**：`openless-all/app/crates/openless-core/src/api.rs:4901-4915`，尤其 `4914-4915`。引入提交：`16a73b9d`。

`apply_channel_mutation()` 按被修改渠道的 `provider_type` 获取 `retiring_provider`，但不检查该渠道是否是活动渠道、是否持有该 runtime 的录音/加载 lease；持久化与状态刷新后，直接调用 `release_local_asr_provider()`。因此渠道 A 与 B 共用一个本地 provider，A 活动、B 非活动时，禁用或删除 B 仍会释放 A 的 runtime。重复 provider 的渠道是 Core 支持的状态，Windows 的 Foundry/Sherpa 添加渠道也允许重复；实验 provider 不在当前 UI 的新增列表中，但 Core 创建/导入契约仍可形成该状态。

**触发条件**：存在两个同 provider 的 ASR 渠道，其中只有 A 是活动渠道；编辑 B 的启用状态或删除 B。若活动 provider 是 TextStream，且已有运行中的 worker，该无关编辑会走 `api.rs:4921-4923` → `experimental_asr.rs:510-516` 的 `release_runtime()` → `textstream.rs:112-133` 的 `stop()`，取消活动 worker；Foundry/Sherpa 则发生不必要的共享实例释放，不能据此一概断言它们丢失转写，因为其部分 session 能保留 Arc 或重新加载。

**可复核证据**：探针 `/tmp/openless-review-core-channel-probe.py` 直接从当前 `api.rs` 提取 `apply_channel_mutation()` 函数体，使用只在内存工作的 credential ports 与 release spy，未执行 native 推理。运行：

```sh
PYTHONDONTWRITEBYTECODE=1 python3 /tmp/openless-review-core-channel-probe.py
```

输出：

```text
inactive_channel_action=disable; active_channel=active; runtime_releases=["experimental-textstream"]
inactive_channel_action=delete; active_channel=active; runtime_releases=["experimental-textstream"]
```

这证明“活动渠道未变化，却执行了同 provider release”，不是完整产品录音验证。

**修复建议**：区分渠道配置变更与 runtime 所有权退出。至少仅在被修改渠道持有活动 runtime 时执行释放；更稳妥的是捕获原 owner / generation，在异步刷新之后只释放该旧 lease，避免 A→B→A 快速切换时旧清理误杀新实例。不要仅在 await 前读取活动 provider，await 后无条件释放同类型的当前实例。

**现有测试缺口**：`crates/openless-core/tests/local_asr_contract.rs` 覆盖活动渠道的 disable/reorder、无隐式 preload、重启冷启动等；`experimental/acceptance.rs` 也只禁用当前正在测试的渠道。缺少“两个同 provider 渠道，编辑非活动的一个，活动 runtime / session 保持不变”的断言，也缺少延迟 credential status 期间切换回来后的 owner/generation 场景。

## R2 — EOF 后 `completed()` 取走 Child，之后 cancel/timeout 不再能杀它

**定位**：`openless-all/app/src-tauri/src/asr/local/experimental/textstream.rs:57-64`；关联 EOF 路径 `:366-368`，取消/超时清理 `:440-459`。引入提交：`16a73b9d`。

`Process::completed()` 在 `child.wait()` 前用 `.take()` 将唯一 `Child` 从共享 `Mutex<Option<Child>>` 中拿走。若 stdout 已关闭但 worker 仍停留在退出/原生析构阶段，reader 线程会在局部 `Child::wait()` 中阻塞；此时 `stop()`、Session 超时清理、关闭实验开关和 `Drop` 看到的都是 `None`，不能再杀掉该仍存活的进程。`running()` 同时错误返回 false，下一轮可尝试启动另一个使用固定 7890 端口的 worker。

**触发条件**：worker stdout 到达 EOF，但 worker 进程尚未退出；随后调用 cancel / runtime release，或等到 finish 的 180 秒超时。正常 Python/MLX 退出若很快完成，不会暴露这个问题。本次不声称真实 MLX 已复现退出挂起。

**可复核证据**：`/tmp/openless-review-core-process-probe.py` 从 `textstream.rs` 提取原 `Process` struct/impl，只用 std 和 libc 声明编译一个小型 rustc 探针，不经过 Cargo。自有子进程关闭 stdout 后执行 `sleep`，模拟同一退出边界；探针调用原 `completed()` 和 `stop()`，最后明确 SIGKILL 并回收自己的进程组。

```sh
PYTHONDONTWRITEBYTECODE=1 python3 /tmp/openless-review-core-process-probe.py
```

输出：

```text
EOF observed; completed owns Child; running()=false; child_alive_after_stop=true
waiter_after_explicit_cleanup=Err("TextStream worker exited: signal: 9 (SIGKILL)")
```

**修复建议**：等待退出与取消必须共享仍有效的进程所有权。可由一个专用进程监督任务独占 Child、通过消息接受 kill/wait；或使用短周期 `try_wait`，仅在确认已退出后从共享 slot 移除。不要把 mutex 持有到无界 `wait()` 结束，否则只是把“丢失 kill 句柄”换成 stop 无法获得锁。

**现有测试缺口**：`textstream.rs` 已测试 startup cancellation、空闲 writer 唤醒和 generation preflight；`protocol.rs` 与 `test_worker.py` 也覆盖 EOF 不提交 draft。但它们没有测试“EOF 不等于进程退出”，也没有断言 finish timeout / disable / cancel 后 owned PID 确实消失。应新增先发 ready/final、再关闭 stdout 而不退出的假 worker，验证取消与超时都能回收。

## R3 — 损坏的 Foundry 缓存删除只移除 receipt，留下大文件

**定位**：`openless-all/app/src-tauri/src/asr/local/native_model_receipt.rs:92-106`，完整性判定 `:70-80`；调用方 `foundry_runtime.rs:1131-1134`。引入提交：`16a73b9d`。

冷删除 `remove()` 通过 `installed_dir()` 获取删除路径，而 `installed_dir()` 只有 receipt 中每个文件都存在且大小完全一致时才返回目录。成功下载后，只要其中一个文件被截断/移走/清理，或者一次后续替换使 receipt 不再完整，`installed_dir()` 就返回 None；`remove()` 随后跳过目录删除，仍调用 `forget()` 并返回成功。其余数百 MB / GB 缓存因此残留，而且唯一指向缓存目录的 receipt 被删掉，之后无法再通过该冷删除路径找到它。

**触发条件**：Windows Foundry 已有 receipt，模型缓存后来不完整；重启后 SDK manager 尚未初始化，调用模型删除。相同问题不要求 SDK 推理发生，也不应靠先启动 SDK 才能清理。

**验证边界**：已逐行追踪 `installed_dir → remove → forget` 以及 manager 为 None 的实际调用分支；这是静态确定的文件状态分支。本次未执行 `native_model_receipt` Rust 测试、未运行 Windows SDK，也没有修改用户缓存来复现。

**修复建议**：把“可用于加载的完整模型”和“应用可安全删除的受管路径”分开。删除时先读取 receipt 的 model_dir，独立做 canonical / root containment 与 symlink 边界检查，不要求内容完整；只有删除完成或目录确实不存在后才忘记 receipt。receipt 损坏或目录无法安全归属时应返回明确错误，而不是成功且失去追踪。

**现有测试缺口**：`native_model_receipt.rs:113-128` 只覆盖完整权重的 record/lookup/remove 和非法 alias。应覆盖两个文件中缺一个、一个文件截断、receipt 存在而目录不存在、拒绝 root/outside 路径，并在成功删除后断言剩余大文件也消失。

## 本次执行的验证

以下检查均通过，没有使用 Cargo 锁、模型下载或录音：

- `PYTHONDONTWRITEBYTECODE=1 python3 openless-all/app/src-tauri/src/asr/local/experimental/textstream/test_worker.py -v`：7/7，通过；native 模块全部 fake，SSE 仅临时 loopback server。
- `node scripts/local-asr-lifecycle-contract.test.mjs`。
- `node scripts/macos-local-asr-retirement-contract.test.mjs`。
- `node scripts/macos-audio-usage-description-contract.test.mjs`：只对 `/tmp` plist fixtures 运行检查脚本。
- `node scripts/macos-optional-runtime-bundle-contract.test.mjs`。
- `node scripts/macos-runtime-minimum-version-contract.test.mjs`：静态契约确认 14.0。
- `node scripts/less-computer-opencode-contract.test.mjs`。
- 上述两个原函数提取探针均编译、执行成功，临时 rustc 源码/可执行文件自动清理，自有子进程也已回收。

这些 Node 检查主要是源码/结构契约，不能证明 worker 生命周期、Windows 原生推理、打包/签名或真实 GUI 成功。`git diff --exit-code HEAD -- openless-all/app/crates/openless-core openless-all/app/linux-egui openless-all/app/src-tauri .github` 在核验时为空；工作树里主 Agent 的前端修改未被本次更改或回滚。

## 审查覆盖清单

“完整”指读取新增文件或相关实现全部；“差异+追踪”指覆盖本范围所有变更块，并读取必要未修改调用方，不表示整个大型文件每一行都重新审计。

### Core（差异+追踪；实验 service/tests 完整）

路径前缀 `openless-all/app/crates/openless-core/`：

- `src/api.rs`：Beta admission / opt-out cleanup、本地渠道 mutation、ASR activation/release/settings 事务；追踪活动所有权与 credential await。
- `src/experimental_asr.rs`、`src/experimental_asr/tests.rs`：下载、取消、独立 worker 锁、开关 generation、重启恢复、install、删除、channel 注册与 lease。
- `src/experimental-gemma-manifest.json`、`src/experimental-textstream-manifest.json`：读取 manifest 入口与 shape/哈希校验路径；未联网重验全部上游对象内容。许可证文件为新增数据，不作法律审查。
- `src/local_asr_service.rs`、`src/model_store.rs`、`src/local_asr_catalog.rs`：本地 activation、prepared/installed 边界、文件下载取消、迁移、旧模型清理、storage relocation、native adapter 分流。
- `src/coding_agent.rs`、`src/less_computer.rs`：Beta gate、settings admission 锁序、run/capture/test ownership、取消与 cleanup；没有执行外部 Coding Agent CLI。
- `src/credentials.rs`、`src/provider_rules.rs`、`src/provider_service.rs`：retired channel 投影、experimental channel 注册、本地验证 probe 不启动推理、provider defaults。
- `src/settings.rs`、`src/shared_types.rs`、`src/domains.rs`、`src/dictation_context.rs`、`src/lib.rs`：新开关默认值与反序列化、Host 能力、接口及 TextStream batch preference 例外；未扩展审查首录音链路。
- `src/cloud_sync_e2ee_documents/{registry.rs,tests.rs}`、`src/cloud_sync_e2ee_store/{native.rs,tests.rs}`：device-local opt-in 排除同步及 rollback 状态保留。
- `examples/export_provider_descriptors.rs`。
- `tests/{local_asr_contract.rs,macos_asr_retirement.rs,auxiliary_voice_lifecycle.rs}`：已有生命周期断言及上述缺口；Rust 测试没有运行。

### Tauri / native（差异+追踪；新增运行时/installer/protocol 完整）

路径前缀 `openless-all/app/src-tauri/`：

- `src/asr/local/experimental.rs`、`experimental/{gemma.rs,gemma.swift,textstream.rs,acceptance.rs}`。
- `src/asr/local/experimental/textstream/{install.rs,install-guardian.sh,protocol.rs,worker.py,test_worker.py}`；`requirements.lock` 仅核对被 `--require-hashes` / `--only-binary` 使用的边界，未联网审计全部 wheels。
- `src/asr/local/native_model_receipt.rs`、`src/asr/local/foundry_runtime.rs`、`src/core_adapters.rs`：Foundry download/restart、receipt inspection、冷删除、runtime release 与实验 service 绑定。
- `src/asr/local/{mod.rs,models.rs,test_run.rs}`；旧 native 文件删除及其 router/build 引用清理：`apple_speech_provider.rs`、`cache.rs`、`local_provider.rs`、`mlx_qwen_engine.rs`、`mlx_set_metallib_path.cpp`、`mlx_worker.rs`、`qwen_engine.rs`、`qwen_ffi.rs`、`whisper_provider.rs`；未重新审计被删除代码内部全部算法。
- `src/commands/{credentials.rs,experimental_asr.rs,foundry_asr.rs,mod.rs,qa.rs}`：host/窗口边界、provider retirement、IPC 注册与 file-only 状态。
- `src/{lib.rs,tauri_coordinator_host.rs,tauri_events.rs,hotkey.rs}`、`src/coordinator/hotkey_loops.rs`、`src/persistence/credentials.rs`：window gate/main-thread 调度、隐藏、AppKit 系统事件转换、权限与设备级迁移；未触发系统权限弹窗。
- `backend-tests/tests/{native_model_receipt.rs,textstream_protocol.rs}`：测试文件挂接检查。
- `Cargo.toml`、`Cargo.lock`、`build.rs`、`Info.plist`、`tauri.conf.json`、`tauri.macos.conf.json`、删除的 `tauri.macos-mlx.conf.json` 与 vendor gitlinks。

### egui / Linux（全部变更块+必要调用方；不运行原生窗口）

路径前缀 `openless-all/app/linux-egui/`：

- `src/main.rs`：QA popup generation、queued event 隔离、状态 delta、失败投递、窗口重开/关闭、选区预览与 QA 共用窗口、tray 生命周期；逐项对照新增回归测试。
- `src/{backend.rs,i18n.rs,lib.rs,popup.rs,settings.rs}`、`src/linux_app/settings_save.rs`。
- `src/ui/frontend/{mod.rs,settings.rs,siri_gl.rs,view_model.rs}`：Beta 显隐/撤销、快捷键显示、CPU glow 非有限值修复及 tessellation 测试。
- `examples/headless_host.rs`、`tests/host_contract.rs`。

本轮未找到可确认由这些变更造成的 QA/选区“无输出”回归；不能将这一结果解释成已完成真实选区捕获、fcitx5、X11/Wayland 或付费多模态服务验收。Less Computer popup 未在 supervisor polling 列表中是基线已存在现象，未将它作为本分支新问题报告。

### 构建 / 合同 / 脚本

- `.gitmodules`、`.github/actions/cache-macos-mlx/action.yml` 删除；`.github/workflows/{android-apk.yml,ci.yml,release-tauri.yml}` 差异。
- `openless-all/app/contract/backend-2.0.json`。
- `openless-all/app/scripts/build-mac.sh`、`check-macos-audio-usage-description.sh`、`macos-dmg-layout.py`、`sync-egui-i18n.mjs`、`measure-startup.mjs`；后者仅为 browser benchmark，未当作 native 启动性能证据。
- 新增/变更的 `desktop-version-contract.test.mjs`、`local-asr-lifecycle-contract.test.mjs`、`macos-local-asr-retirement-contract.test.mjs`、`macos-audio-usage-description-contract.test.mjs`、`macos-optional-runtime-bundle-contract.test.mjs`、`macos-runtime-minimum-version-contract.test.mjs`、`macos-build-cache.test.mjs`、`shared-backend-wire-contract.test.mjs`、`less-computer-opencode-contract.test.mjs`、`encrypted-sync-ui-bridge.test.mjs`、`frontend-test-runner.test.mjs` 的本次变更；删除的 macOS MLX staging/toolchain/cache 脚本及其引用。
- 对照 `docs/local-asr-lifecycle.md`、`docs/experimental-local-asr.md`、`docs/local-asr-verification.md` 的边界说明。Foundry 老安装无 receipt 必须一次显式 Download/verify + 重启是已有明确设计，未将该行为本身误报为 bug。

## 未验证与交接边界

前端视觉/交互和首录音链路由其他审查负责；本轮仅为论证上面的生命周期结论读取了少量相关前端/录音适配调用。未运行 Cargo 测试或构建、Windows/Linux 原生程序、真实实验模型、安装 Python/wheels、实际选区/麦克风/Accessibility 操作、DMG/签名/公证与任何收费服务；历史文档中“绿 CI/真实模型验收”没有被复述为本次独立执行结果。主 Agent 若修复，建议先补上述 owner / EOF-live-child / corrupt-receipt 三组小测试，再按对应平台做必要验证，不需要为本报告重新完整构建全部平台。
