//! JNI bridge between Kotlin overlay code and Rust Coordinator.

use std::sync::{Arc, OnceLock};

use crate::coordinator::Coordinator;
use crate::types::{CapsulePayload, CapsuleState};

static COORDINATOR: OnceLock<Arc<Coordinator>> = OnceLock::new();
static OVERLAY_VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn register_android_coordinator(coordinator: Arc<Coordinator>) {
    let _ = COORDINATOR.set(coordinator);
}

pub fn notify_capsule_state(payload: &CapsulePayload) {
    #[cfg(target_os = "android")]
    {
        let state = capsule_state_name(payload.state);
        let message = payload.message.as_deref();
        if let Err(error) = crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::notify_overlay_bridge(env, context, state, message)
        }) {
            log::warn!("[android-native] notify overlay bridge failed: {error}");
        }
    }
    let _ = payload;
}

pub fn show_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::jni::android::with_android_env(|env, context| {
            show_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn hide_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::jni::android::with_android_env(|env, context| {
            hide_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn replace_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::jni::android::with_android_env(|env, context| {
            replace_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn refresh_overlay_layout() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::start_service_action(
                env,
                context,
                "com.openless.app.OpenLessOverlayService",
                "com.openless.app.overlay.REFRESH_LAYOUT",
            )
        })?;
    }
    Ok(())
}

pub fn refresh_overlay_if_visible() -> Result<(), String> {
    if is_overlay_visible() {
        refresh_overlay_layout()
    } else {
        Ok(())
    }
}

#[cfg(target_os = "android")]
fn show_overlay_with_context(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
) -> Result<(), String> {
    crate::android::jni::android::start_service_action(
        env,
        context,
        "com.openless.app.OpenLessOverlayService",
        "com.openless.app.overlay.SHOW",
    )?;
    OVERLAY_VISIBLE.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[cfg(target_os = "android")]
fn hide_overlay_with_context(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
) -> Result<(), String> {
    crate::android::jni::android::start_service_action(
        env,
        context,
        "com.openless.app.OpenLessOverlayService",
        "com.openless.app.overlay.HIDE",
    )?;
    OVERLAY_VISIBLE.store(false, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[cfg(target_os = "android")]
fn replace_overlay_with_context(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
) -> Result<(), String> {
    crate::android::jni::android::start_service_action(
        env,
        context,
        "com.openless.app.OpenLessOverlayService",
        "com.openless.app.overlay.REPLACE_OVERLAY",
    )?;
    OVERLAY_VISIBLE.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

pub fn is_overlay_visible() -> bool {
    OVERLAY_VISIBLE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Kotlin overlay service 的 onDestroy() 调用此函数，以便在 OS 杀死服务时
/// 同步清除 OVERLAY_VISIBLE 标志，避免 refresh_overlay_if_visible() 向死亡
/// 服务发送无效命令。
pub fn notify_overlay_destroyed() {
    OVERLAY_VISIBLE.store(false, std::sync::atomic::Ordering::SeqCst);
    log::info!("[android-native] overlay service destroyed — OVERLAY_VISIBLE reset");
}

pub fn overlay_trigger_mode_name() -> &'static str {
    let Some(coordinator) = COORDINATOR.get() else {
        return "background";
    };
    match coordinator.android_overlay_trigger() {
        crate::types::AndroidOverlayTrigger::Background => "background",
        crate::types::AndroidOverlayTrigger::Keyboard => "keyboard",
        crate::types::AndroidOverlayTrigger::Always => "always",
    }
}

fn spawn_start_dictation(translation: bool) {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        let result = if translation {
            coordinator.start_dictation_with_translation().await
        } else {
            coordinator.start_dictation().await
        };
        if let Err(error) = result {
            log::warn!(
                "[android-native] {} failed: {error}",
                if translation {
                    "start_dictation_with_translation"
                } else {
                    "start_dictation"
                }
            );
        }
    });
}

fn spawn_stop_dictation() {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = coordinator.stop_dictation().await {
            log::warn!("[android-native] stop_dictation failed: {error}");
        }
    });
}

fn spawn_stop_dictation_with_translation(translation: bool) {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = coordinator
            .stop_dictation_with_translation(translation)
            .await
        {
            log::warn!("[android-native] stop_dictation_with_translation failed: {error}");
        }
    });
}

fn spawn_cancel_dictation() {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        return;
    };
    coordinator.cancel_dictation();
}

