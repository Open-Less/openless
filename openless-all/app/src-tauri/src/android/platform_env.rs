//! 平台层 `AndroidEnv` 的宿主实现。
//!
//! 平台层（`openless-platform`）不直接链接 JNI，也不持有 `Application` context：
//! 那是宿主的所有权。这里把宿主已有的 JNI 能力包装成 `AndroidEnv`，启动时注册一次，
//! 之后平台层的剪贴板 / 权限 / 文本插入 / 选区门面都会走这条路径。

use openless_platform::android::{install_env, AndroidEnv};

struct HostAndroidEnv;

impl AndroidEnv for HostAndroidEnv {
    fn check_permission(&self, permission: &str) -> Result<bool, String> {
        crate::android::jni::with_android_env(|env, context| {
            crate::android::jni::check_self_permission(env, context, permission)
        })
    }

    fn request_permission(&self, permission: &str) -> Result<bool, String> {
        // 目前只有录音权限有请求桥；其它权限由系统设置页处理。
        if permission != "android.permission.RECORD_AUDIO" {
            return Err(format!("no runtime request bridge for {permission}"));
        }
        crate::android::jni::with_android_env(|env, context| {
            crate::android::jni::request_record_audio_permission(env, context)
        })
    }

    fn clipboard_read_text(&self) -> Result<Option<String>, String> {
        crate::android::jni::with_android_env(|env, context| {
            Ok(crate::android::jni::get_primary_clip_text(
                env, context,
            ))
        })
    }

    fn clipboard_write_text(&self, text: &str) -> Result<(), String> {
        crate::android::jni::with_android_env(|env, context| {
            crate::android::jni::copy_to_clipboard(env, context, text).map(|_| ())
        })
    }

    fn insert_via_accessibility(&self, text: &str) -> Result<bool, String> {
        // 无障碍插入是"一次性粘贴"，把文本放上剪贴板后由无障碍服务粘贴。
        crate::android::jni::with_android_env(|env, context| {
            crate::android::jni::copy_to_clipboard(env, context, text)
        })?;
        Ok(crate::android::accessibility::paste_via_accessibility())
    }

    fn read_selection_via_accessibility(&self) -> Result<Option<String>, String> {
        // 尚未接入：Android 的选区读取目前走 QA 显式选区通道。
        Ok(None)
    }
}

/// 注册平台层的 Android 环境。启动时调用一次。
pub(crate) fn install() {
    install_env(Box::new(HostAndroidEnv));
}
