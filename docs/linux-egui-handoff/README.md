# Linux 接入交接总览

状态：canonical（2026-09-07 以源码为准重写，基线 `openless-linux-egui` + Core `639c2fbf` 后的 beta）；更新：2026-09-07。

## 1. 责任与范围

Linux Host Adapter 的剩余实现、全局热键、窗口/托盘/权限/更新等宿主效果、egui 界面与 Linux 产品验收由 egui 团队承接；共享业务全部在 `openless-core`，发现业务接口缺项时回报 Core 负责人，不在 UI/Host 复制规则。Linux 产品完成度不阻塞 Windows/macOS 交付（见[范围](../2.0-requirements.md)）。

## 2. 已有起点（直接复用，不另建第二套后端）

- 组装：`linux-egui/src/backend.rs` — `LinuxBackendBuilder::from_shared_providers(BackendConfig)` + `with_*` 注入（recorder/polisher/inserter/credential store/host actions/settings runtime/local ASR runtime/polish failure policy）。
- Host 门面：`lib.rs` `LinuxHost` — `snapshot` / `subscribe` / `save_settings` / `update_settings_strict` / `drain_events` / `backend()`。
- 已实现模块：`audio.rs`（CPAL 录制）、`credentials.rs`（Secret Service）、`fcitx5.rs`（输入/选区）、`hotkeys.rs`（fcitx5 热键监听）、`selection.rs`、`qa.rs`、`coding_agent.rs`、`marketplace.rs`、`remote_input.rs`、`settings.rs`、`single_instance.rs`、`capabilities.rs`、`resources.rs`、`runtime.rs`、`ui_state.rs`。
- 页面：`ui_state.rs` `Page` 枚举 10 页（Start/Dictation/Qa/Selection/Agent/Services/Models/Remote/History/Settings），主循环 `main.rs`。
- 打包：`release-linux-egui.yml`（deb/rpm/AppImage，独立 manifest）。

## 3. 阅读顺序

1. [01 Core 合同](01-core-contract.md)：可调用的 Core 面与合同文件。
2. [02 缺口登记](02-gap-register.md)：L01–L12 现状、责任与关闭标准（推进主表）。
3. [03 热键与窗口](03-hotkeys-and-windows.md)、[05 原生宿主与数据](05-native-host-and-data.md)：接线缺口细节。
4. [04 页面与领域](04-ui-domains.md)：10 页现状与界面缺口。
5. [06 事件与会话](06-events-and-sessions.md)：订阅、重放、取消语义。
6. [07 验收](07-acceptance.md)：两个完成门与证据要求。

接口细节见[后端契约](../linux-egui-backend-contract.md)；长接口与历史实现参考保留在契约文档。
