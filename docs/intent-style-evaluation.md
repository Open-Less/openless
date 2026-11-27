# 专为构建而生 1.4.0 评测

评测集位于 `openless-all/app/crates/openless-core/tests/fixtures/intent-reconstruction-evaluation.json`，共 16 条虚构中文口述或选区草稿。每条包含必须保留、必须删除和结构要求；这些是语义评审要点，不是逐字匹配答案。未使用用户录音，也没有预填或编造模型输出。

## 离线生成并排记录

在 `openless-all/app/` 执行：

```sh
node scripts/intent-style-comparison.mjs --template /tmp/intent-comparison.json /tmp/intent-comparison.md
```

生成的两列输出均明确标记待真实调用。记录文件拒绝覆盖已有文件。也可以把通过应用取得的结果填到 JSON 中各样例的 `old`、`new` 对象，再用下面的命令生成 Markdown：

```sh
node scripts/intent-style-comparison.mjs /tmp/intent-comparison.json /tmp/intent-comparison-reviewed.md
```

结果对象使用 `{"text":"最终正文","sourceText":"翻译前源文或 null","elapsedMs":123}`。失败时使用 `{"errorCode":"Provider"}`。离线渲染器不发请求、不读取凭据。

## 获准后使用现有后处理渠道对照

先获得真实调用许可，说明请求数：16 条 × 旧新版各一次 = 32 个主要请求；两个翻译样例若两版都缺少标记，现有纯翻译兜底最多再发 4 次，合计最多 36 次。无自动重试、无多渠道或多模型扫描，遇到首个渠道错误即停止并保留部分记录。

提供的是被 `#[ignore]` 排除的测试 `intent_style_tests::intent_live_configured_channel_comparison`。它复用 `SharedCloudTextPolisher`、生产请求组装、协议、翻译解析与兜底路径，不自行创建另一个模型客户端。仅测试构造的内存凭据被使用，不修改应用设置、渠道、风格选择或请求参数。

从用户已经配置的后处理渠道复制渠道 ID、协议类型、模型、思考开关、endpoint、API Key 和渠道级选项到仓库之外的私有 JSON 文件。不要把密钥写到命令行、日志、报告或 Git。以下仅描述字段形状，所有字段值均应来自同一现有渠道：

```json
{
  "channelId": "existing-channel-id",
  "providerType": "openai-compatible",
  "model": "existing-model-id",
  "thinkingEnabled": false,
  "baseline": "1.0.0",
  "credentials": {
    "ark.endpoint": "https://your-configured-endpoint.example/v1",
    "ark.api_key": "your-existing-key",
    "ark.request_format": "chat_completions"
  }
}
```

`thinkingEnabled` 必须与现有配置一致，示例中的 `false` 不代表要求修改用户设置。`providerType` 和 `ark.request_format` 也应照抄，兼容渠道支持 `chat_completions`、`responses`、`messages`，也支持已有 Gemini API 渠道。需要时同时复制 `ark.temperature`、`ark.extra_headers`、`ark.messages_thinking`、`ark.max_tokens`、`ark.thinking_budget`、`ark.service_tier`；凭据值均为字符串，与 Core 存储一致。OAuth / ChatGPT Plan 渠道使用下方应用操作步骤，不由该测试隐式选取登录身份。

旧基线默认由配置显式指定为反馈中的 `1.0.0`；也可填 `1.3.0` 对比当前 beta 实际运行版本。两者均使用不可变的发布快照。新版本为 `1.4.0`。为保持可比，两版使用同一当前组装器及全局口述修正规则；这不是复刻旧应用二进制。每条按旧版、新版顺序串行执行，未注入热词、应用或历史。选区专用指令放在独立的 `selection_instruction` 段，原文始终作为数据处理。

获得许可、填好私有配置后执行（以下命令会真实调用渠道）：

```sh
chmod 600 /private/path/intent-channel.json
OPENLESS_INTENT_COMPARISON_APPROVED=yes \
OPENLESS_INTENT_COMPARISON_CONFIG=/private/path/intent-channel.json \
OPENLESS_INTENT_COMPARISON_OUTPUT=/tmp/intent-live-results.json \
cargo test -p openless-core -j 1 intent_style_tests::intent_live_configured_channel_comparison -- --ignored --exact --test-threads=1
node scripts/intent-style-comparison.mjs /tmp/intent-live-results.json /tmp/intent-live-results.md
```

批准标志只是防误触门槛，不能代替调用前的用户许可。输出包含渠道 ID、请求模型、思考开关、基线版本、候选版本、原始输入、期望要点、源文、最终正文和耗时；不保存配置中的凭据。耗时为整个处理路径耗时，可能包括翻译兜底，并非单次首 token 延迟。该工具不将调用成功视作语义通过，也不声称服务端返回的实际模型与请求名称一致。

## 在应用内手工对照

