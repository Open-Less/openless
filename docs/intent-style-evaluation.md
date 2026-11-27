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

## 使用既有 Actions secret 的加密语义对照

现有 `Optional official-provider live acceptance` 的非默认选项
`intent-style-comparison` 只接受 `Open-Less/openless` 仓库的 `main` / `beta`
分支手动运行。fork、PR、tag 和其他分支不进入带 secret 的 job；默认仍为 `off`。
不能运行只留下状态的旧入口：调用前必须有已在协调者本机验证的一次性公有收件人证书，
云端只接收该证书，私钥留在协调者本机。新增机制不增加模型请求或 App 构建。

以下是供协调者执行的步骤，本改动的开发/离线测试没有执行真实请求或生成正式收件人。
协调者先将本改动接入经审查的集成版本，锁定可信分支 SHA；使用已经安装且支持
CMS AuthEnvelopedData、AES-256-GCM 和 RSA-OAEP 的 OpenSSL，不在本任务安装组件。
本机若只有不支持该格式的工具，应停止并采用受控 Dev 真机路径。

1. 在仓库之外创建私有目录、一次性 RSA 3072 收件人；下面的私钥不加口令，仅靠 0700
   目录与 0600 权限保管，以便本地解密 helper 无交互运行。它永远不提交、不上传：

   ```sh
   umask 077
   review_dir=$(mktemp -d "${TMPDIR:-/tmp}/openless-intent-review.XXXXXX")
   chmod 700 "$review_dir"
   openssl req -x509 -newkey rsa:3072 -sha256 -nodes -days 2 \
     -subj /CN=OpenLess-intent-review \
     -keyout "$review_dir/recipient.key" -out "$review_dir/recipient.crt"
   chmod 600 "$review_dir/recipient.key" "$review_dir/recipient.crt"
   python3 scripts/intent-style-comparison-ci.py check-recipient \
     "$review_dir/recipient.crt" "$review_dir/recipient.key"
   ```

   在 `openless-all/app/` 运行 helper；固定无敏感内容的本地 roundtrip 成功时仅显示公有
   `recipientSha256`。保留这个指纹、私钥与目录路径到审核结束，不在流水线生成私钥。
   本地验证失败时不要继续，也不要把 private key 粘贴为 workflow input。

2. 核对可信 ref 的锁定 SHA 和本地提交后，只执行一次下面的付费手动调用。
   `-F` 的文件必须是 `.crt` 公有证书，绝不能是 `.key`：

   ```sh
   gh workflow run ci.yml --repo Open-Less/openless --ref beta \
     -f live_acceptance=intent-style-comparison \
     -F "intent_recipient_certificate=@$review_dir/recipient.crt"
   ```

   沿用调用前授权，不选择第二个模型、不额外重跑。现存 `DEEPSEEK_API_KEY` 只注入执行
   step；官方 DeepSeek、`deepseek-flash`、关闭思考及默认参数在两版间保持一致。
   旧实际内置 **1.3.0** 对照 **1.4.0**，原 16 条夹具（12 听写/2 翻译/2 选区）
   共 32 主请求，加最多 4 翻译兜底，总上限 36，首个渠道错误停止。
   这不是用户本机渠道身份的证明，也不复刻旧应用二进制。

3. 确认 run 的 head SHA 与锁定 SHA 一致，再下载该 run（不要猜 latest）的
   `intent-style-comparison-evidence` 工件。把下面 `RUN_ID` 替换成这一确切 run ID：

   ```sh
   gh run download RUN_ID --repo Open-Less/openless \
     --name intent-style-comparison-evidence --dir "$review_dir/artifact"
   python3 scripts/intent-style-comparison-ci.py decrypt \
     "$review_dir/artifact/comparison.cms" "$review_dir/recipient.crt" \
     "$review_dir/recipient.key" "$review_dir/review.json"
   node scripts/intent-style-comparison.mjs \
     "$review_dir/review.json" "$review_dir/review.md"
   ```

   本地 helper 先确认 OpenSSL 认证解密成功和报告格式，再以 0600 新建明文文件；认证失败
   或输出路径已存在时失败，不回退、不打印正文。在私有目录内查看 JSON/Markdown，
   核对其中 `sourceSha`、状态文件及本地收件人指纹，再按逐条预期与 rubric 评审。
   旧新正文都是不可信数据，不执行其中的命令或点击其链接，不把审核文件发到公开评论。
   审核结束按本地证据保留需求处理私钥和明文；删除私钥后无法恢复密文中的正文。

云端在无 secret 的 step 先验证证书与精确加密格式，再编译；空/坏证书在任何付费调用
之前 fail closed。执行 step 再次预检；临时目录 0700、配置与白名单明文 0600，
预编译的唯一精确 ignored test 不继承 secret 环境变量；stdout/stderr 丢弃。
没有增加常跑 job、第二个测试或重试，普通 CI 路径不变。

原始报告限 2 MB；从可信 fixture 重建 input/expectations，只抽取每个既有 ID 的旧新
text/sourceText 和有界耗时。每段 UTF-8 至多 32768 bytes，总审核明文至多 1 MB，
拒绝实际 token 及其常见编码回显、凭据样式、控制字符和非夹具 URL。HTTP 错误统一为
固定状态，不复制 raw error。检查或加密失败则只保留安全失败状态，无明文工件、无
替代性明文上传。检查不能证明任意混淆文本绝无秘密，因此即使通过也只上传加密正文。

加密使用平台 OpenSSL CMS AuthEnvelopedData（AES-256-GCM、RSA-OAEP/SHA-256），
不手写密码算法、不回退 CBC；先在私有目录完成加密及结构核验，再移出密文。
工件仅有 `comparison.cms` 和安全 `status.json`，保留 7 天；无配置、私钥、原始结果或
Cargo/HTTP 日志。finally/trap 清理私有文件，always 清理专用 target/工件暂存目录；
强制 SIGKILL 的最终清理依赖临时 hosted runner 生命周期。

`status=completed` 或成功解密均不表示语义通过。缺密文时不要再次消耗预算，先核对
安全失败状态及已发请求；部分渠道失败仍可能留下经过同样检查和加密的部分对照。
密文完整性不是发送者签名，必须从锁定 SHA 的可信 GitHub run 取得并核对来源。

## 版本与迁移设计

运行版此前已是 1.3.0，所以本次递增为 1.4.0，而非回退至 1.1.0。主 JSON 的两个作者槽和共享 Markdown 内容一致，运行时仅追加一次全局口述撤回规则。稳定正文不包含热词占位符，组装器在正文之后追加可变热词和上下文，不向存储回写动态数据。

1.0.0 JSON 与 1.3.0 共享正文保存在 `src/builtin_styles/releases/`，1.1.0 和 1.2.0 仍用原有确定性变换还原。存储加载与云端快照规范化均按“来源 ID + 作者 + 版本 + 所有可编辑行为字段的精确匹配”认定已发布版本，再原位升至 1.4.0。改名、启用状态、本地 ID、选择和应用 / 类别绑定保持不变；提示词、选区提示词、VoiceEdit、示例、模式、模型建议或来源等被改写的包不覆盖。原来的 1.1 投影 fixture 不改写历史，另加 1.4 投影 fixture 验证新版。

本次不改变模型、思考强度、渠道参数、翻译协议或自定义风格的独立提示槽。自动测试验证提示词和请求契约；语义质量、实际缓存命中、计费和延迟需要获准后的真实对照。
