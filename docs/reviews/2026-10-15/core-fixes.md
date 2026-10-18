# R1 / R3 修复交付

基线 `0947c05a`，工作目录 `/Users/trip/orca/workspaces/openless/修复-Matel存在的部分问题`。R1、R3 已完成修复和语义回归；只改授权的 3 个文件，没有提交，没有修改 R2 TextStream、前端、其他 Core 文件或 backend-tests manifest/lock。

## 修改内容

### R1：只清理退出的活动 owner，不让清理落后于异步状态刷新

文件：`openless-all/app/crates/openless-core/src/api.rs`。

- `:2332` 新增局部 `local_asr_channel_gate`，只协调 ASR 渠道的 owner 变更与 runtime 清理；LLM 渠道不使用该锁。
- `:4860` 的渠道 mutation 捕获活动 owner，而不是按“正在编辑的任意渠道”的 provider 无条件释放。`set_channel_provider_type` (`:4636`) 与 reorder (`:4771`) 复用此路径，`set_active_provider` (`:4815`) 和显式本地 activation 也参与同一 owner 临界区。
- `:4897` 解析 channel ID / provider type，并兼容 legacy provider-only selection；`active_provider` 不受支持的 persistence-only store 不会因为新检查而失去原有 mutation 能力。
- `:4930` 比较 mutation 前后的活动 owner。非活动同 provider 渠道被禁用、删除、更换 provider 或清理空白草稿时，不会释放活动 runtime；活动本地 owner 离开、且没有同 runtime 新 owner 接管时，仍执行 release。
- 如果规范化后的新活动渠道继续拥有同一个共享 runtime，则保留该实例，避免把“旧 owner 退出”变成“杀掉新 owner”；模型差异仍由下一次显式使用/既有 activation lease 处理。
- **runtime release 在 credential status refresh 之前完成**，之后释放 owner gate。状态查询被阻塞时，新的选择、activation 和 preload 不被 gate 卡住；旧请求恢复时不会再执行任何 runtime 清理，因此不能卸载刚加载的新实例。
- native release 返回错误时，仍发布已经提交的 credential metadata，再向调用方返回 release 错误，避免新增“设置已提交但状态不再更新”的回归。

该修改集中在现有渠道 facade，没有更改 runtime adapter / TextStream ABI，也没有重构全局 settings 或录音链路。

### R3：删除完整性与加载完整性分离

文件：`openless-all/app/src-tauri/src/asr/local/native_model_receipt.rs`。

- `:127` 冷删除直接解析 receipt 的 `model_dir`，不再依赖只接受完整文件的 `installed_dir()`；模型缺文件或文件截断，不会阻止清理受管目录。
- `:93` 通过真实存在的祖先解析目录，即使模型目录及其部分父目录已经不存在，仍可验证归属并完成幂等 cleanup。
- 保留 canonical root containment 检查，拒绝删除缓存根本身、根外现有/缺失路径及指向外部的 symlink。
- 只有目录成功删除或确认不存在后才删除 receipt；JSON 损坏、越界或删除失败会保留 receipt 并报错，后续仍能追踪/重试。
- `installed_dir()` 的加载完整性标准未放宽，状态页也不会为了清理而初始化 Foundry SDK。

## 新增语义回归

`openless-all/app/crates/openless-core/tests/local_asr_contract.rs` 新增 5 项：

1. `non_active_same_provider_channel_edits_do_not_release_the_active_runtime` (`:1287`)：分别禁用、删除、更换 provider、删除空白的非活动同 provider 渠道；断言活动渠道未变、release 不发生、已加载模型保留。
2. `active_local_channel_edits_release_its_runtime_when_no_local_owner_remains` (`:1310`)：对当前 owner 做上述四种操作，断言恰好一次 release 且模型清空。
3. `committed_channel_changes_are_published_even_when_runtime_release_fails` (`:1330`)：release 失败仍传播明确错误，但已提交的渠道变化发出 `CredentialsChanged`。
4. `a_same_runtime_channel_takeover_preserves_the_shared_loaded_instance` (`:1355`)：当前 owner 关闭后由同 runtime 渠道接管，断言不误释放新 owner 的共享实例。
5. `late_credentials_refresh_never_releases_a_reselected_or_reloaded_local_owner` (`:1380`)：分别阻塞 disable/delete/provider type/select/reorder 的 credential refresh，期间重新激活并加载本地模型，然后恢复旧请求；断言新实例保留、不会出现第二次 stale release，且 replacement 不会被 owner gate 锁在状态刷新之后。

`native_model_receipt.rs` 新增 5 项（原有 1 项保留）：

- `:202` 缺文件和截断两种情形都删除整个模型目录，而不只是删除 receipt。
- `:227` 模型目录及其父目录已经缺失时，安全忘记 receipt。
- `:235` 缓存根、根外现有目录、根外缺失目录均拒绝删除，并保留 receipt / 外部文件。
- `:252` Unix symlink 指向根外目录时，即使 receipt 文件列表不完整也拒绝删除。
- `:269` receipt JSON 损坏时报错，不偷偷丢弃追踪信息。

## 测试结果

从 `openless-all/app` 执行：

```sh
CARGO_TARGET_DIR="$PWD/src-tauri/backend-tests/target" \
  cargo test --offline --locked -p openless-core --test local_asr_contract

cargo test --offline --locked \
  --manifest-path src-tauri/backend-tests/Cargo.toml \
  --test native_model_receipt
```

最终结果：

```text
local_asr_contract: 34 passed; 0 failed; 0 ignored
native_model_receipt: 6 passed; 0 failed; 0 ignored
```

`git diff --check` 对 3 个授权文件通过。只对新写的局部代码块使用 rustfmt，没有格式化整个工程；生成局部格式化替换的 `/tmp` 文件已经删除，测试使用的临时模型目录由测试回收。

## 验证边界与交接

这轮 Cargo 仅构建 Core 和独立 backend-tests 的指定目标，全程 `--offline --locked`，没有触发完整 native 构建或 sherpa 库下载，也没有模型下载、录音、调用收费 API、读取用户秘密或安装程序。Core 回归使用内存 credential store / fake runtime，receipt 回归直接编译生产文件、操作临时目录；Windows Foundry SDK 与真实 Windows 文件系统、真实 TextStream/MLX 推理和 GUI 未在本轮执行。R2 文件及主 Agent 并行修改的 manifest、lock、recorder、前端与其他 ASR 文件均未由本 worker 修改。
