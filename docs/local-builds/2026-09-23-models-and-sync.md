# 2026-09-23 本地模型与云同步界面检查

应用源码：`56a22be5`；构建版本：`2.0.0-Beta.2.local.2`；本机：macOS Apple Silicon。实时字幕、字幕组件和动效文件未修改。

## 模型目录与实际支持

本次在安装版实际查看到的 Mac 下载目录是 2 个 Qwen3-ASR 与 6 个 Whisper，未观察到 Foundry 或 sherpa-onnx 条目。Mac 的 Whisper 使用已编入应用的 whisper.cpp，Cargo 启用了 Metal 支持；不能仅因模型名称和 Windows 相同就判定为 Windows 专用，也不能把它标成仅 CPU。

原前端只按模型名开头判断平台，未知名称默认允许；通用模型 DTO 没有保留 Core 的 runtime。此次改为传递 runtime，结合 runtime/family 做平台过滤，对未知模型关闭默认放行；Windows 的两类独立目录也显式限制于 Windows。

| 模型运行时 | 当前应用支持的平台 | 页面标识 |
| --- | --- | --- |
| generic / Qwen3 | macOS、Linux | macOS 的 MLX/Metal 与 C/CPU 按实际能力显示；Linux 为 C/CPU |
| generic / Whisper | macOS | macOS · whisper.cpp |
| Foundry Local | Windows | Windows · Foundry Local |
| sherpa-onnx | Windows | Windows · sherpa-onnx |

## 真实推理验证

通过应用下载了官方 Qwen3-ASR 0.6B，下载流程完成文件校验和原子安装。使用本次构建包自身的 MLX worker，读取该模型运行英语内置样本与本机合成的中文样本；没有切换用户当前 ASR 渠道，也没有把测试音频发给云端 ASR。

| 样本 | 音频时长 | 推理耗时 | 结果 |
| --- | --- | --- | --- |
| 英语 | 3.642 秒 | 3.313 秒 | 正常返回完整句子，hello/test/speech/system 关键内容通过 |
| 中文 | 4.225 秒 | 1.201 秒 | `这是本地语音识别测试。今天的天气很好。` |

两次推理在同一 worker 中依次运行，中文为第二次调用；这些时间是本机样本观测，不是各模型/各硬件的延迟承诺。英语原始输出保存在结果文件中，未将关键内容检查当成逐字准确率测试。本次没有下载并实测全部 Whisper 型号或 Intel/Windows/Linux 设备。

`inference-result.json` 保存实际应用二进制 SHA-256；已确认它与 `/Applications/OpenLess.app` 正在使用的二进制一致。应用原生界面已读取到新的平台/运行时标签和已下载模型状态。

## 云同步

卡片改为 16px 段间距，标题和说明间距 8px，账号标签与用户名相邻，按钮保持 12px 间距。修改仅限云同步卡片，不改变全局字间距。

新加密云同步的需求、HTTP API、加密格式、OpenAPI 与客户端接口改动清单位于工作区 `5-cloud-sync/docs/`。它是独立的文档项目，尚无后端实现；现有手动 `/me/sync` 仍是旧协议，不能拿它明文上传 API 密钥。

## 检查与产物

- 90 个前端测试入口和生产构建通过，包含按运行时区分同名 Qwen/Whisper 的平台回归。
- macOS `.app` 与 DMG 构建、签名、MLX metallib 包内容及一致性检查通过。
- Qwen3-ASR 0.6B 的真实中英文 MLX 推理通过。
- 运行中的应用版本和二进制与本次测试包一致。

安装包、构建记录、测试记录、中文 WAV 与推理结果保存在工作区 `outputs/openless-2.0/2026-09-23/models-sync-ui/`。
