# OpenLess 移动端 Rime

Android 和鸿蒙直接链接 librime 1.17.0，共用本目录 C ABI 会话实现；不依赖 Fcitx / Fcitx5。Linux 现有插件不在本次迁移范围。

`rime_engine.cpp` 提供初始化、创建会话、按键、候选选择、确认、读取状态、取消、提交确认和销毁。原生客户端对象不跨平台传递。待提交文本保留至平台写入成功后 ack；重试或取消由宿主处理。密码框不进入中文路径，禁用学习时使用 `enable_user_dict: false` 的独立拼音方案。

鸿蒙通过 `libopenless_harmony.so` 的 `rimeAsync` 在工作线程调用 C ABI；Android 通过 `libopenless_rime.so` 的 JNI 调用。两端都保留各自原生 IME 生命周期及文本提交接口。

构建使用 `scripts/build-mobile-rime.py`，依赖、词库、编译产物在 `/data-win/openless-cache/mobile-rime`，可通过 `OPENLESS_CACHE_ROOT` 覆盖。引擎版本和输入资源修订固定在脚本中；开发构建不下载个人词库或上传按键数据。

首次构建：

```bash
python3 scripts/build-mobile-rime.py harmony
python3 scripts/build-mobile-rime.py android --ndk /path/to/android-ndk
```

Android 支持 `--abi arm64-v8a|armeabi-v7a|x86|x86_64`。`--stage` 将对应 ABI 原生库、libc++ 和词库复制到已生成的 Android 工程；正常 `npm run tauri:android:build` 自动执行该步骤。

鸿蒙签名包和真机自检已通过；Android arm64 Release APK 已构建，x86_64 的真实 JNI、词库及 Android 控制器通过 CI 模拟器测试。Android 实机手感、其余 ABI 和旧学习词库迁移尚待验证。

当前使用官方 `pinyin_simp` 的原生处理链，接入固定修订 `da1fbe60` 的雾凇拼写规则与中文字表、基础、扩展、腾讯、杂项词库，保留上游版权和 GPL-3.0 许可。生成资源将 `cn_dicts/*` 的名称展平，避免移动资源路径差异；没有引入雾凇 Lua 日期、计算器、置顶等工具，也未打包语法模型。它不是 Linux 所有 Rime 方案与自定义词库的完整替代；更多方案、用户词库迁移和键盘体验需要逐步完善。

Android 保留现有键盘布局和候选展开列表。后端通过 `candidates` 返回原生顺序的前 64 个候选，通过 `select_absolute` 按绝对索引及版本选择；鸿蒙现有的分页 `select` 接口继续按当前页索引工作。只读 `state` / `candidates` 不增加候选版本，避免后台刷新使可见候选失效。候选展开有数量上限；不代表穷举词库或增加词条。

Android 的空格、删除、回车与字母在同一个串行队列中执行，是否仍有拼音组合由执行时的 Rime 状态决定；拼音中的单引号作为音节分隔符处理。刷新键盘重新读取组合和候选。上屏失败保留待提交文字及其后的标点，成功才确认，输入会话切换则清除队列所属状态。回归测试覆盖快打后连续空格/删除/回车、首屏以外的候选选择、只读刷新、分隔符、写入失败重试及编辑器切换。

普通输入框使用共同的 `openless_pinyin_learning` 方案：启用 Rime 原生用户词典、组句和补全，沿用 `rime-user/pinyin_simp.userdb`，保留此前 Android 已学到的数据。鸿蒙普通输入框也使用该方案。选词调频和从分段选词学习新词均由 Rime 完成，宿主不另存完整输入历史或上传按键。会话销毁时由 Rime 提交用户词典事务，下一会话继续使用。当前未打包语法模型，因此不启用依赖该模型的上下文重新排序，也不声称已达到商业输入法的语境预测能力。

Android 继续遵守 `IME_FLAG_NO_PERSONALIZED_LEARNING` 和密码字段策略；不学习的会话使用独立 `openless_pinyin_private` 方案，既不训练也不读取个人词典。鸿蒙密码框不创建中文会话；普通字段的本地学习在获取系统输入框属性后才启用，自检始终使用私密会话。共享资源目录升级为 `rime-shared-1.17.0-ice-da1fbe60-v1`，避免已安装应用因旧资源缓存而漏装学习方案；个人词典目录保持原位。

旧 Android LitePinyin 算法及内置 TSV 已删除；旧备份格式只保留一个不参与输入的兼容适配器。

可评估的上游扩展（当前未启用）：[librime-predict](https://github.com/rime/librime-predict) 提供下一个词预测，需额外预测数据；[librime-lua](https://github.com/hchunhui/librime-lua) 用于复用成熟方案脚本；[librime-octagram](https://github.com/lotem/librime-octagram) 及其模型可用于语法加权。扩展与数据需分别验证 OHOS / Android 构建、版本兼容及安装体积，不自行实现平行算法。当前个人学习直接使用 librime 原生用户词典。

## 云候选

本机 Fcitx5 配置实际默认使用 libime 拼音且开启云拼音，并非 Rime。移动端保留 librime；两端通过 `openless-core::ime_cloud` 使用与 Fcitx5 默认相同的 GoogleCN 拼音协议。参考来源：[Fcitx5 cloudpinyin](https://github.com/fcitx/fcitx5-chinese-addons/tree/master/modules/cloudpinyin)。仅发送当前 2–64 字节的小写拼音及分隔符，不发送上下文、账号或个人词典，不记录查询或响应内容。没有可用性承诺；失败返回空候选，离线 Rime 不等待网络。

默认关闭。鸿蒙在键盘工具中显示 Google 来源和发送说明，Android 在键盘设置中提供相同说明和开关。密码框、属性未知/私密输入及 Android 禁止个性化学习字段不查询。180 毫秒防抖、1.2 秒网络超时、64 KiB 响应上限；每次输入/切框均验证版本，旧结果不得显示或提交。开启开关后，首选本地候选保留，去重的云候选从第二位开始补充。

云候选选中后通过共用 `external` 请求携带候选版本，保留待提交文字至宿主 ack；不直接学习为 Rime 用户词条。它不属于原生词典的选词调频，不能声称云候选已自动训练到本地。已有 Rime 用户词典路径和名称保持不变，旧 LitePinyin 数据仍未迁移。
