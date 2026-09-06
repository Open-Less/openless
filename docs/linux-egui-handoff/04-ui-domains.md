# 04：页面与领域操作

对应 L04–L09。现有[main.rs](../../openless-all/app/linux-egui/src/main.rs)是可复用的真实 UI 起点，但并非功能齐全的产品。
接口查[Core api.rs](../../openless-all/app/crates/openless-core/src/api.rs)与[domains.rs](../../openless-all/app/crates/openless-core/src/domains.rs)。

## 1. 已有页面与需要补齐的操作

| 领域 | 当前生产UI | egui团队接入要求 |
| --- | --- | --- |
| Provider/渠道 | ASR/LLM/Omni渠道创建、重命名、启停、排序、激活、删除、编辑、列模型和校验已有 | 复用 descriptor；完善 vault 锁定/错误恢复、状态刷新和正式交互，不重做认证规则 |
| 本地模型 | Qwen列表、下载、激活、取消已有 | 接 `LocalAsrApi` 的路径/镜像、model_card/remote_info、状态、删除、preload/release、prepare/test_model及取消；激活使用 Core 原子事务 |
| QA | 文本/语音、流式回答、取消/关闭已有 | 接 `set_edit_instruction_mode`、编辑预览、应用/撤回及状态；用每轮 recording token 停止录音 |
| Selection polish | 可编辑预览、确认、取消、完成后撤回已有 | 保留原目标/session核验和错误反馈，不将剪贴板旧选区当有效原目标 |
| Selection Voice | Core service已有，Linux完整触发/意图路由与UI缺失 | 见下节；不是只加一个按钮即可完成 |
| Less Computer | 文本/语音、输出/工具状态、Allow/Deny审批和取消已有 | 接 `CodingAgentApi` 的detect/list_models/run_test/cancel_test和provider/model/executable/workdir/permission配置；不自建审批与续聊策略 |
| 风格包 | 无管理页 | 接Core列表、创建/编辑、启停/激活、内置重置、删除、提示词诊断、ZIP导入/导出；快捷键依赖L01 |
| 词典/纠错 | 无页面 | 接词条增删/启停、预设、规则；pending corrections接受/拒绝/清空。自动建议来源依赖L02 |
| Marketplace | service和下载归档Host已有，无页面 | 接 `MarketplaceApi` 的列表/详情/安装/下载/上传、点赞/作品、设备OAuth启动/轮询/取消/退出 |
| 历史/统计 | 仅最近20条只读和插入状态 | 完整浏览及历史操作、删除/清空、重润色/重转写、复制/导出、录音播放/定位/清理、活动统计；音频先补L03 |
| 设置 | 仅streaming_insert、Agent启用、Remote开关/端口 | 补Linux适用的麦克风/静音、语言/翻译、模式/热键、QA历史、选区、隐私/日志和外观；每项必须有真实消费者 |
| Remote Input | 开关、地址、PIN与重置已有，TLS/H5已接 | 补连接状态、陈旧地址/错误、二维码及LAN/证书信任说明；复用服务并进行真手机验收 |

不要给 Linux 暴露无实现的 Windows TSF 或 macOS AX 权限按钮。当前 Linux 本地运行时为 Generic Qwen；Foundry/Apple MLX 不是这一轮要求跨平台移植的功能。

## 2. Selection Voice 接入链

1. Linux Host 捕获原生目标、选区及会话身份，接入 Core 的共享语音互斥和停止/取消路径。
2. 使用 `SelectionVoiceApi` 的 `begin`、`process_transcript`、`confirm_intent`；由 Core 决定 `route_disposition`。
3. UI 消费 Selection Voice 事件，展示意图选择或路由后的 QA/编辑结果；当前 main.rs 尚未消费这些事件。
4. 编辑经过 `prepare_edit`、`begin_preview_apply`，Host 按 ticket 核验并执行替换，再以真实结果调用 `finish_preview_apply`。
5. 目标失效、取消、重复确认和未知插入结果都应保持安全，不能绕过 Core 事务直接粘贴。

Linux QA 的 target rekey 适配和现有 Selection runtime可以复用；headless直接调用Core的测试不算生产接线完成。
QA 内的预览必须传当前 turn token，使用 `QaApi::begin_edit_preview_apply`／`revert_edit_preview`；原生应用完成后用 `dismiss_session` 只关闭所属回合。不要拆成读取 conversation owner 后直接改 Selection Voice，再无条件关闭 QA。独立 Selection Voice 的 `begin_preview_apply`／`revert_preview` 是无异步原生效果的同步状态操作；`PasteSent` 只表示已发送，不能标成 `Inserted` 或失败。

## 3. 共同交互规则

- UI只保留编辑草稿、焦点等显示状态；持久化变更走Core facade，不直接读写JSON、凭据或风格ZIP内容。
- 设置携带revision，冲突后重读；渠道ID与provider类型分开，默认值/认证来自descriptor。
- 长操作在Host runtime调度，事件驱动重绘；egui frame不阻塞等待网络、模型下载或进程。
- 字段禁用、加载、取消、错误、空列表、重试与重开须齐备；按钮存在但效果未接入应标明不可用。
- 历史重转使用Auxiliary服务与`apply_history_retranscription`，真实provider/model/timing由Core生成；不要根据当前设置猜历史归因。
- API密钥只写入不读回；配对PIN通过显式配对接口受控显示，不进入通用日志/状态广播。

## 4. 关闭标准

每个领域交付“用户入口 → Core调用 → Host效果 → 事件/持久化”的实际流程；验证成功、失败、取消、重新打开和重启。
领域UI完成不自动关闭相关原生缺口；L02/L03/L01分别验收上下文、音频归档和全局热键。
