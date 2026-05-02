# Windows Capsule 平台分离设计

日期：2026-05-02

## 目标

同一个产品目标：

- 录音胶囊在出现、处理中、结束时都稳定可读
- 用户只看到胶囊本体，不看到宿主矩形
- 状态切换不裁切、不变形、不撞边

不同 OS 使用不同承载手段：

- macOS / 非 Windows：保留现有通用胶囊实现
- Windows：单独使用 Windows 原生承载思路和独立组件

## 设计决策

### 决策 1：`Capsule.tsx` 只做平台路由

- `win -> WindowsCapsule`
- 其他平台 -> `SharedCapsule`

`Capsule.tsx` 不再负责布局、状态订阅或视觉分支。

### 决策 2：状态数据共享，视觉承载分离

共享层只保留：

- `CapsulePayload`
- `CapsuleState`
- Tauri 事件订阅
- `cancel_dictation` / `stop_dictation`

平台层各自负责：

- 宿主尺寸
- 可视 pill 尺寸
- processing / error / done 布局
- 状态切换动画和裁切策略

### 决策 3：Windows 只显示 pill，不显示宿主矩形

Windows 宿主窗口继续存在，但视觉上应完全透明。

Windows 组件只允许渲染：

- 左右动作按钮
- 中间 processing / error / done 内容
- 可选 translation badge

不允许继续暴露一层白色矩形宿主框。

## 文件边界

- `openless-all/app/src/components/Capsule.tsx`
  - 平台路由入口
- `openless-all/app/src/components/WindowsCapsule.tsx`
  - Windows 独立组件
- `openless-all/app/src/components/SharedCapsule.tsx`
  - 非 Windows 胶囊实现
- `openless-all/app/src/components/useCapsuleState.ts`
  - 通用状态数据 hook
- `openless-all/app/src/lib/capsuleLayout.ts`
  - 平台尺寸数据

## 本轮验收标准

- `Capsule.tsx` 只剩平台路由
- Windows 独立使用 `WindowsCapsule.tsx`
- Windows `thinking / error / done` 不再复用通用外观层
- `npm run build` 通过
- `cargo check --manifest-path openless-all/app/src-tauri/Cargo.toml` 通过
