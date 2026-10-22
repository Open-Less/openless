# Issue #1026：MicYou 音频增强接入审查

审查日期：2026-10-20。工作分支：`fix/issue-1026-micyou-audio`，基于 `origin/beta` 的 `101f5142`。

## 改动前实现

Issue：https://github.com/Open-Less/openless/issues/1026

关闭该 issue 的 PR：https://github.com/Open-Less/openless/pull/1194

`openless-all/app/crates/openless-core/src/audio.rs` 中的 `NoiseSuppressor`
按回调音频块的下混 RMS 估计底噪，通过迟滞、保持时间和增益平滑控制衰减。
这是噪声门类型的处理；语音打开门后，不会从语音中分离背景噪声。
`AudioEnhancer` 随后施加固定 1.5 倍增益和软限幅，没有自动增益控制。
当前路径由 Tauri 与 Linux egui 的本机录音适配器调用，不能据此认定移动端也已覆盖。

## 上游核对

MicYou：https://github.com/MicYou-Dev/MicYou

核对版本：`d821da032e2b72330ced09678dd127acd2714bef`。

源码：`tauri-app/crates/micyou-audio/src/dsp.rs` 与该 crate 的 `Cargo.toml`。

上游支持 RNNoise（`nnnoiseless`）、PureVox 模型降噪、自动增益、VAD、
均衡器、去混响和需要远端参考音频的 AEC。PureVox/AEC 使用 `micyou-infer`
及嵌入模型，并非仅复制一个函数就能使用。

需要区别功能名称与算法：上游 `Speexdsp` 分支调用自己的 `SpeexStyleNS`；
不能仅凭名称推断已接入官方 SpeexDSP。去混响分支标注为延迟线梳状滤波。

RNNoise 每帧 480 点，处理链以 48 kHz 为工作采样率；OpenLess 输出契约则为
16 kHz 单声道 PCM。上游 `process_rnnoise_single_channel` 会将不足一帧的余数
原样输出并清空。上游主链在调用前已聚齐完整帧；移植时必须保留这层契约，
不能孤立搬走辅助函数。

## 建议的实现顺序

1. 将处理链明确为：输入下混 → 有状态重采样到 48 kHz → 固定 10 ms 分帧 →
   模型降噪 → 有上限的平滑 AGC → 限幅 → 有状态重采样到 16 kHz。
2. 先完成 RNNoise 后端、跨回调余数缓存和停止录音时的收尾协议，再接入 PureVox。
   收尾需要处理模型延迟，记录有效样本数，防止补零增加录音长度或丢失尾音。
3. 将算法与强度、AGC 设置纳入会话冻结配置，并接通持久化、Tauri 和 egui 设置。
   默认关闭；关闭时沿用原有音频输出兼容性。
4. PureVox 单独核对模型许可、资源体积、推理负载与各平台构建；模型不可用时需要
   明确反馈或明确记录降级结果。复用代码保留上游版权和许可说明。
5. AEC 需要扬声器参考流与同步。当前本机录音入口只有麦克风流，先完成参考流设计，
   再提供可用的 AEC 开关。移动端需要逐个核实实际录音链后接入。

## 验证要求

- 相同输入以不同回调长度送入，输出长度、内容和收尾保持一致。
- 覆盖 16/44.1/48 kHz、单双声道、短于一帧、非有限数值与会话重置。
- 对同一组干净语音和语音混噪样本比较关闭、旧实现、RNNoise 与 PureVox。
  记录噪声衰减、语音损伤、识别字错率、首字/尾字保留和处理耗时。
- 分别检查实时识别、最终录音与历史回听，以及 Tauri、egui、移动端实际覆盖情况。

## 已落地的实现

- 用固定版本 MicYou DSP 与纯 Rust 推理模块替换旧噪声门路径，保留原版权和许可证。
- 支持 RNNoise、PureVox 6、基础频谱减法及关闭降噪；可调强度、手动增益、AGC
  目标与速度、语音门限、十段 EQ、梳状滤波、处理阶段顺序及一键恢复默认值。
