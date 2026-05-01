## 摘要

Closes #139

这个 PR 现在从“问题收敛中”推进到“regression review 中”。

本轮已经完成：

- Windows helper-window lifecycle root cause 收敛
- `inactive` 路径的 native hide / non-topmost 收口
- 冷启动最新 debug 包回归
- 人工桌面症状回归：
  - click dead zone：通过
  - screenshot selectable：通过
  - drag stutter：通过

## 修复 / 新增 / 改进

- 对齐 PR 目标：关注 Windows Capsule helper-window lifecycle，而不是单点 dead zone workaround
- 收口 Windows 上 `visible / hidden / inactive / non-participating` 的 Capsule 语义
- 在 backend 上补齐 inactive 后的 native hide 行为，避免 transparent topmost helper window lingering
- 新增 lifecycle contract / smoke 辅助脚本，帮助后续回归持续验证
- 与 [issue-139-capsule-lifecycle.md](/D:/Users/cooper/Practice-Project/202604/openless/docs/github-tracking/issue-139-capsule-lifecycle.md) 保持同一问题口径

## 兼容

- 不包含：Capsule geometry / rounded corner / titlebar frame 纯视觉适配
- 不包含：QA hotkey / selection ask 输入源逻辑
- 对现有用户 / 本地环境 / 构建流程的影响：只聚焦 lifecycle 主线，不扩大到 UI polish 线

## 测试计划

- [x] 命令：`node openless-all/app/scripts/windows-lifecycle-contract.test.mjs`
- [x] 结果：通过
- [x] 证据路径：本地命令输出

- [x] 命令：`npm run build`
- [x] 结果：通过
- [x] 证据路径：本地命令输出

- [x] 命令：`cargo check --manifest-path openless-all/app/src-tauri/Cargo.toml`
- [x] 结果：通过
- [x] 证据路径：本地命令输出

- [x] 命令：`powershell -ExecutionPolicy Bypass -File openless-all/app/scripts/windows-runtime-smoke.ps1`
- [x] 结果：通过 launch / hotkey installed baseline
- [x] 证据路径：本地命令输出

- [x] 命令：人工桌面回归（latest debug cold start -> dictation start/stop）
- [x] 结果：点击 / 截图 / 拖拽三项全部通过
- [x] 证据路径：当前线程回归记录
关联 issue 建议标题：`[ui][windows] Capsule 隐藏后仍参与系统交互`
