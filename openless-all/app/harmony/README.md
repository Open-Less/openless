# OpenLess 鸿蒙前端宿主

ArkTS / ArkWeb Stage 工程，将 React 前端离线打包进 HAP。
已通过 ArkWeb JavaScript Proxy → ArkTS → Node-API → Rust C ABI 接入 `openless-core`。
已接通启动快照、能力查询、设置读取及字段事务写入、历史及活动统计、词典增删改和启用状态、纠正规则读取、风格列表及启动视频标记。
数据使用应用沙箱真实文件存储，鸿蒙 IPC 不回退到浏览器模拟数据。未实现命令明确报错。
已接通系统麦克风枚举、授权查询和 PCM 音量监测；支持提供商目录、渠道管理、模型列表和连接测试。凭据通过鸿蒙 Asset Store 加密保存，启动时恢复到 Rust 内存适配器；渠道元数据独立持久化。
网络状态沿用桌面端的真实 HTTP 探测；外部链接交给系统浏览器，WebView 拦截外站导航。调试日志通过系统文件选择器导出。
已接通输入法 AudioCapturer → PCM 原生桥 → 核心 ASR/润色管线 → 结果预览 → InputClient 确认写入。真实联网识别仍需配置可用渠道后验收。

## 当前完工边界

风格市场 ZIP 下载、渠道测试结果记录、词库建议接受/拒绝和普通云备份状态/上传/恢复/删除接口已接通。历史 WAV 读取、重识别、重新润色、历史删除及清空、纠错规则新增/删除/启停、手改词语收集、词库预设读写、系统剪贴板复制已补齐。隔离沙箱自检验证词语去重、规则启停与删除、预设读写以及重新打开存储后的持久化，测试不修改用户数据。另有离线语音检查，使用隔离的 fixture ASR/润色验证 PCM、结果预览、不自动插入、取消与过期音频拒绝、WAV 读取、重识别不覆盖完成的历史及重新润色；它不等同于真实联网识别。

仍未完成的移动端业务：

- 输入法录音与真实联网 ASR/润色的设备验收；实时转写逐字显示和渠道录音测试。
- 主应用内听写入口及结果事件适配。
- 端到端加密云同步的安全凭据事务、状态事件及界面适配；普通云备份已接到共享服务，远程操作仍需账号配置验收。
- 鸿蒙更新检测及安装流程；不能直接调用安卓 APK 更新。

桌面悬浮窗口、全局快捷键、Windows IME、Wayland、桌面 CLI / Less Computer 等命令不作为当前手机和平板版本的完工条件。`ipc-audit.json` 是共享 IPC 静态覆盖清单，包含这些桌面接口；覆盖比例不等于移动端完成比例，也不证明端到端功能可用。

跨应用输入建议以 IME Kit 为主：输入法通过系统分配的 InputClient 向绑定且获得焦点的编辑框插入文字。基础权限沿用 INTERNET 和 MICROPHONE；输入法需额外声明扩展并由用户在系统设置启用。键盘面板使用输入法框架，无须为了显示面板申请普通悬浮窗。

本项目仅走输入法，不接入无障碍扩展。悬浮窗本身不提供跨应用写入能力；不启用输入法时可用复制后手动粘贴作为退路。

## 输入法面板

`OpenLessInputMethod` 是系统输入法扩展，`pages/InputMethodPanel` 是原生 ArkUI 键盘面板，不经过主设置页的 WebView。已通过 Node-API 接入与 Android 共用的 librime 1.17.0，支持离线拼音、候选词、翻页、空格确认、中英文切换、数字与标点、编码退格及系统输入框删除。没有引入 Fcitx5，也不使用无障碍。

在系统设置的输入法管理中启用“OpenLess 输入法”，然后在普通输入框切换到它。仅声明输入法扩展及既有的网络/麦克风权限，不注册无障碍服务。系统分配的 InputClient 在 inputStop 时解绑；排队按键不会发送到已经解绑的客户端；日志不记录输入内容。

语音按钮使用 16 kHz / 单声道 / 16 位 PCM 录音。识别完成后预览文字，由用户确认写入当前绑定的输入框；写入失败保留结果供重试。取消、收起或解绑会释放麦克风，拒绝迟到结果；密码输入框禁用语音。扩展复用 Asset Store 凭据，并在新会话前读取已保存的渠道设置。主应用内听写能力标记仍为 false，主应用入口另行适配。

