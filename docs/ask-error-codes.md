# Ask 错误代号

Ask 的公开错误提示由 `openless-all/app/ask-window/client.ts` 中的固定映射生成。界面不显示任意后端 `message`、`reason`、内部异常或原始响应。兼容旧接口时只接受 `message` 中精确匹配的已知符号；未知内容固定显示 `ASK-0000`。

| 代号 | 对应原因或操作 |
| --- | --- |
| ASK-0000 | 未识别的请求或后台错误 |
| ASK-1001 | 未配置模型：`not_configured` |
| ASK-1002 | 当前请求仍在处理中：`busy` |
| ASK-1003 | 请求参数无效：`invalid_argument` |
| ASK-1101 | 图片不受支持：`askImageUnsupported` |
| ASK-1102 | 未生成可用内容：`askDraftUnusable`、`askAnswerUnusable` |
| ASK-1103 | 生成内容检查失败：`askDraftReviewUnavailable`、`askAnswerReviewUnavailable` |
| ASK-1104 | 搜索资料不可用：`askDraftSearchUnavailable` |
| ASK-1105 | 选区不可用：`askSelectionUnavailable`、`askSelectionRequired` |
| ASK-1201 | 无法识别请求意图：`askIntentInvalid` |
| ASK-1202 | 模型不支持意图工具：`askIntentToolUnsupported` |
| ASK-1203 | 意图识别超时：`askIntentTimeout` |
| ASK-1204 | 设置读取失败：`askEntrySettingsUnavailable` |
| ASK-1205 | 需要明确目标或重新选择文字：`askIntentClarificationRequired` |
| ASK-1301 | 请求超过上下文预算，或无法有效压缩旧内容 |
| ASK-1302 | 未授权读取相关环境或历史 |
| ASK-1303 | 环境或历史读取不可用 |
| ASK-1401 | 工具请求超时 |
| ASK-1402 | 本次请求的工具调用次数达到限制 |
| ASK-1403 | 工具请求失败 |
| ASK-1404 | 联网搜索已关闭 |
| ASK-1405 | 网页不可访问或读取失败 |
| ASK-1406 | GitHub 发布信息请求失败、无匹配版本或受到限流 |
| ASK-2001 | 事件流或窗口连接失败 |
| ASK-2002 | 语音输入启动、处理或连接失败 |
| ASK-2003 | 附件添加、移除、读取或预览失败 |
| ASK-2004 | 来源链接打开失败 |
| ASK-2005 | 取消请求失败 |
| ASK-2006 | 隐藏窗口失败 |
| ASK-2007 | 展开窗口失败 |
| ASK-2008 | 复制结果失败 |
| ASK-2009 | 结果交付失败 |

已知原因优先于操作代号；操作代号只由前端调用点选择。紧凑窗口在原占位区域临时显示“出现了一点错误 · 代号”，轻晃整块输入窗口；存在草稿时在原输入展示区域短暂显示错误层，草稿内容和选区保留，编辑或提示到期后恢复显示。已有焦点不会清除提示，错误文本不会进入输入值或提交内容。展开窗口通过现有 `useInteraction().notify` 顶部通知显示相同安全提示。

提示在 4.5 秒后、编辑草稿、发起新请求、隐藏或切换会话时清除。普通取消不显示错误。已生成内容继续保留，不再显示旧的核验说明或未核验答复折叠框。精确后端诊断应在受控日志中通过请求与轮次定位，不由此界面回显。
