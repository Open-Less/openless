# 01：Core接入合同

前置：[交接入口](./README.md)。本文只约束接入，不规定页面布局。

## 1. 可以依赖的层

`egui UI → LinuxHost / Linux Adapter → openless-core`。

- Core拥有provider协议、渠道/模型/偏好事务、会话与取消、业务路由、纠错/历史、Agent命令和结果语义。
- Linux Host拥有音频、全局热键、窗口、fcitx5、Secret Service、TLS/socket、进程与文件效果。
- UI拥有页面、编辑草稿、进度/错误展示和事件消费；不另建业务状态机。
- 不引用Tauri源码、不读取Core私有模块、不把`AppHandle`/egui类型加入Core公开合同。

## 2. 构造、启动、退出

1. Host确定显式`BackendConfig`中的data/cache/home等路径及能力；不要在测试中隐式使用真实用户目录。
2. 使用`LinuxBackendBuilder::from_shared_providers(config)?.build()?`复用生产组装；测试可用显式注入入口。
3. 创建事件订阅/Host action消费，再调用`LinuxNativeRuntime::start`；该方法内部启动Core，调用方不重复启动。
4. 检查启动快照`running`和合同版本`2.0.0`。未就绪时禁用业务操作并展示错误。
5. 从Core读取设置/渠道/模型/各领域快照，启动非阻塞UI消费；原生热键须同步保存的配置。
6. 退出时禁止新请求，取消活动会话，停止Host listener/服务并调用幂等shutdown。

现有[backend.rs](../../openless-all/app/linux-egui/src/backend.rs)、[runtime.rs](../../openless-all/app/linux-egui/src/runtime.rs)和[headless示例](../../openless-all/app/linux-egui/examples/headless_host.rs)是实际构造依据。

`from_shared_providers`和未显式注入executor的`build`必须在Host已经创建的Tokio runtime内调用；同步GUI初始化可用短作用域`runtime.enter()`包住构造，退出该作用域再`block_on`启动。builder捕获同一runtime的Handle，录音回调、静音终止和资源析构即使来自普通OS线程也能提交任务。未进入runtime时构造明确失败，不新建runtime或静默丢任务。使用自定义`LinuxBackendBuilder::new`的Host可通过`with_task_spawner`提供自己的实现，但它必须接受任意原生线程调用；Host保持executor存活直至shutdown与原生清理完成。不要用只查“当前线程runtime”的默认spawner承接cpal回调。

## 3. 公开能力与调用位置

| 领域 | 入口/来源 | Host/UI使用要求 |
| --- | --- | --- |
| 听写 | `OpenLessBackend` start/stop/cancel、snapshot | 使用Core session；不要另做Hold/Toggle/Auto/静音/重试策略 |
| 设置 | `LinuxHost::save_settings` / `update_settings_strict` | 带读到的revision；执行Core effect plan，失败按receipt恢复 |
| 渠道/凭据 | Core channel facade、`ProviderApi` | 保留channel ID与provider type区别；默认值/认证规则来自descriptor |
| 模型 | `LocalAsrApi` / `ModelStore` / activation | 用一次activation事务，不拆成UI连续写prefs和active provider |
| 历史/词典/纠错/风格包 | Core facade，[api.rs](../../openless-all/app/crates/openless-core/src/api.rs) | 通过领域方法修改；不直接写JSON或重做ZIP/文本规则 |
| QA | `services().qa` | stop使用每轮token；conversation owner不等于每轮capture token |
| Selection/Selection Voice | 对应`services()`接口 | 预览/确认/撤回/意图由Core决定；Host保存原生目标并执行效果 |
| Less Computer / Agent | 对应`services()`及Core语音入口 | 复用命令、审批、解析与取消；Host只启动/终止进程 |
| Remote Input | `services().remote_input` | Core处理认证/会话/帧序号；Host负责TLS/WSS与H5传输 |
| 能力/权限 | Core snapshot + Linux capability probe | 未实现/未授权/未知要区分，UI不能凭平台名伪造可用 |

DTO和平台Interface集中在[domains.rs](../../openless-all/app/crates/openless-core/src/domains.rs)、[ports.rs](../../openless-all/app/crates/openless-core/src/ports.rs)。

## 4. 业务不变量

- 听写、QA、Selection Voice、Less Computer共享语音互斥：Busy拒绝新会话，不抢占旧会话。
- provider/channel/model与上下文按会话冻结；只有已有合同允许的停止时翻译切换可更新该轮。
- Core按实际管线解析渠道：Omni不依赖传统ASR/LLM，Less/Selection录音只依赖ASR，QA文本不依赖ASR。Raw允许没有可用LLM；若停止时改为翻译，Pipeline使用冻结的LLM或`deferred_llm_error`，不会偷偷切到录音期间新增/启用的渠道。
- 停止、取消、timeout、设备fault与迟到结果必须保持单一终态，不能让旧任务改变新session。
- 逻辑取消会立即使本代token失效；原生初始化、stop或ASR清理仍在途时，Core资源hold继续阻止新语音。收到取消终态不表示已经可以绕过Core强行打开另一个麦克风。
- 自定义`DictationEngine`须原样接收/转发`start_voice_capture`与`start_audio_capture`的`CancellationToken`；ASR启动返回后、开麦前及原生初始化完成后均检查它，迟到句柄必须关闭。生产factory已接好，不要替换成一个永不取消的新token。
- `Inserted`、`PasteSent`、`CopiedFallback`、`NotRequested`和错误`OutcomeUnknown`不可互换。结果未知不得自动再插一次。
- 流式尾段协调、纠错执行顺序、历史/统计归因留在Core；Host只回报真实效果。
- 插入`begin()`本身也可能切换输入源。Core在准备前登记同一可等待结果，取消/丢弃调用方不能跳过尚未完成的原生恢复。
- `TextInserter::capture_target()`在听写认领后、上下文/凭据等待和反馈前同步调用；焦点敏感Host返回仅持本轮原生目标的插入器，异步`begin()`再准备输入源。原生句柄不进入Core DTO，无需插入时不捕获；不依赖焦点的Adapter保留默认`None`。Linux Host接入原目标快照时应覆写此入口，不能等凭据读取后重新抓当前焦点。

## 5. 秘密和数据

- 凭据秘密只通过`CredentialStore`/`SecretValue`传递；通用状态、事件、日志和可序列化UI快照不包含API密钥、OAuth access/refresh token或PIN。配对页面通过显式`read_pairing_pin`受控显示PIN，不广播到通用状态。
- 审批票据（如`Approval.token`、`pending_approval_token`）是Core业务合同的一部分，不是上述凭据秘密；UI须按原合同接收、关联并回传，不能自行伪造。
- Linux已经提供旧凭据解码/迁移起点；只有显式home目录才触发旧来源访问，完成标记最后提交。
- 设置revision冲突后重新读快照合并草稿，不无条件重放旧整表。
- 1.x数据格式兼容与runtime `2.0.0`握手是两回事：继续读旧数据，不接受旧runtime合同冒充已就绪。

## 6. 接口缺口的回报

提交：用户操作、已有公开入口、缺少的业务结果/Host效果、期望事件/错误、最小复现或fixture。
Core负责人修共享规则/接口；egui团队修平台适配/界面。详情按[缺口登记](./02-gap-register.md)分类，不能通过公开私有内部对象绕过。
