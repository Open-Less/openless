# 录音优先与启动延迟

目标：快捷键按下后尽早采集麦克风；识别器或处理线程暂时未就绪时，保存已经采到的
PCM，而不是等待识别器后再开麦。缓冲按需增长、保留原有上限，不新增常驻录音或
第二套大队列。

## 采集与准备顺序

`PipelineDictationEngine::start` 会先提交 provisional
capture 或启动普通 recorder，再并行准备 `transcription.prepare()`。当前流程为：

```text
快捷键 / 正式启动
  → 打开或提交已有的麦克风采集
  → PCM 写入现有 BufferedTranscriptionSession
  → 准备识别器
  → 建连、按原顺序交付暂存 PCM
  → 直接交付后续 PCM
```

`CaptureFirstPreparation` 只缓存一次准备结果，复用原有的 PCM 缓冲。
稳定模式仍在启动返回成功前完成配置准备，但等停止录音后才连接识别器。
准备失败或用户取消时，会停止已启动的录音并清理会话。

这项调整对 Windows 的意义不只是提前调用 recorder：把 provisional capture 先提交
到正式缓冲，也能避免慢准备一直占用原来较小的暂存窗口。

## 资源边界

- 不默认开启 `lowLatencyDictationEnabled`；不扩大 500 ms 的既有预录选项。
- 没有新增 PCM 队列；复用原有 `BufferedTranscriptionSession` 和处理工作线程。
- 等待期间的 16 kHz、单声道、16-bit PCM 每秒约 32 KB，按实际采集量保存；
  缓冲不是启动时一次性分配到上限。达到既有上限仍走停止/错误路径。
- DSP 在音频工作线程中运行。设备回调的工作是格式转换和向既有有界队列交付。
- 这不恢复硬件开始提供数据之前的声音，也没有宣称消除 Windows 驱动冷启动。

## 诊断

Tauri 录音日志增加 `[audio-startup]` 阶段，只有时间和录音/按键代号，不包含录音内容：

- `press` 与 `arm_done_us`：从实际按键到捕获路径建立完成。
- `capture`：关联同一次原生流的各阶段。
- `device_selected`：设备选择耗时。
- `stream_built` / `stream_playing`：建流、play 耗时及距打开入口的时间。
- `first_raw_pcm`：第一个被接受的原生 PCM 回调，包括静音。
- `first_output_pcm`：第一个可交付给识别器/归档的处理结果。

比较 `first_raw_pcm` 与 `first_output_pcm` 可以区分硬件等待与处理/分帧等待。

## 播放回声消除

播放抑制是会话开始时冻结的独立设置。Windows 使用 WASAPI loopback，macOS 在 14.6 及以后使用 Core Audio 进程 tap，Linux 尝试匹配 PipeWire/Pulse 输出监视器。`audio/echo.rs` 对齐带时间戳的播放参考和麦克风采样。无可用参考、路由更改或权限受限时，仍保留普通麦克风处理。更换输出设备后开始新会话以重新选择参考源。播放参考只用于实时处理，录音归档和识别请求不包含参考音频。

信号夹具可以验证对齐、回声衰减和双讲保护。蓝牙延迟、设备切换、长录音时钟漂移和系统播放不被打断需要实际设备验证。源码与溯源见 [MicYou DSP 上游说明](../openless-all/app/crates/micyou-dsp/UPSTREAM.md)和 [#1223](https://github.com/Open-Less/openless/issues/1223)。
Windows 快捷键日志使用 `press` 和 `capture` 关联；预热路线标记为 `warm_hub`。

回归测试 `capture_starts_before_slow_preparation_and_cancellation_releases_it_promptly`
会故意阻塞识别器准备，验证麦克风已开始、开头 PCM 仍能顺序交付，并验证准备期间
的取消无需等待准备完成即可释放麦克风；另有准备失败清理和 Windows WASAPI
资源生命周期测试。完整 `openless-core` 测试与 Windows CI 覆盖这些路径。

本轮未在原生 Windows 麦克风上测量优化前后的 P50/P95，不报告未经测量的毫秒收益。
