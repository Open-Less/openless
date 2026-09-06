# PR #1019：更新范围后的桌面复审闭环

范围依据：[2.0需求](./2.0-requirements.md)与[桌面验收清单](./2.0-desktop-acceptance.md)。Linux剩余Host/UI按[交接目录](./linux-egui-handoff/README.md)移交，不计作本批桌面功能缺陷。
第一轮固定比较：`beta@fc9824ee` → `dc350780`；审查整个PR，不仅审查文档提交。本文续记此前R01–R28之后发现的问题。

## 第一轮：新的独立团队

| 审查方 | 范围 | 确定问题 |
| --- | --- | --- |
| Windows专项 | 原生输入/热键/录音/本地ASR/进程Job/窗口与打包；对照1.x生产路径 | R32、R33、R35 |
| macOS专项 | AX/TIS/隐私/流式/原生ASR/窗口/CLI/构建；对照1.x生产路径 | R30 |
| 共享产品专项 | React IPC/事件、渠道/凭据/模型、QA/Selection/Agent、历史/词典/风格/市场/Remote | R31、R34 |
| 主代理复核 | Core主听写/停止/取消/重试/插入、需求和证据边界，逐条确认团队发现 | R29及全部问题的修复复核 |

本轮Standards未确认独立硬性违规；首轮独立审查发现7项确定问题，综合验证与修复追踪追加R36。内部审核不替代GitHub正式批准，也不证明真实设备功能全部通过。

## 问题与验证登记

| ID | 问题、基线与影响 | 修复及验证状态 |
| --- | --- | --- |
| R29 / P1 | Core停止请求在Starting等待时重新读取当前session；取消A并启动B后，旧stop会完成B并落字，违反D02会话隔离 | 已固定首次目标session；公开Core回归先红（返回B的Inserted）后绿 |
| R30 / P1 | Core流式取消未排空native write即恢复TIS，且先释放voice lease；旧CGEvent输入可能与新会话重叠。1.x先等typer结束再恢复，违反D09 | 已修复：排空后恢复、清理后释放占用；一次性落字也等待已提交效果，收尾由Host executor持有，不随stop调用方丢弃。流式/一次性/调用方drop三项先红后绿 |
| R31 / P1 | QA dismiss等待旧runtime.cancel后无条件清snapshot/HideQa；期间重开的新对话被清空，违反D10 | 已在首await前逻辑关闭，异步部分仅清旧owner；新对话/show-only/新preview三项先红后绿，全QA合同24项通过 |
| R32 / P2 | Windows普通落字恢复原目标失败后直接返回错误，遗漏1.x允许时的copy-only兜底，违反D09 | 已恢复按开关复制降级，不向新焦点粘贴；native源码合同先红后绿，既有结果映射随全Tauri测试通过 |
| R33 / P2 | Windows普通落字复用选区恢复逻辑，漏掉1.x的IsIconic→SW_RESTORE，最小化目标不能恢复，违反D09 | 已补还原后激活和原目标指纹复核；native源码合同先红后绿，真窗口证据单列 |
| R34 / P1 | 本地模型UI在Core激活前创建/启用/置顶渠道，提前修改active；模型缺失或prepare失败仍破坏原云渠道，违反D06 | 已改为Core准备成功后一次metadata提交；三runtime及缺失/禁用渠道回归先红后绿，含并发编辑共24项合同通过；前端source合同约束新生产路径 |
| R35 / P1 | Windows Less Computer窗口虽存在，设置组件、语音快捷键编辑和原生监听仍被macOS条件挡住，违反明确承诺的D12 | 已补内外层UI和原生热键；hook context按安装线程隔离，注册回执参与设置事务，迟到注册核对目标；重绑/禁用只清所属Starting/Recording slot。回归及全Tauri通过，native注入证据见下 |
| R36 / P1 | Less Computer取消会释放Capture，但capture_cancelled把不存在/被替换的lease判为未取消；冷启动迟到后可继续录音，现有headless示例亦失败 | 已将失去捕获所有权判为失效，首await前claim、context/native/转写后复验；取消标记先于资源等待，迟到事件不触碰Agent run。两类回归先红后绿，Less Computer24项及headless示例通过 |

## 本轮已取得的自动证据

- 文档提交`dc350780`：15个文档文件，链接/空白检查通过，已推送原PR分支。
- 第一轮Windows专项：183项限定Rust测试及4项Node合同通过；包含真实Job进程回归，不代表真实输入/设备验收。
- 第一轮macOS专项：4项源码/打包合同通过；主机Windows，Speech检查按平台跳过，未执行本地macOS原生测试。
- 第一轮共享产品：TypeScript/Vite与66项前端/合同测试通过；后续修复后还须重新完整执行。

### 第一轮修复后的综合验证

- Core：718 passed / 1 ignored，领域合同5/24/17/3/24/12/15/15全部通过。
- Windows Tauri：434 passed / 1 ignored；Rust1.88 `cargo check`通过。ignored为需显式触发的原生键盘注入smoke，不是普通单元测试失败。
- 前端：TypeScript/Vite构建与67个测试入口通过，含新增Windows原目标合同与更新后的Core激活接线合同。
- Linux Windows-host：48 lib + 4 host通过；真实Linux条件编译/设备效果由对应runner/设备单列，不把零运行的cfg测试算通过。
- Core/Linux严格Clippy与依赖、秘密、隔离、runtime seam、公开面、command/event基线检查通过；headless示例重新可运行。
- Windows双monitor真实smoke曾通过独立收键、关闭和重建；末次复跑`SendInput=0`未通过，记录为原生注入环境未稳定复验，不宣称重复设备验收成功。最终主听写/ASR/Agent真实设备闭环仍待对应验收。
- 文档head `dc350780` 的CI `33983624572`四平台通过；这只是修复前代码证据，新修复提交必须重新运行CI。

## 后续轮次记录

第一轮修复、综合验证并推送后，派全新的团队审查整个PR；若发现确定问题，继续修复、验证、推送，再交新的团队复审。准确head、测试计数和最终结论在完成时补录，不沿用旧head证据冒充新验证。

## 不得混同的完成条件

- 源码/自动验证：本记录跟踪确定缺陷是否全部关闭。
- Windows/macOS设备验收：按D01–D19逐项记录目标应用、录音、原生模型、权限、升级与签名证据；未执行的不填通过。
- Linux接入：Core合同与文档可交接；Linux产品侧待办独立交egui团队。
- GitHub：CI、正式review与merge gate分别记录；不自动合并、打tag或发布。
