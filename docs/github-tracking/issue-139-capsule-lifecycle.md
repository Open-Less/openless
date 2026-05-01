## 现象 / Symptom

这不是单一的 click dead zone bug，而是一组已经在 Windows 实机上被观察到、且共享同一根因的 helper-window lifecycle 症状：

- click dead zone：原 Capsule 区域附近会挡住底层输入框或按钮
- screenshot selectable：截图工具仍然可以选中这块透明区域
- drag stutter：在该区域拖拽时出现明显卡顿或 compositor 异常
- lingering transparent overlay：录音结束后，Capsule 仍可能以透明顶层窗 linger

当前证据说明：这些现象不应拆成多个互不相关的问题，而应视为同一个生命周期语义偏差。

### 证据 / Evidence

运行与代码证据：

- `openless-all/app/src-tauri/tauri.conf.json:33-47`
  - `capsule` 被配置为 `transparent + alwaysOnTop + focus:false + visible:false`
- `openless-all/app/src-tauri/src/lib.rs:594-623`
  - Windows 端 `capsule` runtime host bounds 为 `220x84/118`，明显大于可见 pill `196x52`
- `openless-all/app/src-tauri/src/coordinator.rs:2398-2432`
  - Windows 端显示路径走 `ShowWindow(SW_SHOWNOACTIVATE)` + `SetWindowPos(...SWP_NOACTIVATE...)`
- `openless-all/app/src-tauri/src/coordinator.rs:2455-2479`
  - 结束阶段依赖 `window.hide()` 作为生命周期结束语义
- `openless-all/app/src/components/Capsule.tsx:278-281`
  - 前端 `idle` 只把可见内容缩成 `0x0`，真正结束仍取决于后端窗口是否已完全退出参与
- [2026-05-02-platform-lifecycle-audit.md](/D:/Users/cooper/Practice-Project/202604/openless/docs/2026-05-02-platform-lifecycle-audit.md)
  - 审计已把该问题收敛为 Windows helper-window lifecycle contract 偏差

现场证据：

- 用户已在 Windows 上观察到 dead zone / screenshot selectable / drag stutter / lingering overlay
- 这些表现与透明顶层 helper window 未真正退出 OS 参与的形态一致

### 5 Whys / 根因分析

1. 为什么会出现点击死区、截图可选中、拖拽卡顿？
   - 因为录音结束后，Windows 上的 Capsule host window 仍可能继续存在并参与桌面层级。
2. 为什么录音结束后窗口还会继续参与？
   - 因为当前实现把“生命周期结束”主要建模成 `hide()`，而不是“保证 helper window 不再参与 hit-test / capture / z-order / compositor”。
3. 为什么这个问题在 Windows 上更容易暴露？
   - 因为 Windows 的 Capsule host geometry 更大、show path 更特殊，并且是透明顶层窗；一旦 hide 语义失守，残留面积极大且更容易干扰系统行为。
4. 为什么这和 macOS 的原始设计意图不一致？
   - macOS 的原始意图是：Capsule 只在 active stage 短暂出现，结束后自然收起，不再作为前台交互对象继续存在；Windows 当前更像“视觉结束了，但 OS 对象还挂着”。
5. 为什么之前没有被门禁拦住？
   - 现有检查更多关注“窗口显示/隐藏”和几何配置，没有直接验证 inactive state 下它是否真的退出系统参与。

### 平台边界 / Platform Scope

- 直接症状范围：当前已确认是 Windows 实机问题。
- 问题层面：backend helper-window lifecycle contract + Windows native window participation。
- 全平台风险判断：根因模式不是 Windows 独有，任何透明 helper window 只要“视觉隐藏 != 生命周期结束”都可能中招；Capsule 目前是 Windows 上最先爆出来的样板案例。

### 认领 / Ownership

- owner intent：`@Cooper-X-Oak`
- 当前对应 draft PR：`#140`

### 当前状态 / Current status

- lifecycle 主线修复已完成第一波
- 人工桌面回归结果：
  - click dead zone：通过
  - screenshot selectable：通过
  - drag stutter：通过
- 当前建议：从“问题收敛中”推进到“regression review 中”

## 影响 / Impact

- 直接影响 Windows 端核心输入体验与系统交互可信度
- 会误伤底层 app 的点击、截图、拖拽，用户容易误判成其他应用故障
- 因为残留对象透明且顶层，这类问题隐蔽、难复现、难定位
- 如果不从生命周期语义修，后续即使修掉某一个 dead zone，仍可能继续遗留 screenshot / z-order / compositor 问题

## 建议接受标准 / Proposed Acceptance Criteria

- [ ] Windows 上 Capsule 的“结束”语义与 macOS 对齐：inactive 后不再继续参与系统交互
- [ ] inactive Capsule 不再造成 click dead zone
- [ ] inactive Capsule 不再被截图工具选中
- [ ] inactive Capsule 不再引入 drag/compositor stutter
- [ ] 为 Windows 增加一条直接验证 inactive Capsule non-participating 的 smoke / regression check
- [ ] 修复方案明确区分 visual state 与 host-window lifecycle state，而不是继续叠加局部 workaround

## TODO / 不确定项

- 是否需要把 `capsule hidden => no hit-test / no capture / no topmost participation` 抽成统一 helper-window contract，复用于 QA panel
- 当前 `PR #140` 建议保持 draft tracking 角色，待范围与根因完全收敛后再转 ready
建议 issue 标题：`[ui][windows] Capsule 隐藏后仍参与系统交互`
