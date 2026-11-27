//! 所有 wgpu 设备共用的 `DeviceDescriptor`。
//!
//! wgpu 28 起 `MemoryHints` 的默认值是 `Performance`：wgpu-hal 的 Vulkan 子分配器
//! 会按 device 128–256 MB / host 64–128 MB 的内存块向驱动申请
//! （`wgpu-hal/src/lib.rs` 的 `AllocationSizes::from_memory_hints`）。对 egui 这种
//! 「一堆小 buffer + 小纹理」的负载，那块显存纯属预占：
//!
//! | hints | 单个渲染进程建 device 后的显存（GTX 1660 SUPER / NVIDIA 615.71.09） |
//! |---|---|
//! | `Performance`（wgpu 默认） | 195 MiB |
//! | `MemoryUsage`（这里） | 15 MiB |
//!
//! 帧率不受影响（实测胶囊 29.8 fps 上限、设置页 72.7 fps 两档都不变）。宿主、主窗、
//! 每个弹窗都是独立进程，各自建一个设备，所以这里统一按最低占用配置；egui 也要求
//! 2D 纹理上限能覆盖 4K 表面，这一点与 egui-wgpu 的默认保持一致。

use wgpu::{DeviceDescriptor, Limits, MemoryHints};

/// 与 egui-wgpu 默认的 `max_texture_dimension_2d` 一致：足够容纳 4K+ 表面。
const MAX_TEXTURE_DIMENSION_2D: u32 = 8192;

/// eframe 窗口与原生 layer-shell 胶囊共用的一份设备描述。
pub fn device_descriptor() -> DeviceDescriptor<'static> {
    DeviceDescriptor {
        label: Some("openless-wgpu-device"),
        required_limits: Limits {
            max_texture_dimension_2d: MAX_TEXTURE_DIMENSION_2D,
            ..Limits::default()
        },
        memory_hints: MemoryHints::MemoryUsage,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_descriptor_prefers_memory_usage_over_performance() {
        assert!(
            matches!(device_descriptor().memory_hints, MemoryHints::MemoryUsage),
            "Performance hints reserve a 128 MiB device block per process"
        );
    }

    #[test]
    fn device_descriptor_still_covers_4k_surfaces() {
        assert_eq!(
            device_descriptor().required_limits.max_texture_dimension_2d,
            MAX_TEXTURE_DIMENSION_2D
        );
        // 3840 宽是 egui 在 4K 表面下的硬需求；常量断言放在 const 块里，
        // 编译期就能拦住把上限调小的改动。
        const { assert!(MAX_TEXTURE_DIMENSION_2D >= 3840) };
    }

    #[test]
    fn device_descriptor_asks_for_no_optional_features() {
        // 只改显存策略：不能顺手把 wgpu 的实验特性打开。
        assert!(device_descriptor().required_features.is_empty());
    }
}
