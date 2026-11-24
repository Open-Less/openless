//! Android JavaVM / Activity context 的注入点。
//!
//! 持有它们的只能是宿主（tao/tauri 的 Android runtime）。平台层只保存两个裸指针，
//! 并保证只在 JNI 调用期间借用。

use std::ffi::c_void;
use std::sync::OnceLock;

/// 宿主启动后注入的 Android 运行环境。
#[derive(Clone, Copy, Debug)]
pub struct AndroidContext {
    /// `JavaVM*`
    pub java_vm: *mut c_void,
    /// Activity 的 `jobject`（全局引用，宿主保证生命周期覆盖进程）。
    pub context_jobject: *mut c_void,
}

// SAFETY: 两个指针都是进程级、只读使用的运行时句柄；JNI 的线程附着由调用方负责。
unsafe impl Send for AndroidContext {}
unsafe impl Sync for AndroidContext {}

static CONTEXT: OnceLock<AndroidContext> = OnceLock::new();

/// 注册 Android 运行环境。宿主启动时调用一次；重复调用只有第一次生效。
pub fn install(context: AndroidContext) {
    let _ = CONTEXT.set(context);
}

/// 取已注册的 Android 运行环境。
pub fn get() -> Option<&'static AndroidContext> {
    CONTEXT.get()
}

/// 初始化 `ndk-context`，让依赖它的原生库（Oboe / AAudio 等）能拿到 JavaVM 与 Context。
///
/// 同一个进程只能初始化一次；重复调用会被忽略（`ndk-context` 自己也只接受第一次），
/// 因此这里返回是否由本次调用完成初始化。
pub fn install_ndk_context() -> bool {
    let Some(context) = get() else {
        log::warn!("[android] JNI context unavailable; ndk-context not initialized");
        return false;
    };
    // SAFETY: 两个指针由宿主提供且生命周期覆盖进程；ndk-context 只保存它们。
    let result = std::panic::catch_unwind(|| unsafe {
        ndk_context::initialize_android_context(context.java_vm, context.context_jobject);
    });
    match result {
        Ok(()) => true,
        Err(_) => {
            log::warn!("[android] ndk-context was already initialized or rejected initialization");
            false
        }
    }
}
