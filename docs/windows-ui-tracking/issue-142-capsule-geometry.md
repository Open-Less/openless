# Issue #142 Placeholder / 占位

## 中文摘要

本 PR 是 issue #142 的 draft 占位，专门跟踪 Windows Capsule 变形、失真与尺寸错位问题。
当前只保留问题边界、几何证据和后续修复准入条件，不引入业务逻辑改动。

## Scope / 范围

- Capsule native window bounds
- visual pill metrics
- badge position
- Windows DPI / transparent window clipping

## Evidence / 证据入口

- `openless-all/app/src-tauri/src/lib.rs`
- `openless-all/app/src/components/Capsule.tsx`
- `openless-all/app/src/lib/capsuleLayout.ts`

## Merge Rule / 合并规则

- 仅当 issue #142 的几何对齐与 Windows smoke 验证完成后才允许从 draft 转为 ready。
