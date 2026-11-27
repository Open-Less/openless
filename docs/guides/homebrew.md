# Homebrew 更新提醒

在 macOS 上，用 Homebrew 管理正式版的用户可以安装本机更新提醒。从仓库根目录执行：

~~~sh
brew tap Open-Less/openless https://github.com/Open-Less/openless
node openless-all/app/scripts/install-homebrew-openless-reminder.mjs
~~~

打开新终端后，`brew update`、`brew update-reset` 或 `brew upgrade` 成功时会检查 OpenLess。发现新版会在终端显示，同一个版本只发送一次系统通知。提醒不会自动安装应用。

确认要升级后执行 `brew upgrade --cask open-less/openless/openless`。手动安装的版本改由 Homebrew 管理前，先核对版本，避免把新版换成旧版。

卸载提醒时，从 `~/.zshrc` 移除 `OpenLess Homebrew update reminders` 标记块，再移除 `~/.local/share/openless/homebrew-openless-hook.zsh` 和 `~/.local/share/openless/homebrew-openless-notify.mjs`。
