//! JNI bridge between Kotlin overlay code and Rust Coordinator.

use std::sync::{Arc, OnceLock};

use crate::coordinator::Coordinator;
use crate::types::{CapsulePayload, CapsuleState};

static COORDINATOR: OnceLock<Arc<Coordinator>> = OnceLock::new();
static OVERLAY_VISIBLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn register_android_coordinator(coordinator: Arc<Coordinator>) {
    let _ = COORDINATOR.set(coordinator);
}

pub fn notify_capsule_state(payload: &CapsulePayload) {
    #[cfg(target_os = "android")]
    {
        let state = capsule_state_name(payload.state);
        let message = payload.message.as_deref();
        if let Err(error) = crate::android_jni::android::with_android_env(|env, _context| {
            crate::android_jni::android::notify_overlay_bridge(env, state, message)
        }) {
            log::warn!("[android-native] notify overlay bridge failed: {error}");
        }
    }
    let _ = payload;
}

pub fn show_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android_jni::android::with_android_env(|env, context| {
            show_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn hide_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android_jni::android::with_android_env(|env, context| {
            hide_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

#[cfg(target_os = "android")]
fn show_overlay_with_context(env: &mut jni::JNIEnv, context: &jni::objects::JObject) -> Result<(), String> {
    crate::android_jni::android::start_service_action(
        env,
        context,
        "com.openless.app.OpenLessOverlayService",
        "com.openless.app.overlay.SHOW",
    )?;
    OVERLAY_VISIBLE.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[cfg(target_os = "android")]
fn hide_overlay_with_context(env: &mut jni::JNIEnv, context: &jni::objects::JObject) -> Result<(), String> {
    crate::android_jni::android::start_service_action(
        env,
        context,
        "com.openless.app.OpenLessOverlayService",
        "com.openless.app.overlay.HIDE",
    )?;
    OVERLAY_VISIBLE.store(false, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

pub fn is_overlay_visible() -> bool {
    OVERLAY_VISIBLE.load(std::sync::atomic::Ordering::SeqCst)
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

fn spawn_dictation(start: bool) {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        let result = if start {
            coordinator.start_dictation().await
        } else {
            coordinator.stop_dictation().await
        };
        if let Err(error) = result {
            log::warn!(
                "[android-native] {} failed: {error}",
                if start { "start_dictation" } else { "stop_dictation" }
            );
        }
    });
}

fn spawn_cancel_dictation() {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        return;
    };
    coordinator.cancel_dictation();
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
        let mut env = JniEnv::from_raw(env_ptr)
            .map_err(|error| format!("attach JNI env: {error}"))?;
        f(&mut env, &context)
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStartDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_dictation(true);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_dictation(false);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeCancelDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_cancel_dictation();
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
            crate::android_jni::android::can_draw_overlays(env, context)
        })
        .unwrap_or(false);
        crate::android_jni::android::export_jboolean(visible)
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeIsOverlayVisible(
        _env: *mut JNIEnv,
        _class: JClass,
    ) -> jboolean {
        crate::android_jni::android::export_jboolean(is_overlay_visible())
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeGetOverlayTriggerMode(
        env: *mut JNIEnv,
        _class: JClass,
    ) -> jstring {
        let mode = overlay_trigger_mode_name();
        match JniEnv::from_raw(env) {
            Ok(mut env) => crate::android_jni::android::export_jstring(&mut env, mode),
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
}
