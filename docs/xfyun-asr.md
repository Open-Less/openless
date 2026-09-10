# 讯飞（iflytek）ASR 配置

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-07。

## 1. 代码中的定义

- Provider：`iflytek`（labelKey `asrIflytek`），`authRequirement = Xfyun`；无内置默认端点/模型。
- 凭据（`crates/openless-core/src/asr/xfyun.rs:62-69`）：`app_id` + `api_key`（两者非空才视为已配置）；连接时以 `app_id + timestamp` 计算 `signa` 签名（`compute_signa`，xfyun.rs:637）。
- 验证探针：`asr_silence_allows_no_final`。
- 对接的是讯飞开放平台实时语音转写（RTASR）；协议细节以 `asr/xfyun.rs` 当前实现为准。

## 2. 在应用内配置

设置 → AI 服务 → 语音识别 → 添加渠道，选择讯飞；填入 AppID 与 API Key，保存后执行“验证”。验证结果（成功/失败与时间）显示在渠道列表。

## 3. 开通与排错

- 服务开通、AppID/API Key 获取在[讯飞开放平台控制台](https://www.xfyun.cn/)完成（外部平台操作，以官方文档为准）。
- 鉴权失败多为 AppID 与 Key 不配套或时间偏差；渠道验证结果会给出可读错误。
