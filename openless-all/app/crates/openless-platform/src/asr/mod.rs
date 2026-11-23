//! 本地 ASR 的平台侧部分。
//!
//! 拆成两层：
//! - **纯逻辑**（本模块）：provider id 元数据、模型文件收据/安装状态、sherpa 运行时事件描述。
//!   这些不依赖任何宿主类型，也不链接原生 SDK，因此可以独立编译与测试。
//! - **原生运行时**（后续迁移）：sherpa-onnx / Foundry SDK / Apple Speech，需要各自的
//!   原生库与 build 脚本，按 `asr-sherpa` / `asr-foundry` / `asr-apple` 子 feature 门控。
//!
//! 宿主仍然拥有 `TranscriptionEngine` 端口实现与引擎选择策略；平台层只提供
//! "模型在哪、装没装、运行时阶段叫什么" 这类事实。

pub mod ids;
pub mod receipt;

// sherpa 的**描述层**（阶段枚举、事件负载）是纯逻辑，随 `asr` 一起编译；
// 原生运行时要链接 sherpa-onnx，留在宿主的 `local_asr_sherpa` cfg 下，后续迁入。
pub mod sherpa;