fn spawn_switch_style_pack() {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    coordinator.switch_to_previous_style_pack();
}

fn spawn_open_qa_from_overlay() {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    log::info!("[android-native] open_qa_from_overlay requested");
    tauri::async_runtime::spawn(async move {
        if let Err(error) = coordinator.open_qa_from_overlay().await {
            log::warn!("[android-native] open_qa_from_overlay failed: {error}");
        }
    });
}

fn spawn_finalize_qa_from_overlay() {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    log::info!("[android-native] finalize_qa_from_overlay requested");
    tauri::async_runtime::spawn(async move {
        if let Err(error) = coordinator.finalize_qa_from_overlay().await {
            log::warn!("[android-native] finalize_qa_from_overlay failed: {error}");
        }
    });
}

fn capsule_state_name(state: CapsuleState) -> &'static str {
    match state {
        CapsuleState::Idle => "idle",
        CapsuleState::Recording => "recording",
        CapsuleState::Transcribing => "transcribing",
        CapsuleState::Polishing => "polishing",
        CapsuleState::Done => "done",
        CapsuleState::Cancelled => "cancelled",
        CapsuleState::Error => "error",
    }
}

#[cfg(target_os = "android")]
mod jni_exports {
    use super::*;
    use jni::objects::{JClass, JObject};
    use jni::sys::{jboolean, jstring, JNIEnv};
    use jni::JNIEnv as JniEnv;

    unsafe fn with_jni_context<R>(
        env_ptr: *mut JNIEnv,
        context: JObject,
        f: impl for<'local> FnOnce(&mut JniEnv<'local>, &JObject<'local>) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut env =
            JniEnv::from_raw(env_ptr).map_err(|error| format!("attach JNI env: {error}"))?;
        f(&mut env, &context)
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStartDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_start_dictation(false);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStartDictationWithTranslation(
        _env: *mut JNIEnv,
        _class: JClass,
        translation: jboolean,
    ) {
        spawn_start_dictation(translation != 0);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_stop_dictation();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopDictationWithTranslation(
        _env: *mut JNIEnv,
        _class: JClass,
        translation: jboolean,
    ) {
        spawn_stop_dictation_with_translation(translation != 0);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeCancelDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_cancel_dictation();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeSwitchStylePack(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_switch_style_pack();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeOpenQaFromOverlay(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_open_qa_from_overlay();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeFinalizeQaFromOverlay(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_finalize_qa_from_overlay();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeShowOverlay(
        env: *mut JNIEnv,
        _class: JClass,
        context: JObject,
    ) {
        let _ = with_jni_context(env, context, |env, context| {
            show_overlay_with_context(env, context)
        });
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeHideOverlay(
        env: *mut JNIEnv,
        _class: JClass,
        context: JObject,
    ) {
        let _ = with_jni_context(env, context, |env, context| {
            hide_overlay_with_context(env, context)
        });
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeCanDrawOverlays(
        env: *mut JNIEnv,
        _class: JClass,
        context: JObject,
    ) -> jboolean {
        let visible = with_jni_context(env, context, |env, context| {
            crate::android::jni::android::can_draw_overlays(env, context)
        })
        .unwrap_or(false);
        crate::android::jni::android::export_jboolean(visible)
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeIsOverlayVisible(
        _env: *mut JNIEnv,
        _class: JClass,
    ) -> jboolean {
        crate::android::jni::android::export_jboolean(is_overlay_visible())
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeGetOverlayTriggerMode(
        env: *mut JNIEnv,
        _class: JClass,
    ) -> jstring {
        let mode = overlay_trigger_mode_name();
        match JniEnv::from_raw(env) {
            Ok(mut env) => crate::android::jni::android::export_jstring(&mut env, mode),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeNotifyOverlayPermissionChanged(
        env: *mut JNIEnv,
        _class: JClass,
        context: JObject,
    ) {
        if overlay_trigger_mode_name() == "always" {
            let _ = with_jni_context(env, context, |env, context| {
                show_overlay_with_context(env, context)
            });
        }
    }

    /// 供 Kotlin overlay service 的 onDestroy() 调用，将 OVERLAY_VISIBLE 清除。
    /// 解决 OS 杀死服务时 Rust 端状态永久失同步的问题。
    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeNotifyOverlayDestroyed(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        notify_overlay_destroyed();
    }
}
