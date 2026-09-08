# 02：Linux 缺口登记与实施顺序

基线：`2b315572`；责任依据：[当前范围](../2.0-requirements.md)。以下是源码盘点，不是通过声明。

## 1. 状态定义

- **接线缺口**：Core 已有业务，Linux Host 尚未执行必要的原生效果。
- **界面缺口**：已有 Core/Host 入口，生产 UI 未提供完整操作。
- **待实测**：存在真实实现，但还需要对应桌面、设备或安装证据。
- 本表由 egui 团队推进 Linux Host/UI；若发现共享接口不能承载该业务，回报 Core 负责人修复，不在 UI 复制规则。

## 2. 待办登记

| ID | 当前状态与明确缺口 | 关闭标准 / 详细说明 |
| --- | --- | --- |
| L01 | 接线＋界面：`switch_style`、`open_app`、`style_packs` 热键修改被 Linux settings 明确拒绝 | 实际全局注册、触发、重绑、失败恢复和重启还原；分别声明 X11/Wayland 支持。[热键](./03-hotkeys-and-windows.md) |
| L02 | 接线：HostContext/EditObservation 仍为默认 Noop，Selection 的 source_app 为 None | 按隐私设置捕获真实应用/允许的上下文；原生手改观察能产生 Core 纠错建议并拒绝迟到结果。[原生](./05-native-host-and-data.md) |
| L03 | 接线：CPAL 无 RecordingArchive，未执行录音期间系统静音/恢复；缺提示音与胶囊 | 录音归档、保留策略、失败恢复、同归档重试和历史重转可用；所有终态恢复音量，反馈可见。[原生](./05-native-host-and-data.md) |
| L04 | 接线＋界面：Selection Voice 未形成 Linux 生产触发/捕获/意图路由；QA 缺编辑模式/应用/撤回 UI | 完整触发至 Core intent、预览、目标核验、应用/取消/撤回；不能用已有 Selection polish 页面代替。[领域](./04-ui-domains.md) |
| L05 | 界面：词典、纠错规则/建议、风格包管理没有页面 | 各领域增删改/启停/预设/导入导出和失败反馈；通过 Core facade 持久化。[领域](./04-ui-domains.md) |
| L06 | 界面：Marketplace service 已接入，市场页面缺失 | 浏览、安装、上传/下载、点赞/作品管理、设备 OAuth 登录/取消/退出；复用 Core 协议。[领域](./04-ui-domains.md) |
| L07 | 界面＋L03依赖：历史仅只读最近20条，缺完整历史/统计/录音操作 | 浏览和原有历史操作、重润色/重转写、录音播放/导出/清理与统计；真实归因不由 UI 拼装。[领域](./04-ui-domains.md) |
| L08 | 界面：本地 Qwen 有下载/激活/取消，缺完整模型管理和运行时控制 | 路径/镜像、详情/状态、删除、预载/释放和准备/测试取消；真实推理另附证据。[领域](./04-ui-domains.md) |
| L09 | 界面：多数 Linux 适用设置、Agent 检测/模型/路径/权限配置缺失 | 设置有实际消费者、revision/错误处理；保留现有 Less Computer 输出/审批/取消流程。[领域](./04-ui-domains.md) |
| L10 | 接线＋界面：无托盘/自启；通知仅状态栏，重启仅提示，AppImage能力判定不等于更新器 | 窗口/后台运行、通知、自启、检查/下载/安装更新与重启有真实 Host 效果。[原生](./05-native-host-and-data.md) |
| L11 | 已实现待实测：fcitx5 输入/选区、CPAL、Secret Service、Qwen、CLI 进程、Remote TLS/H5 | 通过真实桌面/设备矩阵；未通过的逐项记录，不把整模块称为缺失。[验收](./07-acceptance.md) |
| L12 | 已有包构建，待 Linux 产品验收与正式分发 | 完成上述应用缺口及 Linux 安装/升级/回滚、签名和更新证据；不阻塞 Windows/macOS 首批交付。[验收](./07-acceptance.md) |

## 3. 已有且应直接复用

- 云 ASR/LLM/Omni、Provider descriptor 和渠道管理、Linux vault、Core ModelStore、Generic Qwen runtime。
- 普通听写与现有 fcitx5 热键、ticket 化落字、Selection polish 预览/撤回、QA 文本/语音、Less Computer 工具审批。
- Remote Input TLS/H5、配对和会话桥接、单实例、Core 合同校验、事件重放、native shutdown。

对应实现位置见[Core合同](./01-core-contract.md)、[领域](./04-ui-domains.md)和[原生](./05-native-host-and-data.md)。这些代码是接入起点，不需要另建第二套后端。

## 4. 依赖顺序和完成口径

1. 保留启动、版本校验、事件与退出合同，跑通一个现有听写流程。
2. 补 L01–L03 原生能力；页面可并行，但历史音频/自动纠错不能脱离其 Host 依赖单独宣布完成。
3. 补 L04–L09 领域入口及完整成功/失败/取消流程，再补 L10 桌面集成。
4. 按 L11–L12 获取真实环境与发布证据。

任务记录格式：`ID / owner / commit / 已完成效果 / 自动证据 / 设备证据 / 剩余限制`。
“待实测”不是“未实现”，也不是“已完成”；新发现必须按层归属，不能仅因 Linux 故障就判定 Core 缺失。
