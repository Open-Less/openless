# Linux（egui 宿主）已知缺口登记

这份文件记录**已被确认、但当前不打算实现**的 Linux 侧缺口。每条都写明：现象、
根因（带代码位置）、为什么现在不补、将来若要补需要什么。

补齐后把条目移到对应的验收文档（`docs/linux-egui-handoff/`）。

---

## G01 · 词汇纠正卡片在 Linux 上永不出现

**现象**：听写结束后手工改掉插入的文字，macOS/Windows 会弹出「要不要记住这个纠正」
卡片，Linux 不出来。设置里的词汇表仍然可以手动加词。

**根因**：卡片的数据源是 Core 的 `pending_corrections()`，只有宿主把它接上
「原生文档变更观察器」才会被填充：

```
手动改文本 → 宿主 EditObservationAdapter::arm() → Core 判定属于刚插入的文本
          → CoreEditObservationSink::publish() → queue_pending_correction()
          → 宿主轮询 pending_corrections() → 胶囊窗画卡片
```

- `linux-egui/src/backend.rs` 构造 `BackendDependencies` 时**没有设置
  `edit_observation`**，于是 Core 回退到 `NoopEditObservationAdapter`
  （`crates/openless-core/src/ports.rs`，`arm()` 直接 `Ok(())`）。
- 因此 `pending_corrections()` 恒为空，`main.rs::refresh_vocab_suggestions()`
  每次都走 `hide_vocab_card()`。
- macOS 的实现在 `src-tauri/src/core_adapters.rs`（`TauriEditObservationAdapter`，
  基于 AX API），Windows 基于 UIA。Linux 没有等价物：X11 需要 AT-SPI 且要求目标
  应用开启无障碍；Wayland 下没有跨应用读取焦点文本的通用接口。

**为什么现在不补**：卡片是「锦上添花」的学习入口，补它需要引入 AT-SPI 常驻监听、
处理无障碍权限与隐私边界，收益不足以支撑当前范围。词汇学习仍可通过设置里的
词汇表手动完成。

**将来若要补**，最小实现路径：

1. 新增 `linux-egui/src/edit_observation.rs`，实现 `EditObservationAdapter`；
2. 订阅 AT-SPI `org.a11y.atspi.Event.Object::TextChanged`（需要 `atspi` crate，
   以及用户会话开启 `org.a11y.Status.IsEnabled`）；
3. `publish()` 只上报「落在刚插入文本范围内的替换」，把接受/拒绝的决定权留给
   Core（`CoreEditObservationSink` 已经做了 generation 与范围校验）；
4. 在 `docs/linux-egui-handoff/` 记录 AT-SPI 不可用时的降级行为（应静默不弹卡）。

**现有测试**：`main.rs` 里的卡片用例都是直接调用 `queue_pending_correction()`
构造数据（`vocab_card_message`、`capsule_rebuild_rotates_card_identity_*` 等），
验证的是「卡片出现后的行为」，不能证明 Linux 真的能产生卡片。
