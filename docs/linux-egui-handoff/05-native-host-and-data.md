# 05：原生宿主、数据和系统集成

对应 L02、L03、L10、L11。平台效果归 Linux Host；现有代码可复用，具体缺口如下。

## 1. 音频、存档和反馈

现有[audio.rs](../../openless-all/app/linux-egui/src/audio.rs)提供 CPAL 设备、PCM 转换、音量和致命错误回调；Core决定静音停止和重试策略。

- `LinuxCpalRecorder::start`目前只消费计划中的设备名称，没有执行录音期间系统静音/恢复。
- `LinuxActiveRecording`没有覆写`archive()`；Core默认返回None。因此录音归档、失败归档恢复、同归档静默重试的音频来源尚不完整。
- 补充[RecordingPlan](../../openless-all/app/crates/openless-core/src/dictation_context.rs)的适用效果及[RecordingArchive/read_pcm](../../openless-all/app/crates/openless-core/src/ports.rs)；`archive_enabled=false`（QA/划词语音）时不得创建临时归档，不能用成功后删除代替不落盘。主听写/Less Computer 的保存与清理须遵循独立音频保留设置，不能误用历史条数。
- 用户取消、设备断开、启动失败和正常停止都必须恢复已改变的系统音量；不要覆盖录音期间用户主动修改的新状态。
- 提示音、录音胶囊/overlay尚未接入；消费Core阶段/反馈事件，避免UI自己猜测处理已结束。
- Less Computer成功非debug录音的归档清理由Core执行；Host需提供真实`RecordingArchive`和可执行的`discard`。未提供archive不等于已经验证文件保留策略，失败/debug场景也不能无条件删除。

验收：真实输入设备切换/拔出、无声停止、权限失败、归档开关/上限、失败重试与取消、历史录音播放/重转、各终态音量恢复。

## 2. 输入目标、上下文和手改观察

[fcitx5.rs](../../openless-all/app/linux-egui/src/fcitx5.rs)已有ticket化插入、失败复制与结果区分；[selection.rs](../../openless-all/app/linux-egui/src/selection.rs)已有捕获/应用/取消/撤回和原目标核验。

**确定缺口**：Linux factory 未覆写默认`NoopHostContextAdapter`与`NoopEditObservationAdapter`，选区`source_app`仍为None。
接入[HostContextAdapter与观察合同](../../openless-all/app/crates/openless-core/src/ports.rs)，采集平台能够可靠提供的应用身份和授权上下文，报告无法提供的能力。

- 关闭光标上下文时不读取文档；必要前台应用元数据与文档内容分开处理。
- 记录开始时固定原目标，不因主窗获得焦点而改成当前窗口；迟到观察须核验generation/session。
- 不把PRIMARY旧值或剪贴板副本当成当前原控件仍匹配的证明。
- Core继续拥有流式尾段协调与纠错规则；Host不能因未知结果再粘贴一次。

需在X11/Wayland各自的GTK、Qt、浏览器、终端验证；组合环境不支持可靠替换时明确降级/禁用，不伪造已插入。

## 3. 凭据和1.x数据

[credentials.rs](../../openless-all/app/linux-egui/src/credentials.rs)已有Secret Service、channel metadata与旧凭据迁移；[backend.rs](../../openless-all/app/linux-egui/src/backend.rs)仅在显式home路径下启用旧来源访问。

- 复用CredentialStore，不在UI读取vault数据库、推导账号名或保存明文密钥。
- 旧来源读取、目标写入、完成标记的顺序必须可重入；失败保留来源，不能凭“开始迁移”删除旧数据。
- 只操作OpenLess所属账号/命名空间；测试用独立data/cache/home和测试keyring，不触碰真实凭据。
- vault锁定/拒绝/不存在分别展示，保存失败不可显示成功；渠道删除不能误删其他渠道的秘密。
- 旧模型和自定义目录迁移复用Core ModelStore，不由UI复制目录并猜测ready状态。

## 4. 模型、进程与Remote

| 已有实现 | 需要保留/验证 |
| --- | --- |
| Linux Generic Qwen runtime、Core下载/激活事务 | 包内runtime路径、真实推理、timeout/cancel和旧释放不卸载新模型；Foundry/MLX不在Linux移植要求内 |
| Linux ProcessAdapter | 使用Core AgentCommand；验证桌面PATH、stdin背压、取消后进程组清理、审批/拒绝与自然结束 |
| Remote TLS/WSS、H5、Core配对/音频桥接 | 多连接隔离、首帧、停止时仍可取消、断线恢复、端口占用、手机证书信任；不要另写认证或会话状态机 |

这些模块有生产实现；缺真实凭据、模型或设备结果时写“待实测”，不要整模块标“未实现”。

若Host实现驻留模型缓存，须在`ModelRuntimeAdapter::claim_lease/preload_lease/release_lease`中原子维护模型与激活代次；同属Generic不代表同一缓存。普通使用应撤销旧激活的释放权，迟到加载不能覆盖新缓存，也不能取消仍有效的旧会话。Core负责激活失败补偿与metadata提交，Host不得仅按模型ID查询后再无条件释放整个runtime。

## 5. 系统集成仍需实现

现有[main.rs](../../openless-all/app/linux-egui/src/main.rs)将tray能力传为false；通知仅显示状态栏，`RequestRestart`仅提示手动重启，缺自启与实际更新器。
[capabilities.rs](../../openless-all/app/linux-egui/src/capabilities.rs)按AppImage布局判断`supports_auto_update`，这只是能力判定，不能当作更新服务已完成的证据。

egui团队补窗口/托盘/通知/自启，以及AppImage检查、验证、下载、替换和重启；系统包遵循其分发方式，不硬套AppImage更新流程。直到接线完成，UI不得依据此flag展示假可用操作。
缺fcitx5插件时当前程序进入启动失败页；产品可用性方案须明确安装/重载指引，若选择允许配置模式则只开放不依赖该插件的操作。
