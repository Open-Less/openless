//! Android JNI 基础（宿主薄封装）。
//!
//! JNI 助手与无障碍服务通道已迁到 `openless-platform::android`；宿主只在启动时
//! 注入 JavaVM 与 Activity context（`openless_platform::android::context::install`）。

pub use openless_platform::android::jni::android::*;
