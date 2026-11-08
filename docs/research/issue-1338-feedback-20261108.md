# #1338 A2：点击语音编辑「打开面板」后的修复尝试

## 证据与范围

- 只读附件 `openless-2026-11-08T11-41-24-2.log`，108813614字节；SHA256 `cd1a2e1d15a4759fa7e1fef06672154bf21feef2c95881ee5cd1643d87c4de33`，与无`-2`附件哈希一致。禁止改写附件。
- 包含8个运行的历史诊断，不能把相邻运行拼成因果链。A1构建`b743c50abc2d9e04f59eaeee2fa3e124ba470331`确实运行过。最终checkpoint属于重启后的`r18e60f1163561fa8`，不是故障候选运行的完整性担保；前端发送、应用与尾部捕获仍unknown。
- 用户明确反馈：点击设置中语音编辑会话右侧「打开面板」必现。此前确认异常时语音输入和选区助手仍可用。此为用户报告的稳定触发/结果，不伪称开发机已复现。
- `r18e60f01dcfc6bec`：L119910–119917有close_requested/destroyed，窗口类别未记录，不能断定关闭了哪个窗口。L119924–119955 P4操作237/240/243/246记录requested与dispatch_returned=ok，无entered/returned；对应P2父操作236/239/242/245仍acquired/released，后台继续运行。记录期内dropped计数0，不等于该历史运行尾部完整。语义SUSPICIOUS，不能宣称已证明整个主线程永久死锁。
- 目前命令不在P1 allowlist，缺少本次点击与上述历史运行的直接operation关联；补对应入口/创建/显示边界，不伪造旧记录关联。

## 正常契约与语义检查

- N7：主窗口点击打开面板后，应完成面板创建/显示并保留主窗口IPC可用；voice-edit session仍由面板后续start_voice_edit_session启动，非main调用保持拒绝。来源：用户期望、SelectionWorkspaceSection.tsx、commands/voice_edit.rs注释及window标签校验。用户报告VIOLATION；旧日志具体点击阶段INSUFFICIENT_EVIDENCE。
- N8：接受的主线程任务不应因窗口创建重入而使后续工作无进展；派发成功只证明入队，不证明执行。来源：Tauri宿主派发契约。上述P4为SUSPICIOUS，重启/关闭及尾部截断替代解释未排除。
- N2原设置/权限请求应继续完成；用户报告VIOLATION，现场未直接记录每个页面Promise。
- E1：A1内置导出携带专用NDJSON和当前checkpoint已观测成立（L120835）；NO_VIOLATION_OBSERVED，仅限这次导出，不代表原故障修复。

## 修复前假设

| ID | Hypothesis | Falsifiable prediction | If true, what change would make the bug disappear? | Probe IDs |
|---|---|---|---|---|
| R2-H1 | Windows同步IPC命令中懒创建WebView发生重入/同步等待（N7/N8） | 打开命令进入后创建阶段无返回；异步分派同一命令后创建、显示返回且主窗口请求恢复 | 仅把voice_edit_window_open改async，不变校验、会话、窗口参数 | P1/P2/P4 |
| R2-H2 | 窗口已创建，Windows focus/show阶段阻塞（N7/N8） | 创建returned而focus或show无returned；H1变更后同样停在该阶段 | 仅调整证实的显示阶段执行边界 | P2/P4 |
| R2-H3 | 窗口关闭/销毁后的宿主事件队列生命周期异常（N8） | 对应窗口生命周期与后续任务不执行相关，创建显示此前完成；需固定窗口类别观察 | 仅修证实的宿主句柄生命周期，不重建整个后端 | P4/P6 |
| R2-H4 | 后端完成打开而主窗口前端readiness/请求应用挂起（N2/N7） | 创建显示完整返回，原主窗口页面Promise仍挂起；需前端补证 | 仅修已定位页面/桥生命周期，不盲目重试 | P1/P2/P4；frontend仍unknown |

## 修复资格与单变量实验

- 锁定依赖为src-tauri/Cargo.lock中的Tauri2.12.4。官方对应tag源码：`https://raw.githubusercontent.com/tauri-apps/tauri/tauri-v2.12.4/crates/tauri/src/webview/webview_window.rs` L58–59、L115–116明确警告Windows同步命令/事件处理器创建WebviewWindow会deadlock，并推荐async command或独立线程。
- 当前实际路径：SelectionWorkspaceSection点击 -> openVoiceEditWindow -> voice_edit_window_open（同步）-> TauriCoordinatorHost::show_voice_edit -> show_voice_edit_window -> ensure_voice_edit_window -> WebviewWindowBuilder::build。主窗口标签限制仍保留。
- R2-H1=INCONCLUSIVE causal confidence / ACTIONABLE reversible experiment：用户稳定触发 + 锁定API已知重入约束 + 现场主线程派发无进展而后台锁仍运行，支持针对这一条边界做单变量实验；不是宣称其他原因已排除。R2-H2/H3/H4仍INCONCLUSIVE。
- attempt_id=A2-voice-edit-open-async；change_id=C2-async-command-dispatch。仅命令sync->async改变业务执行边界；增加既有P1/P2/P4内的闭合阶段，不新增P7，不改超时/重试/锁顺序/窗口参数/权限/会话提交。
- 测试先编写，运行仅Actions；未执行本地测试、编译、安装或应用启动，不声称观察到red/green。
- 下一门禁：完成候选→review-diff-bugs完整复审至no bug found→提交/push fork→准确SHA Actions CI/桌面/移动端→交付新包固定触发复现。继续同一个session NDJSON，保留历史；用户确认修复前不清理。
