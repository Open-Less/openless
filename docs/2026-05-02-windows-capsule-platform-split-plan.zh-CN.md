# Windows Capsule 平台分离实施清单

日期：2026-05-02

## 步骤

1. 新增通用状态 hook
   - 抽出 capsule 事件订阅和动作命令
2. 拆出非 Windows 组件
   - 把现有非 Windows 渲染迁到 `SharedCapsule.tsx`
3. 接入 Windows 独立组件
   - `Capsule.tsx` 只做 `win -> WindowsCapsule`
4. 验证
   - `npm run build`
   - `cargo check --manifest-path openless-all/app/src-tauri/Cargo.toml`
   - Windows 启动并前置主窗口

## 本轮不做

- helper-window lifecycle 重构
- QA panel 交互修复
- 业务状态流改写
