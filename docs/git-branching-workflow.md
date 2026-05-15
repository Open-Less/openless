# Git branching workflow

## 目标

把这个 fork 固定成一套可执行、低风险的工作流：

- `origin/beta`：日常开发与集成缓冲区
- `origin/main`：稳定可发版分支
- `upstream/main`：原始项目主线，只作为同步来源，不直接在其上开发

## 一次性设置

确认 remote：

```bash
git remote -v
```

预期结果：

```text
origin   https://github.com/rackliu/openless.git
upstream https://github.com/appergb/openless.git
```

## 日常开发

每次开始新任务，固定从 `beta` 切功能分支：

```bash
git checkout beta
git pull --ff-only origin beta
git checkout -b feature/<short-name>
```

开发完成后推到 fork：

```bash
git push -u origin feature/<short-name>
```

然后在 GitHub 上创建 PR：

- base: `beta`
- head: `feature/<short-name>`

规则：

- 不直接在 `main` 上写功能程式碼。
- 不把多个独立问题塞进同一个功能分支。
- 每个功能分支只服务一个明确目标。

## 同步 upstream

同步原始项目时，不直接把 `upstream/main` 合进 `main`。先进入 `beta` 做集成和验证：

```bash
git fetch upstream --prune
git checkout beta
git pull --ff-only origin beta
git merge upstream/main
```

如果冲突较大，改用专用同步分支：

```bash
git checkout beta
git pull --ff-only origin beta
git checkout -b sync/upstream-YYYYMMDD
git merge upstream/main
```

处理冲突后，在本机完成最小验证，再合回 `beta`。

## 发版路径

稳定版只从 `main` 发，不从 `beta` 直接打 tag。

标准顺序：

```text
feature/* -> beta -> main -> tag release
```

建议操作：

```bash
git checkout main
git pull --ff-only origin main
git merge --ff-only beta
git push origin main
```

如果 `main` 不能 fast-forward 到 `beta`，不要强推；改走 PR 或手动审查差异后再合并。

## 向 upstream 提交

只有在 fork 中已经验证过的最小切片，才向 upstream 提交。

推荐顺序：

1. 功能先在 `feature/*` 完成。
2. 合入 `origin/beta` 并通过 CI / 本机验证。
3. 从最新 `upstream/main` 切一个干净分支。
4. 只挑选要贡献的最小提交或最小 diff。
5. 向 upstream 建立 PR。

这样做的目的，是把你的 fork 专属改动和可上游化改动分开，避免一次 PR 带入本地策略、实验开关或未成熟流程。

## 每次开始工作前的检查

```bash
git status --short --branch
git remote -v
git branch -vv
```

看到以下任一情况时，先停下来整理：

- 当前在 `main` 但准备改功能程式碼
- `beta` 落后 `origin/beta` 很多
- 本地有未提交修改，但准备切到另一个不相关任务
- 正准备同步 upstream，却还没先更新本地 `beta`

## 当前仓库约定

截至 2026-05-15，本仓库已完成以下设置：

- 已建立 `beta` 分支并推送到 `origin`
- 当前开发应默认进入 `beta` 或其子分支
- 已添加 `upstream = https://github.com/appergb/openless.git`