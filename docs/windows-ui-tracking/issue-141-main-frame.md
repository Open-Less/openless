# Issue #141 Placeholder / 占位

## 中文摘要

本 PR 是 issue #141 的 draft 占位，专门跟踪 Windows 主窗口圆角贴合与外框不完整问题。
当前只沉淀调查结论、证据入口和后续修复范围，不引入功能性改动。

## Scope / 范围

- Windows main window
- frameless / custom chrome
- Mica / transparent / shadow
- first-paint visual consistency

## Evidence / 证据入口

- `openless-all/app/src-tauri/tauri.conf.json`
- `openless-all/app/src-tauri/src/lib.rs`
- `openless-all/app/src/components/WindowChrome.tsx`

## Merge Rule / 合并规则

- 仅当 issue #141 的根因修复、验证和回归检查完成后才允许从 draft 转为 ready。
