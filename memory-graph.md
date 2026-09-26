# Memory Graph — OpenLess

- slug: openless
- path: F:/编程/openless
- updated: 2026-09-20

## Summary

Tauri 2 跨平台 AI 语音输入与选区助手，前端使用 React / TypeScript，原生层使用 Rust。

## Entities

- OpenLess (Project): 唯一工作副本为 F:/编程/openless。
- LiveTranscriptPill (Module): 共享实时转写胶囊，控制文字位置、入场和外框尺寸。
- insertTextAnimation (Module): 字素差分、传播延迟和胶囊目标几何的纯函数模型。

## Relations

- Capsule / TypelessCapsule --renders--> LiveTranscriptPill。
- LiveTranscriptPill --uses--> insertTextAnimation / framer-motion。

## Facts

- 文字使用稳定字素 key；水平位置以右边缘为锚，独立于垂直入场动画。
- 新字从下方淡入并轻微上弹；字距 1.4px，旧字按新增批次分为 2–4 字组向左传播，组间 26ms，传播延迟上限 96ms。
- 胶囊回中比左扩晚 140ms 启动，允许中途偏左，最终居中。
- 文本可视区域跟随实时胶囊宽度，长文本保留右侧并裁剪左侧。
- scripts/insert-text-motion-h5.mjs 在独立无头 Chrome 中验证轨迹、回中、快速纠错、长文本和减少动态效果。

## Decisions

- 流式原文默认开启，字号默认14px、范围12–20px；通过UserPreferences保存并由prefs:changed即时同步。
- 原文仅接收transcript_delta，忽略润色输出；关闭后保留波形与状态，原生窗口尺寸不变。
- 浏览器验证不能替代打包后的原生窗口体验验证。
