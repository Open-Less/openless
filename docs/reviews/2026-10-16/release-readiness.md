# beta6 / 2.0.0 发版前 Review

## 范围与结论

复核 PR #1197 的 `0454dc56..0947c05a` 整合范围、此前语音/Core/UI 独立报告，以及当前工作树全部生产代码、回归用例、版本和发布链路改动。主 Agent 完成最终复核，没有重新创建 Agent 或定时提醒。

语音 worker 显式 FIFO Shutdown、Qwen/Stepfun 启动错误解锁、TextStream 可取消进程等待、ASR 活动 owner 退役与冷删除边界均已复核。设置页/主页面退出保留、Base UI Select/Combobox/Switch/Collapsible/Popover、快捷键保存/取消、共享实验下载快照与取消、云同步/登录弹窗的惰性退出，以及 macOS 原生窗口按钮/关闭事件均在本轮范围内。没有发现尚未修复的合并阻塞项；仍须以当前 PR HEAD 的云端 CI 作为合并门禁。

## 本轮发现并修复的发布问题

1. **日期构建号会使 Homebrew 下载 404。** 原脚本从 `v2.0.0-tauri` 推导 `2.0.0`，但实际文件名含 `2.0.0+build.YYYYMMDD`。新增 `resolve-release-cask-version.mjs`，从 ARM/Intel 实际文件名读取完整且一致的应用版本，并核对 metadata-free tag；cask URL 的 tag 部分去除 build metadata，文件名保留完整版本。拒绝缺失、多余、错版本、跨架构不一致的 DMG。
2. **cask 最低系统要求与 2.0 Host 不符。** 2.0 的 Tauri 配置要求 macOS 14.0；cask 原来仍标记 Monterey。现保留 1.x 的 Monterey，2.x 使用 Sonoma，并增加与 Host minimumSystemVersion 的契约检查。
3. **Stable 资产可能未齐就对外可见。** Tauri 与 Android 原来只让 Beta 保持 draft，Stable 第一个平台完成就发布。现两个渠道都上传到共享 draft，管理员在两条资产工作流全部通过并核验后显式发布。Homebrew 对 draft 不更新；Stable 由管理员公开后自动触发独立 `sync-homebrew-cask.yml`，不依赖人工记得更新 cask，也不重跑资产流水线。仅 published latest Stable 可修改 cask，旧 tag 的手动重放不会降级。流程同步到 `RELEASING.md`。
4. **快速 Escape 可能跳过退出保留。** 全量本地浏览器回归发现 `useExitMount` 的 passive effect 尚未记住打开状态时即可关闭。改在 layout effect 中提交保留状态；Chromium/WebKit 各重复 5 次，10/10 通过。截图基线按已验收的 100% 默认值更新，不增加容差。
5. **本机 Homebrew 未管理 OpenLess。** 已接入 README 指定的官方 tap，当前公开 cask 仍是 1.3.18，不用它覆盖已安装修复版。新增并安装 zsh 事件提醒，成功执行 brew update/upgrade 后只读比较实际 App 与 cask，终端提示及同版本去重通知；无 cron/LaunchAgent，安装器幂等且备份已有 `.zshrc`。正式版公开后再由 Homebrew 接管更新。

## 本机发布前门禁

用户确认当前修复版无明显问题。只读核验 `/Applications/OpenLess.app`：

- 版本 `2.0.0-beta-a.beta.6+build.20261015-be`，签名校验通过，主程序 SHA-256 为 `7f0918b3012d273fb1ec61b3a041089c85acf9fcb04f73e5ce933bbf6076bba4`，与最终本机构建一致。
- 本机核验时生产源文件没有晚于该构建的功能改动。后续 Review 额外补充共享退出保留的 layout-effect 竞态修复及发布/提醒脚本；该 UI 修复经浏览器重复验证，没有替换正在使用的 App，不能冒称它已经进入本机旧二进制。
- 进程持续运行。日志中 2026-10-15 23:46 +0800 的两次录音均依次出现音频处理线程退出、Omni stream 完成、`phase=completed outcome=Inserted`，分别 227 与 24 字符；此前也有连续开始/停止和完整转写成功记录，没有再出现已停止录音却卡住 worker 的原故障。
- 日志曾出现启动凭据迁移重试及更新检查网络失败，不把它们解释为已消失的全局错误；后续真实录音/请求/插入已经成功。未导出转写正文或 Keychain 秘密，未重置用户配置。

结合用户验收，本机修复版运行门禁已满足。此结论不冒充 Windows 真机、Linux 产品验收或云端安装包验收；不额外挂载/发布未经独立验收的 Linux 安装包。

## 验证及发布次序

- `npm test`（含 build 与前端/契约）通过；新增日期 cask 与双渠道 draft 契约分别通过，Ruby cask 语法检查通过。
- Core lib：1204 passed、1 条显式 external Codex ignored；`local_asr_contract`：34 passed；独立 backend-tests：46 passed。
- 本机当前完整 Chromium/WebKit 用例已一次执行 100/100 通过（不带更新截图参数）。首轮 PR CI 中 macOS、MSRV、Android、Linux Core/egui/deb/rpm 已通过；UI 仅四个 Linux 快捷键旧截图失配，功能用例通过。随后排版页的四张旧 Linux 参考也被完整收集；已逐张审查 Ubuntu 实际图并更新快捷键、排版共八张对应参考；两张独立页面使用 soft 截图断言收集完整失败证据，任一失配仍让 CI 失败。后续以最终 HEAD 全量 CI 为合并门禁，不提高容差。
- 本机 `brew update` 已成功；官方 tap 已接入，提醒脚本已安装。当前 Homebrew 公开版仍为 1.3.18，未用旧版覆盖已验收修复版。
- Beta 应用版本规范为 `2.0.0-Beta.6+build.20261015`，tag 为 `v2.0.0-Beta.6-tauri`。日期为该候选准备日期，不改已安装 App。
- PR 合并、被标记提交的 CI 通过后，先构建/验证/公开 beta6；之后通过 `beta → main` PR 发布 `2.0.0+build.<正式版准备当天 YYYYMMDD>`，tag 保持 `v2.0.0-tauri`，Stable 标记 latest。

本报告记录 Review 与本机门禁结论；PR、CI、tag、资产及公开状态以实际后续交付记录为准，不将尚未执行事项写成通过。