参考：[IME Kit](https://developer.huawei.com/consumer/cn/doc/harmonyos-guides-v5/ime-kit-V5)、[输入法插入与预上屏示例](https://developer.huawei.com/consumer/cn/doc/doccenter-dev-faq/faqs-ime-7)、[官方扩展能力模板（包含 Accessibility 和 InputMethod）](https://developer.huawei.com/consumer/cn/doc/doccenter-deveco-studio/ide-hmos-add-new-ability)。

从仓库根目录构建：

```bash
bash openless-all/app/scripts/build-harmony-frontend.sh
```

依赖：已安装前端 npm 依赖、HarmonyOS Command Line Tools、Python 3。
工具链默认 `~/.local/share/harmony/current`，可通过 `OPENLESS_HARMONY_TOOLCHAIN` 指定。
Rust 首次构建需要 `rustup target add aarch64-unknown-linux-ohos`。
缓存默认 `/data-win/openless-cache`，可通过 `OPENLESS_CACHE_ROOT` 指定。

- `harmony/web-dist`：React 离线资源，使用相对 URL。
- `harmony/openless-project`：构建工程副本、项目缓存和 HAP。
- `harmony/rust-target`：Rust 鸿蒙交叉编译产物。
- `harmony/ohpm`：ohpm 下载缓存。
- `cargo`：Rust 依赖下载缓存。
- `harmony/hvigor`：Hvigor 用户缓存及工具依赖。
- `npm`：本脚本使用的 npm 下载缓存。

HAP 位于 `harmony/openless-project/entry/build/default/outputs/default/`。
仓库只维护鸿蒙源文件，构建中间文件不写回鸿蒙源码工程。
前端以 `VITE_OPENLESS_PLATFORM=harmony` 构建，采用移动端能力配置。

包名 `openless.hm.com`，compatible API 12，target API 20，compile SDK 使用本机默认版本。
先生成未签名 HAP；若本地签名目录证书与 Profile 齐全，则使用官方工具生成 `entry-default-signed.hap`。
签名目录默认 `~/.local/share/harmony/signing/local-test`，可用 `OPENLESS_HARMONY_SIGNING_DIR` 覆盖；密码从本地文件读取，不写入源码配置。真机安装需要该包名对应的华为调试证书，以及包含目标设备的调试 Profile。
其他包名的 Profile 不能直接使用。签名材料不能提交到仓库。
编译成功不代表真机运行已验证。

鸿蒙构建使用单文件 IIFE 和延迟执行的普通脚本，去除资源标签的 crossorigin 属性，避免 rawfile 不透明来源导致的模块加载失败；桌面及安卓构建不受影响。

使用 `$rawfile('web/index.html')` 加载页面，开启 JavaScript 和 DOM Storage。
参考：[华为 Web 页面加载文档](https://developer.huawei.com/consumer/cn/doc/harmonyos-guides/web-page-loading)。

设备运行时数据位于应用 `filesDir/openless`，运行时缓存位于 `cacheDir/openless`；电脑上的统一目录用于开发构建缓存。
WebView 通过同步 request/takeResult 传递请求票据，ArkTS 异步执行系统操作，Rust 命令通过 Node-API async work 在后台执行。避免直接将 Promise 从 JavaScript Proxy 返回导致不同 WebView 版本错误解析。
麦克风音量通过本机 WebView 自定义事件返回，关闭列表后释放采集器。API 24 上输入设备切换遵循系统路由，无法强制切换不在当前路由上的麦克风。

面板参考小艺输入法布局：浅灰底、白色圆角键帽、紧凑工具栏，去掉常驻品牌文字。工具栏的布局按钮可切换标准、分离和悬浮键盘，选择保存在本地；标准和分离模式带独立数字行，横屏空间不足时隐藏数字行并压缩按键高度。分离模式包含中间留空、两侧空格、重复的 G/V 键及底部输入法切换、标点、表情、数字入口。

悬浮模式使用 IME Kit 原生 `FLG_FLOATING` 面板，顶部横条可拖动，− / + 调整大小，↙ 恢复固定键盘；位置与尺寸保存在本地，并限制在当前屏幕范围内。无需普通悬浮窗权限或无障碍。面板在屏幕方向改变和重新显示时重新适配尺寸。

兼容运行环境可能不支持 `getEditorAttribute`。该接口报错或超过 1.2 秒未返回时，使用 Rime 私密会话继续提供中文候选，不读取/训练个人词典，禁用语音，回车采用换行；迟到的属性结果不能重新开启学习。已识别的密码框仍只提供英文。文字写入仍由系统分配的 `InputClient` 完成，兼容环境是否支持写入需要真机验证。`scripts/harmony-editor-compat.test.mjs` 覆盖属性报错、超时、迟到结果、密码框和过期客户端。

所有宿主使用 IME Kit 的异步写入/删除接口，避免兼容桥接的同步 IPC 阻塞键盘。除下述兼容桥接策略外，写入只发送一次：接口成功或系统 `textChange` 通知严格匹配本次选区替换后的完整文字时确认提交。后者要求事先收到当前输入框的文字和选区通知；仅光标移动、无基线或不匹配的文字不能证明成功。用于匹配的文字仅在内存保留最多 4096 个 UTF-16 单元，切换输入框时清除；手动恢复只清除本次确认目标，保留当前输入框基线供下一次写入匹配；密码框和属性未知的输入框不保留文字通知。日志不记录输入内容。写入/删除等待上限 800 毫秒；未确认的词保留在 Rime，工具栏提供“已写入”（只 ack，不再次插入）、“重试写入”和“取消待提交”。卓易通是否提供足够的通知完成自动确认仍需设备验收。

设备反馈确认卓易通的文字通知和回读均不可用。对于属性可读、包名为空或为 `com.droi.tong` 的非密码编辑框，提交、删除及搜索/完成等编辑框动作采用单次异步发送，Rime 随后释放已交给系统的文字并继续组词；不将缺失的宿主回执作为下一次输入的前置条件，也不要求首次手动确认。不进行自动重发。这里的 Rime ack 表示文字已交给系统接口，不代表宿主确认落字；明确返回 false 或非预期错误时提示检查输入框，桥接对已实际落字的操作也会返回的 `12800003` 仅记录诊断，避免迟到回执重新锁住键盘。旧输入框结果不影响新输入框。属性未知和密码框不采用这一提交策略。未确认回执的兼容桥接仍无法提供与正常原生编辑框相同的写入成功保证，真实连续输入、连续删除需要设备验证。

“已写入”和“取消待提交”不等待挂起的客户端写入队列，恢复时使旧任务失效；旧确认/失败不能覆盖恢复后的状态或确认新词。等待确认期间禁用“重试”；超时也不自动重发，用户应检查输入框，已出现文字时选择“已写入”。切换输入框会建立新队列、释放旧 Rime 会话，不让旧客户端的无响应请求拖住新输入框初始化。回归检查包含精确文字通知、错误通知、无基线、永不返回的写入、手动确认后继续输入、迟到结果、密码框和输入框重绑定。

候选栏支持展开及翻页，数字九宫格与符号页独立，逗号、句号、常用符号和长按标点随中英文切换。字母和数字键使用右上角符号提示：长按 350ms 或明显上滑后显示预览，松开只输入一次，不额外输入原字母；上滑距离随键高限制在 22–30vp，并限制横向偏移，滑回可取消。顶部反馈区区分原生编辑器确认的“已输入”和兼容编辑器单次派发的“已发送”。`scripts/harmony-symbol-gesture.test.mjs` 验证手势互斥、取消、切换编辑器、误触距离及分离键盘映射。支持长按删除、单次大写及大写锁定、中英文切换与长按系统输入法切换。表情和工具页均接到实际操作；语音保留录音、结束、预览确认和取消。颜色通过 ArkUI 深浅色资源跟随系统，不改主应用设置布局。`scripts/harmony-panel-layout.test.mjs` 检查模式、持久化、缩放、拖动边界、横屏和原生切换失败回退；真实触摸与显示仍以设备验证为准。

### 离线词库与云候选升级

扩展创建时并行预热词库，与输入框初始化共用同一个 Promise，不创建学习会话或云查询。首次资源复制使用异步分段写入和临时文件原子重命名，避免大文件同步写入阻塞界面；不在输入框就绪路径运行完整打字自检，主应用诊断仍可调用 `testRime`。首次安装/词库升级仍需 Rime 部署编译，后续复用缓存。`scripts/harmony-rime-startup.test.mjs` 覆盖并行初始化、短写、资源缓冲区边界和失败重试。

两端共用固定修订的雾凇中文词库和拼写规则，保留本机已有 `rime-user/pinyin_simp.userdb`。不捆绑雾凇 Lua 工具或语法模型。新资源目录单独版本化，避免已安装应用跳过词库更新。

键盘工具中的“云候选”默认关闭；说明明确当前拼音将发送至 Google，用户开启后才查询。仅对属性已知的非密码框生效，失败保留本地候选；不将输入框周围文字交给云服务。候选回复及选择分别验证输入版本，云结果插入后仍按原生索引选择本地候选。更多说明见 `mobile-ime/README.md`。
