# OpenLess 的 Homebrew 同步与本机提醒

## 公开版本自动同步

两个发版流水线先把桌面和 Android 资产上传到共享 draft。管理员确认两条流水线和安装包后，通过 GitHub UI 或管理员 `gh release edit` 发布。`release:published` 自动触发 `.github/workflows/sync-homebrew-cask.yml`，仅处理已公开、latest、非 Beta 的 `vX.Y.Z-tauri`。

脚本从 ARM/Intel DMG 的真实文件名读取完整版本，检查两个架构一致且与 tag 对应，再计算 SHA-256 并更新默认分支的 `Casks/openless.rb`。tag 不带 `+build`，DMG 文件名保留日期构建号；1.x 保持 Monterey，2.x 使用与 Host 对应的 Sonoma。

权限前提：当前 `Protect #beta` 规则仅允许 OrganizationAdmin bypass，默认 `GITHUB_TOKEN` 不能直推。无人值守同步须配置仓库 Secret `HOMEBREW_CASK_ADMIN_TOKEN`，由组织管理员提供、只授权此仓库 Contents read/write；不修改分支保护，不在聊天或日志暴露值。未配置且确有更新时工作流明确失败并提示，不能把准备好文件当作已同步；cask 已被管理员更新时则幂等通过。此 Secret 是外部配置前提，不由代码自证已经存在。

手动恢复同步（不重建包）：

```sh
gh workflow run sync-homebrew-cask.yml --repo Open-Less/openless -f tag=v2.0.0-tauri
```

## 本机更新提示（zsh + Node.js）

```sh
brew tap Open-Less/openless https://github.com/Open-Less/openless
node openless-all/app/scripts/install-homebrew-openless-reminder.mjs
```

安装器保留已有 `.zshrc` 内容，首次修改先备份，重复执行不会重复插入。新终端会加载 zsh 的 `preexec` / `precmd` 事件钩子：成功执行 `brew update`、`brew update-reset` 或 `brew upgrade` 后检查 OpenLess。它不覆盖 brew、不自动替换 App、不建立 cron 或 LaunchAgent。

每次检查都会在终端显示结果；发现新版时另发 macOS 通知，同一个可用版本只通知一次。系统关闭通知时仍能看到终端结果。优先比较实际 `/Applications/OpenLess.app` 的版本，避免旧公开版覆盖手动安装的修复版；正式版高于同版本 Beta。Homebrew 命令失败时不冒称已成功检查。

也可单次检查：

```sh
node ~/.local/share/openless/homebrew-openless-notify.mjs --notify
```

确认公开版更新后执行 `brew upgrade --cask open-less/openless/openless`。尚未由 Homebrew 管理的手动安装需先通过 cask 安装接管；不要为了接管而降级当前修复版。

卸载提醒只需删除 `.zshrc` 中 `OpenLess Homebrew update reminders` 标记块和 `~/.local/share/openless/homebrew-openless-{hook.zsh,notify.mjs}`；没有后台 Agent 需要停止。
