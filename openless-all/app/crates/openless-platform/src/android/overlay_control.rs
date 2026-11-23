//! Android 悬浮层控制（SHOW / HIDE / REPLACE_OVERLAY / REFRESH_LAYOUT）。
//!
//! 只发 Intent 给 Kotlin 的 `OpenLessOverlayService`，并维护"可见"标志。
//! `OVERLAY_VISIBLE` 由 Kotlin 侧 `onDestroy()` 通过 [`notify_overlay_destroyed`] 清零，
//! 否则刷新会打给一个已经死掉的服务。

use std::sync::atomic::AtomicBool;

static OVERLAY_VISIBLE: AtomicBool = AtomicBool::new(false);

pub fn show_overlay() -> Result<(), String> {
        {
        crate::android::jni::android::with_android_env(|env, context| {
            show_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn hide_overlay() -> Result<(), String> {
        {
        crate::android::jni::android::with_android_env(|env, context| {
            hide_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn replace_overlay() -> Result<(), String> {
        {
        crate::android::jni::android::with_android_env(|env, context| {
            replace_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn refresh_overlay_layout() -> Result<(), String> {
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

/// Called by the Kotlin overlay service's onDestroy() to synchronously clear
/// the OVERLAY_VISIBLE flag when the OS kills the service, keeping
/// refresh_overlay_if_visible() from sending commands to a dead service.
pub fn notify_overlay_destroyed() {
    OVERLAY_VISIBLE.store(false, std::sync::atomic::Ordering::SeqCst);
    log::info!("[android-native] overlay service destroyed — OVERLAY_VISIBLE reset");
}

