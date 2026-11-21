# 阿里百炼（DashScope）ASR 模型

## 1. 代码中的定义

- Provider：`bailian`，`authRequirement = api_key`；默认端点 `wss://dashscope.aliyuncs.com/api-ws/v1/inference/`；默认模型 `fun-asr-realtime`。
- 自动模式按模型路由到 DashScope 经典 WebSocket、Qwen 实时 WebSocket 或录音文件转写。自定义模型可在高级设置显式选择协议，由 Core 校验可用值。验证探针随有效路由选择。
- 目录来源：`provider_rules::provider_descriptors` → 生成文件 `src/lib/ipc/provider-descriptors.generated.json`（重新生成：`cargo run --locked -p openless-core --example export_provider_descriptors`）。

## 2. 内置模型示例

| 模型 | 说明 |
| --- | --- |
| `fun-asr-realtime`（默认） | Fun-ASR 实时转写 |
| `fun-asr-flash-8k-realtime` | 8k 采样实时版 |
| `fun-asr-flash-2026-06-15`、`fun-asr`、`fun-asr-2025-11-07`、`fun-asr-2025-08-25` | Fun-ASR 各版本 |
| `fun-asr-mtl`、`fun-asr-mtl-2025-08-25` | 多语种（MTL） |
| `qwen3-asr-flash-realtime`、`qwen3-asr-flash-realtime-2026-02-10`、`qwen3-asr-flash-realtime-2025-10-27`、`qwen3-asr-flash` | Qwen3 ASR 实时/闪速 |
| `qwen-audio-3.0-asr-flash` | Qwen Audio 3.0 ASR |
| `paraformer-v2` | Paraformer v2 |

模型 ID 直选与目录以生成文件为准；新增模型先改 Core `provider_rules.rs` 再重新生成目录，不要手工编辑生成文件。

## 3. 在应用内配置

设置 → AI 服务 → 语音识别 → 添加渠道，选择百炼；填入 API Key（DashScope），选择模型；保存后执行“验证”。