- 增强总开关仍默认关闭。开启后的默认方案为 RNNoise 80% + AGC，EQ、梳状滤波和
  语音门限默认关闭。设置按录音会话冻结。
- 下混、抗混叠重采样、480 点分帧、干湿信号延迟对齐、模型收尾与有效样本数限制。
  不复用会增删样本的播放缓冲管理。
- Tauri、egui 与外部 PCM 录音入口共用处理链；鸿蒙主应用到 IME 的语音设置交换
  包含新参数。实时识别与归档消费同一份处理后的 PCM。
- 增加录音停止和暖录音会话边界的收尾，避免丢失不足一帧的尾音。
- 对启用降噪后的 AGC 增益加入活动约束：RNNoise 使用语音概率，PureVox 使用
  对齐后的湿/干信号能量比，避免 AGC 重新放大被模型拒绝的底噪。

AEC7 模型和带参考信号的 DSP 接口已保留并通过合成回声测试。目前宿主没有同步
扬声器参考采集，因此应用中未启用 AEC，也不提供无效开关。网络传输、虚拟声卡、
插件宿主和播放缓冲属于 MicYou 的麦克风转发产品功能，不适合直接并入识别录音链。

## 验证与限制

- 最终 Core 全量回归：1326 通过、3 忽略，包含音频、归档、配置和采集优先的回归。
- 音频分块一致性覆盖 8/16/44.1/48/96 kHz，含 RNNoise、PureVox、频谱减法和无降噪。
- 验证短录音、尾部脉冲、非有限数值、立体声跨回调余数、重置和重复停止。
- 增加默认 AGC 与降噪组合的底噪回归：修复前合成样本输出/输入功率约 0.875；
  修复后 RNNoise 与 PureVox 均通过输出功率小于输入 25% 的断言。
- 生产 Tauri 录音模块通过独立宿主测试；egui 检查、鸿蒙宿主交叉检查、鸿蒙配置
  沙箱测试通过。DSP 引擎 Windows、Android、鸿蒙交叉检查通过。
- 前端生产构建与完整 `npm test` 的 145 个测试文件通过，浏览器预览验证算法切换与设置面板。
- 完整 Tauri Windows 检查受现有 C 依赖交叉工具链限制；Linux Tauri 本就被仓库禁止。
  本轮没有原生 Windows/macOS 或手机上的麦克风实测。

发布模式纯 DSP 合成吞吐检查，5 秒输入：RNNoise 约 0.039 秒、PureVox 约 0.57 秒。
这是本机算法吞吐，不包括音频设备、重采样、网络或识别服务的耗时。
尚未完成真实语音识别字错率对比，不能据此声称识别率提升。

离线 A/B 工具（输入为 16 kHz 单声道 PCM WAV，输出文件必须不存在）：

```sh
cargo run --release --manifest-path openless-all/app/Cargo.toml -p openless-core \
  --example enhance_audio -- input.wav output.wav rnnoise
```

最后一个参数支持 `rnnoise`、`purevox`、`spectral`、`off`。
`off` 只关闭降噪阶段，仍运行默认 AGC/限幅；原始输入 WAV 是完全关闭增强的对照。

## 持续跟进与采集优先

- `scripts/micyou-upstream.json` 固定上游 revision 与源文件/模型 SHA-256。
- `scripts/micyou-dsp.patch` 单独记录 OpenLess 适配；同步时先在临时目录应用补丁，
  冲突时不写入工作区。同步器的 6 项离线测试与固定版本重放检查通过。
- `.github/workflows/sync-micyou.yml` 每日检查相关上游改动，通过模型与 Core 测试后
  创建/更新 draft PR。未启用自动合并；接口、依赖或补丁冲突仍需审查。
- 工作流合入默认分支且仓库允许 Actions 创建 PR 后才会生效；GITHUB_TOKEN 创建的
  PR 不自动触发独立 PR 工作流，合并前需运行正常原生平台 CI。
- `PipelineDictationEngine` 已调整为先采集，再准备识别器。慢准备期间复用原缓冲，
  不添加第二套 PCM 队列，也不默认启用常驻预热。详见 [启动延迟说明](audio-startup-latency.md)。
