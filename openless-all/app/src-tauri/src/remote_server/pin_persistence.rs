//! 配对 PIN 的落盘（宿主薄封装）。
//!
//! 平台加固的原子文件写入已迁到 `openless-platform::storage::secure_file`。

pub(super) use openless_platform::storage::secure_file::{
    load_or_create_pin_at_path, persist_pin_atomically,
};
