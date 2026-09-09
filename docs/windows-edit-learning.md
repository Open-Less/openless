# Windows 手改学习（实验）

## 功能与使用

将“发现听写后的手改 → 用户确认 → 记住改法”扩展到 Windows。

1. 在设置 / 数据存储中开启“手改词条学习（Windows 实验）”。沿用默认关闭的 `cursorContextEnabled` 偏好；不会自动为现有用户打开。
2. 听写成功插入后，在原输入框内修改一个短词，停顿约 1.2 秒。
3. 在建议卡片确认后，才将这条替换写入本机纠正规则。取消不会保存。
4. 后续听写（包括 raw 模式）复用已有纠正规则流程。在词汇表 / 纠正规则中可停用或删除。

这是明确确认的文字替换，不是训练 ASR，也不是模型原生热词偏置。macOS 仍保留原来的词汇表学习路径；Windows 不会向润色模型发送输入框上下文。

## 实现与隐私边界

- 使用现有 `windows` 依赖的 UI Automation TextPattern；COM 对象仅在专属 MTA 线程使用。
- 250 ms 轮询，最多 60 秒；连接和事务超时均为 500 ms。不主动聚焦控件。
- 仅观察原前台窗口、原进程、原焦点控件。切换窗口或控件、关闭开关或停止监听后不再提交建议。
- 输入文本必须在字段中唯一匹配，以固定前后文定位听写区域；不明确的位置不会学习。
- 读取字段限制为 8192 个 UTF-16 单元，超限跳过。匹配所需字段快照只存在内存，不写入日志或上传；日志仅记录修改的字符数。
- 每次读取前检查密码标记、焦点、启用状态和进程。跳过已知密码管理器、终端、带终端的编辑器以及 OpenLess 自身；不支持 TextPattern 的控件直接跳过。
- 不使用键盘记录、剪贴板、OCR 或全局文档监控。UIA 属性依赖宿主实现，进程名单不是对所有敏感应用的完整识别。
- 单字来源、空替换、通配符、重复冲突和已知连锁替换被拒绝；保存失败在卡片显示错误，不提前移除建议。

## 限制

富文本编辑器、网页自绘控件、不同进程的内嵌控件或不公开 UIA TextPattern 的应用可能无法学习。追加文字、发送/清空输入框、整句重写和不明确的匹配不会生成规则。固定词替换仍可能在不同语境误改，必须由用户确认，并允许停用/删除。

本补丁基于 1.3.18 稳定版；提交目标为 `main`。`beta` 已重构部分 coordinator/core 模块，不能把稳定版构建结果当作 beta 的验证证据。

## 验证记录

- Windows release 构建、`cargo check --locked --lib` 和前端构建已在本地完成。
- 提交前重新执行 `npm run build`：通过。
- 提交前重新运行这份代码先前编译的 release 测试可执行文件：host_document 77 项、persistence::correction 4 项、edit_watch 4 项、raw 纠正规则 1 项，合计 86 项全部通过。此次复跑并非重新编译 Rust 测试。
- 修改后的 Windows 应用已启动；完整“实际输入框听写 → 手改 → 确认卡片 → 下一次听写应用规则”尚未完成端到端人工验收。macOS/Linux 未在本次环境构建验证。

可复现测试命令（在 `openless-all/app`，原生依赖按仓库构建说明准备）：

```sh
npm run build
cd src-tauri
cargo test --locked --release --lib host_document
cargo test --locked --release --lib persistence::correction
cargo test --locked --release --lib coordinator::dictation::tests::edit_watch
cargo test --locked --release --lib non_streamed_output_still_applies_correction_rules
```

人工验收清单：

- [ ] 开关关闭时不出现学习卡片。
- [ ] 支持 UIA 的输入框中修改短词后出现卡片，拒绝不会创建规则。
- [ ] 确认后重启应用仍保留规则，raw 听写应用规则，停用/删除后不再替换。
- [ ] 切换窗口或输入框后不再学习原字段。
- [ ] 密码框、终端、超长字段和重复匹配字段不产生建议。
- [ ] 保存冲突时卡片显示错误而不是虚假成功。

本提交不包含模型权重、录音、个人词库、凭据、本机服务脚本或已编译程序。
