# 自部署 MiMo-V2.5-ASR 接入说明

本分支把 OpenLess 的 `xiaomi-mimo-asr` provider 改成优先兼容自部署 vLLM-Omni MiMo-V2.5-ASR。

## 推荐配置

如果你的模型服务是：

```text
http://36.147.35.14:30081/v1/chat/completions
```

OpenLess 里 ASR 配置填写：

```text
ASR Provider: xiaomi-mimo-asr
Endpoint: 36.147.35.14:30081
Model: mimo-v2.5-asr
API Key: 如果你的网关要求鉴权就填真实 key；裸 vLLM 服务可留空
```

也可以填：

```text
Endpoint: http://36.147.35.14:30081
Endpoint: http://36.147.35.14:30081/v1
Endpoint: http://36.147.35.14:30081/v1/chat/completions
```

代码会自动归一化到 `/v1/chat/completions`。

## 改动点

1. 支持裸 `IP:端口`，自动补 `http://` 和 `/v1/chat/completions`。
2. API Key 允许为空；只有填写了 key 才发送 `Authorization: Bearer ...`。
3. 请求体从小米官方 `input_audio` 改为 vLLM-Omni 兼容的 `audio_url`：

```json
{
  "model": "mimo-v2.5-asr",
  "stream": false,
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "audio_url",
          "audio_url": {
            "url": "data:audio/wav;base64,..."
          }
        },
        {
          "type": "text",
          "text": "请把这段音频完整转写成文字，只输出转写结果。语言自动识别。"
        }
      ]
    }
  ],
  "modalities": ["text"],
  "temperature": 0,
  "max_tokens": 2048
}
```

## 日志

新增 `[mimo-asr]` 日志，包含：

- 归一化后的请求地址
- 模型名
- 鉴权是否启用
- PCM/WAV/JSON 大小
- 分片序号
- HTTP 状态码
- 请求耗时
- 解析文本长度

常见错误判断：

```text
404 Not Found        多半是 endpoint 路径错了
401 Invalid API Key  网关要求鉴权，API Key 不对
400 Incorrect padding 旧版 input_audio data 格式不兼容；本分支已改为 audio_url
502 Bad Gateway      上游网关或 vLLM 服务异常/超时
```

## 服务端快速验证

```bash
curl http://36.147.35.14:30081/v1/models
```

能返回模型列表后再在 OpenLess 中测试 ASR。
