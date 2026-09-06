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

### 第二轮：`fc9824ee...98a0ca38`

两名全新Windows/macOS审查员交叉覆盖整个PR的原生路径、React入口与共享领域，主代理补充复现及修复复核。不是仅审查第一轮补丁；本轮确认以下9项Spec问题，未确认独立Standards违规。

| ID | 确定问题 | 修复/验证状态 |
| --- | --- | --- |
| R37 / P1 | Less Toggle/Auto松键后Esc仅清Core，Host Recording slot常驻导致永久无法再开；关闭面板也未清原生捕获 | 已统一Esc、关闭、胶囊及CLI取消的所属capture收尾；Core CLI回归先红后绿，保留QA独立作用域；Host可取消后复录 |
| R38 / P2 | Less桥丢弃原生Instant，在ASR冷启动排队后重建now，把Auto短按当成长按 | 已透传modifier/combo真实Instant及generation；450ms启动+50ms短按回归先红后绿 |
| R39 / P2 | 录音控制ready-check与pending.push之间可被attach/flush穿过，静音Stop永久丢失 | 已固定pending→slot锁序；Starting保存同一control，静音和胶囊Stop交接前后恰执行一次 |
| R40 / P2 | Less录音、电平与转写没有生产事件，Composer只等旧operating胶囊状态，Windows无glow可代替 | 已接Core typed VoiceState、React/胶囊；保留一条带原seq/session的有效投影供截断重放恢复，拒绝旧终态覆盖新录音；语音忙时保留草稿且不提交 |
| R41 / P1 | Less成功非debug语音未删除WAV，回退到共享Core后丢失1.x成功归档清理 | 已按成功/非debug条件discard，debug与失败保留；录音保留策略回归先红后绿 |
| R42 / P1 | text_inserter.begin尚未登记时取消会释放voice lease；迟到TIS准备/恢复干扰新会话 | 已在begin前登记Shared preparation并持hold，取消与迟到start共同等待一次恢复；原回归和丢弃start回复回归通过 |
| R43 / P1 | 会话中关闭cursor_context_enabled后，完成仍按冻结true重启AX编辑观察 | 已与设置事务串行并读当前开关，普通/流式两分支回归先红后绿 |
| R44 / P1 | QA/Less先release gate再等待native stop，旧mute恢复可破坏新录音；冷ASR取消后仍可开mic | 已分离逻辑取消与资源hold，初始化/stop/ASR清理完毕前Busy；recorder前检查token，已提交任务持有收尾；也覆盖主听写无插入、初始化/停止/取消回复被丢弃与shutdown清理 |
| R45 / P2 | QA Completed/Failed/Cancelled被`phase != Idle`误判仍在使用语音，阻止Less启动 | 已只阻止真实活动阶段，三个QA终态保持面板打开也可启动Less；公开回归通过 |

第二轮问题全部修复、测试并推送前，不将`98a0ca38`标为最终闭环；下一轮须由新的审查团队复审。

第二轮定向证据：Windows Host最终8项通过；Core生命周期10项（含最后CLI回归）、typed VoiceState合同及最近投影回归通过；插入准备、隐私、丢弃回复的回归通过。曾挂起的旧stop测试因owned startup首次poll前使用notify_waiters丢通知，已改为保留permit的notify_one，单项再次通过；不是跳过测试。

第二轮综合验证：Core 722 passed / 1 ignored，生命周期10项、其它合同6/24/17/3/24/12/15/15全部通过；Windows Tauri 440 passed / 1 ignored；前端构建与67个测试入口通过；Linux Windows-host 48+4、headless示例、Core/Linux严格Clippy、依赖和公开合同检查通过。MSRV与最终远端CI按精确修复head记录。`98a0ca38`的CI `34002890692`四平台通过，但不能代替第二轮修复提交的CI。

第二轮修复提交`c0807d97`已推送；CI `34006442463`的Windows、macOS、Linux、Android全部通过。Linux artifact按PR条件跳过，不算产物验收。

### 第三轮：`fc9824ee...c0807d97`

两名新的独立审查员交叉检查桌面原生路径与整个共享Core，主代理复现并处理完成回调竞态。本轮确认以下6项Spec问题，未确认独立Standards违规。

