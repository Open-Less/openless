# 快捷键、下拉位置与自建同步入口审查

范围：将本轮本地改动提交到 `beta`；不包含性能调优、模型修改、正式发版、标签移动或用户配置迁移。

基线：`f4ffcd42862bf6394fd6dd4a3443aadd576a9fb3`。此前本地 `7a7b872d` 与该 squash merge 的树完全一致；已将新分支基线对齐，不重复提交 #1207。

## 独立 review 及处理

初审为只读审查，无测试执行。发现并处理：

- **P1 Linux 空绑定契约不完整**：原先空翻译键无法转为 keysym，空听写键走无效 custom-key 路径，旧绑定可能保留。现将显式空绑定映射到原生 `(0, 0)` 注销；听写使用已有 `SetHotkeyRaw`，该路径清除 custom/raw/keylist 并保存插件状态。录音页与快捷键页都允许清空，保存层不再拒绝；增加转换及保存重载回归。
- **P1 视觉基线包含已删除的 Mac 听写键按钮**：更新并审查 macOS Chromium/WebKit、深浅主题共四张快捷键截图，不提高像素容差。Ubuntu 四张对应参考已从 CI `38868358498` 的实际 Chromium/WebKit 截图取得并逐张审查、更新；没有复制 macOS 字体渲染结果。
- **P2 自建同步配置在同步结束后仍禁用**：只在 `syncing` 期间做 1 秒状态轮询，防止请求重叠、退出清理轮询、忽略退出后返回值；增加确定性的 `syncing → ready` 与轮询停止回归。正常同步页不加载自建 Token。
- 原字体菜单位置测试仍断言左右展开，已改为下方对齐断言；Tooltip 的右侧解释提示保持不变。

## 检查与边界

- 原生 macOS 580 通过、4 项需模型文件测试忽略的已有证据仍适用于未更改的原生路径。
- 新增 Core 清空/保存/重载/冲突回归 4/4 已通过。
- review 修正后前端契约测试 124 个文件通过；TypeScript/Vite production build 通过；完整 macOS Chromium/WebKit 回归 **112/112**。
- 独立 follow-up 已确认所有代码问题修复，无新增代码 finding。首轮 CI `38868358498` 的 Ubuntu UI 为 108 通过 / 4 个旧截图失败；实际四张截图均符合删除重复入口后的预期，已更新，待最终 head CI 验证。
- 首轮 Linux Core 回归均通过，随后 Clippy 拒绝新测试的 `field_reassign_with_default`：测试改用 struct initializer，不禁用 lint；本地同一测试 `cargo clippy -- -D warnings` 通过。
- macOS 无法执行 Linux 主机 crate：其 egui/cpal/Wayland 依赖仅在 Linux target 声明；本地运行因此报目标依赖缺失。未把它记为测试通过，也未为此改变依赖契约，需以 Linux CI 结果为准。
- 未修改已安装 App、用户快捷键偏好、录音、历史、凭据或 TCC 授权。用户已自行采用初始本地构建：版本 `2.0.0+build.20261016`、可执行文件 SHA-256 `e6e009cb557e36d691803d1915efad5e1a7995fd5aedd2ca41e53c0480f5fad7`。
- 不提交根目录原有未跟踪 `package.json` 与 `pnpm-lock.yaml`。

## 新本地构建

源码提交 `e0a85f30a45eb9d01151f8fd6bd1e7df685c19bd` 已正常构建 macOS ARM64 App / DMG，包含同步入口迁移与 review 的生产修复。内部版本暂保持 `2.0.0+build.20261016`（发版版本另待确认）。

- 安装包：`/Users/trip/Downloads/OpenLess-local-settings-20261016-arm64.dmg`，28,977,430 字节。
- DMG SHA-256：`20515b4151a44a98243d95fabf4769ca78867bf59ac3d0dff6681ed88e853813`。
- 可执行文件 SHA-256：`9596c71b01f7ab5d93ac28d8b1e19a38daf6e033f804d4e4017ac1a9f189f8d7`。
- ad-hoc 深层严格签名、只读挂载后包内文件一致性、Speech / 麦克风描述、磁盘映像完整性及布局检查通过；临时挂载已清理。未替换用户正在运行的初始构建。

## 发版请求待确认

用户提出再次构建 Beta6 及 2.0 正式版，但指定 Beta 版本为 `2.0.0-beta-a.beta.5+build.20261016`，正式版为 `2.0.0+build.20161016`；已询问 Beta5/Beta6 与正式版日期是否为笔误。未擅自变更版本。

已公开 `v2.0.0-Beta.6-tauri`（31 项资产）与旧源 `v2.0.0-tauri` 草稿（release ID 414239763，26 项资产）均保持不变。新源码不可通过移动已有标签伪装成同一构建。版本及新标签确认前，继续 PR/CI/合并和不发布的本地构建。
