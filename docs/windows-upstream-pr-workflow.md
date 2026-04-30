# Windows upstream PR workflow

## 目标

Windows 主线先在 `fork/dev` 完成发现、修复、CI、自审和复审，再收敛成明确 upstream 维护项。不要把未收敛的真机 findings 直接写到 upstream issues 或 upstream PR。

## 标准流程

1. 在 `fork/dev` 修复问题。
   - 每个提交只解决一个明确问题。
   - findings 先写到本地记录或 fork issue。
   - 不向 upstream 新增噪声 issue。

2. 在 `fork/dev` 触发 CI。
   - Windows build 必须过。
   - 新增/修改的 Windows smoke 必须能在本机复跑。
   - 真实凭据、物理热键、ASR、插入 fallback 等不能完全 CI 化的项目，要留下本机证据路径和日志摘要。

3. 在 fork 上开自有 PR。
   - base: `fork/dev`
   - head: 功能分支
   - PR 描述使用中文，按模板填写。
   - PR 必须包含 fork CI 链接、真机回归摘要、自审结论。

4. 复审 fork PR。
   - 先按 code review 方式找阻断项。
   - 修完 review findings 后再次跑 fork CI。
   - 只有 fork PR 复审通过，才能进入 upstream 收敛。

5. 收敛 upstream 维护项。
   - 从 fork PR 中拆出最小 upstream 维护切片。
   - upstream PR 只包含已验证的最小改动。
   - upstream PR 描述必须带 fork PR / fork CI 链接，说明该切片来自已验证的 `fork/dev` 工作流。
   - upstream issue 只用于已经确认、可维护、可复现、需要 upstream 跟踪的问题；不要把探索期 findings 扔到 upstream。

## upstream PR 进入条件

- `fork/dev` 已包含修复。
- fork PR 已通过 CI。
- fork PR 已完成自审和复审。
- upstream 分支从最新 upstream base 切出。
- upstream diff 能独立解释，不依赖 fork/dev 的其他未提交上下文。
- PR 描述包含：
  - 单一目标
  - 不包含范围
  - fork PR 链接
  - fork CI 链接
  - 本机 Windows 回归证据

## 禁止项

- 禁止从未验证的本地 finding 直接创建 upstream issue。
- 禁止绕过 fork/dev CI 直接推 upstream PR。
- 禁止把多个 Windows 真机问题混成一个 upstream PR。
- 禁止在 upstream PR 中提交真实服务凭据、用户本地配置、构建产物或临时目录。

## 当前执行规则

后续 Windows 主线默认顺序为：

```text
fork/dev 修复 -> fork/dev CI -> fork PR -> 自审/复审 -> upstream 最小 PR
```

如果 upstream PR 需要更新，先确认对应 fork PR 和 fork CI 证据，再同步 upstream PR。
