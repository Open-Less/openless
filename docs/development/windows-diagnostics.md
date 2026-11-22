# Windows 无响应诊断

需要复现无响应或静默退出时，在 [Release (all platforms)](../../.github/workflows/release-tauri.yml) 手动选对应分支，设 `platform=windows`、`windows_hang_diagnostics=true`，留空 `release_tag`。下载 `openless-windows-x64-issue-1266-diagnostics` 的安装包、原生 EXE、匹配的 PDB 与两个 provenance 清单。清单记录源码提交、构建身份和 EXE/PDB 内容指纹。普通包默认关闭此诊断功能。

日志追加到 `%TEMP%\openless-issue-1266-2e2c7a47-session.ndjson`。按构建、运行和操作身份对齐设置锁、主线程派发、热键处理、输入法提交、录音停止及窗口退出。派发返回、闭包执行和用户看见的文字是三个独立证据。实际结果未知时不重放外部写入。

在可用的 Tauri 开发者控制台可调用 `window.__TAURI_INTERNALS__.invoke('issue1266_health')` 读取队列丢弃、写入和错误计数。卡死时控制台也可能没有响应。日志缺行时记录证据覆盖情况。

保留故障发生时间、是否仍有进程、主界面/胶囊/托盘/热键各自的反应。确有需要时私下采集同一进程的内存转储并使用匹配符号。普通应用日志、内存转储和录音可能包含敏感内容，交付前核对范围。历史调查见 [#1266](https://github.com/Open-Less/openless/issues/1266) 和 [#1227](https://github.com/Open-Less/openless/issues/1227)。[诊断实现](../../openless-all/app/src-tauri/src/issue1266_debug.rs)