OAuth 渠道或不希望复制凭据时，在风格设置建立两个临时自定义风格，分别粘贴不可变 `releases/intent-reconstruction-1.0.0.json` 的 `prompt` / `selectionPrompt` 与新版 `intent-reconstruction-v1.json` 的对应字段。另行对比 1.3.0 时使用 `releases/intent-reconstruction-1.3.0.md`。不要直接导入带官方来源的旧包，以免启动迁移把基线升级。

保持同一个已经配置的渠道、模型、思考开关、语言及热词；关闭上下文历史，记录这些设置。依次将样例作为普通润色、按风格润色后翻译或选区润色输入；翻译目标用样例的 `targetLanguage`，有 `instruction` 的选区走独立编辑指令。每条在两版各处理一次，将真实结果填入离线记录，不将语音重新识别的随机差异混入文本重构比较。

逐条检查信息是否遗漏、撤销是否生效、是否重复归组、层级与编号是否合理、专名数字是否保留、是否新增要求或代答。给每条记录通过 / 不通过和具体偏差；失败后可以修改提示词，但不能覆盖已有对照证据。最终效果由用户判断。渠道是否返回缓存读取和推理用量，仍须通过应用的用量账本另行检查；该并排记录不代替缓存或计费测量。

## 使用既有 Actions secret 的手动状态对照

仓库已有的 `Optional official-provider live acceptance` 增加了非默认选项
`intent-style-comparison`。仅限 `Open-Less/openless` 仓库的 `main` / `beta`
分支手动运行；fork、PR、tag 和其他分支不进入带 secret 的 job。默认仍为 `off`，
普通 CI 不会调用模型。协调者先将本改动接入经审查的集成版本，核对分支 SHA 后可执行一次：

```sh
gh workflow run ci.yml --repo Open-Less/openless --ref beta \
  -f live_acceptance=intent-style-comparison
```

这是未自动执行的真实付费调用命令；沿用调用前授权，不能重复运行消耗预算。
它只使用现存 `DEEPSEEK_API_KEY` secret，不读取或搬运本机配置；渠道固定为官方 DeepSeek，
模型与现有官方验收一致为 `deepseek-flash`，思考关闭，使用默认参数。
两版在同一配置下按原 16 条夹具顺序比较旧运行版 **1.3.0** 与 **1.4.0**：
12 条听写、2 条翻译、2 条选区；32 主请求加最多 4 个翻译兜底，总上限 36。
这不是用户本机渠道身份/模型的证明，也不是旧应用二进制的完整复刻。

先在无 secret 的 step 编译，再直接执行唯一已编译的 Core 测试二进制及精确 ignored
测试。secret 只给执行 step；runner 专用目录 0700、配置文件 0600，
不把密钥放进命令行。子进程不继承 secret 环境变量，stdout/stderr 丢弃，
配置及原始结果由 Python finally 和 shell trap 清理。缺失凭据、渠道报错、超时、
缺报告、零测试或不完整结果均使 step 失败，不自动重试。

上传工件 `intent-style-comparison-status` 只含 `status.json`：可信夹具及期望、
版本、请求模型、思考开关、源 SHA（格式有效时）、每侧 pending/completed/request_failed
和有界端到端耗时。provider text、sourceText、原始错误、配置和 Cargo/HTTP 日志全部不上传。
模型响应属于不可信数据，字段筛选本身无法保证任意响应内容不含凭据，因此这一模式
保守地只留下安全执行状态。**completed 只表示请求返回，不表示语义通过；工件不能替代
旧新正文的人审。** 如需实际语义对照，须由协调者另行确认能够安全查看响应的受控路径，
或在 Dev 应用中按真实输入流程验收，不把状态工件称为 #26 通过证据。

## 版本与迁移设计

运行版此前已是 1.3.0，所以本次递增为 1.4.0，而非回退至 1.1.0。主 JSON 的两个作者槽和共享 Markdown 内容一致，运行时仅追加一次全局口述撤回规则。稳定正文不包含热词占位符，组装器在正文之后追加可变热词和上下文，不向存储回写动态数据。

1.0.0 JSON 与 1.3.0 共享正文保存在 `src/builtin_styles/releases/`，1.1.0 和 1.2.0 仍用原有确定性变换还原。存储加载与云端快照规范化均按“来源 ID + 作者 + 版本 + 所有可编辑行为字段的精确匹配”认定已发布版本，再原位升至 1.4.0。改名、启用状态、本地 ID、选择和应用 / 类别绑定保持不变；提示词、选区提示词、VoiceEdit、示例、模式、模型建议或来源等被改写的包不覆盖。原来的 1.1 投影 fixture 不改写历史，另加 1.4 投影 fixture 验证新版。

本次不改变模型、思考强度、渠道参数、翻译协议或自定义风格的独立提示槽。自动测试验证提示词和请求契约；语义质量、实际缓存命中、计费和延迟需要获准后的真实对照。
