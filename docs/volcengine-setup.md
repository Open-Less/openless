# 火山引擎（volcengine）ASR 配置

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。

## 1. 代码中的定义

- Provider：`volcengine`（labelKey `asrVolcengine`），定义于 Core `provider_rules.rs`；`authRequirement = Volcengine`（专用鉴权形态），无内置默认端点/模型（`defaultEndpoint` / `defaultModel` 为空，按通道配置）。
- 验证探针：`asr_silence_allows_no_final`（静音段允许无 final 帧，验证以可取消的静音探测完成）。
- 凭据字段（`provider_rules.rs:300-302`）：`volcengine_auth_mode`（鉴权模式，如 ApiKey/官方端点模式）、`volcengine_app_key`、`volcengine_access_key`（布尔项 + 模式选择；具体取值在设置界面录入，凭据走系统安全存储，不落明文）。

## 2. 在应用内配置

设置 → AI 服务 → 语音识别 → 添加渠道，选择火山引擎；按界面提示填入鉴权字段，保存后执行“验证”得到真实验证结果（成功/失败与时间会记录在渠道列表）。

## 3. 端点与排错

- ApiKey 模式使用火山官方实时 ASR 端点（历史修复 #931 后的行为，以 `crates/openless-core/src/asr/volcengine.rs` 当前实现为准）。
- 弱网行为：连接超时与重试在 Host/Core 实现，失败信息展示在渠道验证结果中。
- 开通服务、创建应用与获取密钥属火山控制台操作，以[火山官方文档](https://www.volcengine.com/docs)为准；本仓库只维护代码行为。