| ID | 确定问题 | 修复/验证状态 |
| --- | --- | --- |
| R46 / P1 | Selection Voice通用Esc/故障/shutdown只取消Core状态，Host麦克风与slot仍存活；冷启动也可能留下目标owner | 在首await前绑定既有RecordingControlSink，所有终止入口只调用同一所属capture清理；同步撤销Starting目标，原生stop/ASR清理完成前保留resource hold。Core与真实Tauri Host seam先红后绿 |
| R47 / P2 | QA Completed/Failed/Cancelled仍拦截通用取消路由，Selection Voice不能收到Esc | 路由仅匹配Recording/Thinking/AwaitingApproval；三种QA终态与Selection并存的公开回归先红后绿 |
| R48 / P2 | Less在Agent启动前遇到recorder.stop、ASR.finish或空转写错误，仅回Idle/日志，没有可见错误 | 复用capture_fault原子终态认领，发布一次安全错误且返回脱敏错误；用户取消不报Error。各错误及取消排除回归先红后绿 |
| R49 / P2 | Host翻译标志可跨按钮/CLI/静音停止泄漏到下次会话；Starting捕获上下文期间的翻译请求又会丢失 | 删除Host重复状态，Core保存当轮意图并在所有停止入口统一应用冻结上下文，显式stop override优先；冷启动、三种停止入口与失败补偿回归通过 |
| R50 / P2 | Host重建Core胶囊payload丢失warming等字段，native start返回后首PCM未到却显示已就绪 | 透传完整payload；兼容缺省的recordingReady仅在首个实际PCM回调置真，零电平/0ms首帧亦有效；Less遵循同一首帧语义。Core、Host和事件桥回归通过 |
| R51 / P1 | A发布Completed后等待设置锁，期间A取消且B启动；A迟到回调会清B状态并为旧文本注册编辑观察 | 观察注册在锁后复核session，迟到原生注册按generation撤销；完成复用按session复位并检查反馈归属。确定性并发回归先红（B被置空）后绿 |

第三轮综合验证时，新增wire字段暴露canonical fixture未同步、旧热键source合同仍绑定被删除的Host路径；同步实际合同而不跳过断言。完成回调并发测试改用多worker测试runtime，保持Core生产runtime边界检查不变。

第三轮最终本地验证：Core 725 passed / 1 ignored，生命周期14项、其它合同6/24/17/3/24/12/15/15全部通过；Windows Tauri 444 passed / 1 ignored；前端构建与67个测试入口通过；Linux Windows-host 48+4、headless示例、Core/Linux严格Clippy、Rust1.88 Tauri检查、依赖/秘密/隔离/runtime/公开面/command-event检查均通过。修复提交的远端平台CI与第四轮新团队结论仍单独记录，不把本地验证表述为macOS设备验收。

### 第四轮：`fc9824ee...90cbd014`

新的Windows/macOS审查员再次审查整个PR，主代理复核共享合同和数据接线。本轮Standards未确认独立违规；确认以下3项Spec问题。第三轮提交已逐Git对象核对SHA后上传至原PR，未重写历史。

| ID | 确定问题 | 修复/验证状态 |
| --- | --- | --- |
| R52 / P2 | 市场服务在构造时独立保存HTTP client，绕过共享net代理缓存；关闭useSystemProxy仍使用系统代理，且运行中切换不生效。1.x市场/OAuth使用共享client | 已删除固定client和重复builder，逐请求URL复用共享net缓存，仍禁止重定向并为loopback直连；独立进程假代理回归先红后绿，覆盖启动关闭、同服务true→false→true及混合配置OAuth回环 |
| R53 / P2 | 取消A的原生清理已完成并释放资源，但A调用方尚未恢复时B可以启动；A迟到回复无条件发送HideDictationFeedback。其他失败/shutdown出口也缺归属检查 | 已统一所有Hide出口的session守卫，检查和同步Host enqueue保持同锁；手工延迟cancel回复的公开回归先红（Hide覆盖Show）后绿，R51回归保持通过。Tauri当前该HostAction是no-op，故这是共享Core合同问题，不声称已复现桌面胶囊故障 |
| R54 / P1 | Remote鉴权在获取lifecycle锁前读取旧PIN；轮换持锁期间已读取旧值的鉴权等待后可在新服务代次放行，生产WS没有第二次PIN检查 | 已将PIN读取置于与轮换相同的生命周期临界区；公开接口并发回归先红（Ok，预期BadPin）后绿，旧PIN被拒、新PIN仍可认证 |

第四轮最终本地验证：Core 726 passed / 1 ignored，生命周期14项及其它合同6/24/18/3/24/13/15/15通过；Windows Tauri 444 passed / 1 ignored；TypeScript/Vite与67个前端测试入口、Linux Windows-host 48+4、headless示例、Core/Linux严格Clippy、Rust1.88 Tauri检查、六个架构/安全/兼容基线检查和定向格式检查均通过。独立修复复核确认R53全部10个原有Hide出口共用归属判断，没有复制遗漏。此提交后继续第五轮全新团队审核。

后续独立团队的最终结论和新head CI写入原PR描述；本文保留已复现问题及修复证据，不预填尚未完成的审核或设备结果。

## 不得混同的完成条件

- 源码/自动验证：本记录跟踪确定缺陷是否全部关闭。
- Windows/macOS设备验收：按D01–D19逐项记录目标应用、录音、原生模型、权限、升级与签名证据；未执行的不填通过。
- Linux接入：Core合同与文档可交接；Linux产品侧待办独立交egui团队。
- GitHub：CI、正式review与merge gate分别记录；不自动合并、打tag或发布。
