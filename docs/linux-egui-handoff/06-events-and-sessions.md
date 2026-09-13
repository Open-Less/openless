# 06：事件、会话与取消

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。

入口：Core `events.rs` / `types.rs`、`linux-egui/src/lib.rs`（`LinuxHost`）、`linux-egui/src/main.rs`（主循环）。

## 1. 订阅与消费

- `LinuxHost::subscribe()` 返回 `EventSubscription`；`drain_events()` 批量取走语义事件交给 UI reducer。
- 主循环必须在每帧消费事件；导航切换不得停止事件消费或丢弃会话（QA/Selection/Agent 的后台事件保留操作或导航提示）。

## 2. 会话与取消

- 所有长操作走 Core session：创建/取消/终态由 Core 裁决，UI 只触发与显示。
- Remote stop 保持可取消的 session；迟到结果按 epoch 丢弃（桌面同规则，见[桌面验收](../2.0-desktop-acceptance.md)划词域）。
- 事件顺序与重放语义以 `contract/backend-2.0.json` 的 `backendEvent` 与 Core `events.rs` 为准；UI 不得自行重排或合并事件。

## 3. 已有时序约定（保留，不因重写界面丢失）

- 听写终态、录音停止与音量恢复事件必须成对（见[05](05-native-host-and-data.md) L03）。
- 审批/待办事件跨页存活；审批动作固定在页面外展示。
- 启动阶段先消费 startup snapshot，再消费增量事件（见[01](01-core-contract.md)）。
