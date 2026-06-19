//! Shared JNI helpers for Android Rust modules.

#[cfg(target_os = "android")]
pub mod android {
    use jni::objects::{JClass, JObject, JString, JValue};
    use jni::JNIEnv;
    use jni::JavaVM;

    pub fn with_android_env<R>(
        f: impl for<'local> FnOnce(&mut JNIEnv<'local>, &JObject<'local>) -> Result<R, String>,
    ) -> Result<R, String> {
        let android_context = ndk_context::android_context();
        let vm = unsafe {
            JavaVM::from_raw(android_context.vm().cast())
                .map_err(|error| format!("attach Android JVM: {error}"))?
        };
        let mut env = vm
            .attach_current_thread()
            .map_err(|error| format!("attach Android thread: {error}"))?;
        let raw_context = android_context.context() as jni::sys::jobject;
        if raw_context.is_null() {
            return Err("Android context not yet initialized".to_string());
        }
        // SAFETY: raw_context is non-null and points to a valid Android Context object
        // provided by tao/Tauri; the reference lifetime is valid for the duration of `f`.
        let context = unsafe { JObject::from_raw(raw_context) };
        f(&mut env, &context)
    }

    pub fn call_static_void(
        env: &mut JNIEnv,
        class_name: &str,
        method: &str,
        sig: &str,
        args: &[JValue],
    ) -> Result<(), String> {
        let class = env
            .find_class(class_name)
            .map_err(|error| format!("find class {class_name}: {error}"))?;
        env.call_static_method(class, method, sig, args)
            .map_err(|error| format!("call {class_name}.{method}: {error}"))?;
        Ok(())
    }

    fn load_context_class<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
        class_name: &str,
    ) -> Result<JClass<'local>, String> {
        let class_loader = env
            .call_method(context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("get Context class loader: {error}"))?;
        let class_name_obj = jobject_str(env, class_name)?;
        let class_obj = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&class_name_obj)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("load app class {class_name}: {error}"))?;
        Ok(JClass::from(class_obj))
    }

    fn call_static_void_with_context_class<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
        class_name: &str,
        method: &str,
        sig: &str,
        args: &[JValue],
    ) -> Result<(), String> {
        let class = load_context_class(env, context, class_name)?;
        env.call_static_method(class, method, sig, args)
            .map_err(|error| format!("call {class_name}.{method}: {error}"))?;
        Ok(())
    }

    pub(crate) fn call_static_bool_with_context_class<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
        class_name: &str,
        method: &str,
        sig: &str,
        args: &[JValue],
    ) -> Result<bool, String> {
        let class = load_context_class(env, context, class_name)?;
        env.call_static_method(class, method, sig, args)
            .and_then(|value| value.z())
            .map_err(|error| format!("call {class_name}.{method}: {error}"))
    }

    pub fn jstring<'local>(
        env: &mut JNIEnv<'local>,
        value: &str,
    ) -> Result<JString<'local>, String> {
        env.new_string(value)
            .map_err(|error| format!("create jstring: {error}"))
    }

    pub(crate) fn jobject_str<'local>(
        env: &mut JNIEnv<'local>,
        value: &str,
    ) -> Result<JObject<'local>, String> {
        Ok(jstring(env, value)?.into())
    }

    pub fn start_activity_class(
        env: &mut JNIEnv,
        context: &JObject,
        class_name: &str,
    ) -> Result<(), String> {
        start_activity_class_with_flags(env, context, class_name, 0x10000000)
    }

    pub fn start_activity_class_with_flags(
        env: &mut JNIEnv,
        context: &JObject,
        class_name: &str,
        flags: i32,
    ) -> Result<(), String> {
        let intent = env
            .new_object("android/content/Intent", "()V", &[])
            .map_err(|error| format!("create activity intent: {error}"))?;
        let class_name_obj = jobject_str(env, class_name)?;
        let component = env
            .new_object(
                "android/content/ComponentName",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[JValue::Object(context), JValue::Object(&class_name_obj)],
            )
            .map_err(|error| format!("create component name: {error}"))?;
        env.call_method(
            &intent,
            "setComponent",
            "(Landroid/content/ComponentName;)Landroid/content/Intent;",
            &[JValue::Object(&component)],
        )
        .map_err(|error| format!("set activity component: {error}"))?;
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(flags)],
        )
        .map_err(|error| format!("set intent flags: {error}"))?;
        env.call_method(
            context,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )
        .map_err(|error| format!("start activity: {error}"))?;
        Ok(())
    }

    pub fn start_service_action(
        env: &mut JNIEnv,
        context: &JObject,
        service_class: &str,
        action: &str,
    ) -> Result<(), String> {
        let intent = env
            .new_object("android/content/Intent", "()V", &[])
            .map_err(|error| format!("create service intent: {error}"))?;
        let service_class_obj = jobject_str(env, service_class)?;
        let component = env
            .new_object(
                "android/content/ComponentName",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[JValue::Object(context), JValue::Object(&service_class_obj)],
            )
            .map_err(|error| format!("create component name: {error}"))?;
        env.call_method(
            &intent,
            "setComponent",
            "(Landroid/content/ComponentName;)Landroid/content/Intent;",
            &[JValue::Object(&component)],
        )
        .map_err(|error| format!("set service component: {error}"))?;
        let action_obj = jobject_str(env, action)?;
        env.call_method(
            &intent,
            "setAction",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&action_obj)],
        )
        .map_err(|error| format!("set service action: {error}"))?;
        let start_method =
            if action.ends_with(".START_RECORDING") && android_sdk_int(env)? >= 26 {
                "startForegroundService"
            } else {
                "startService"
            };
        env.call_method(
            context,
            start_method,
            "(Landroid/content/Intent;)Landroid/content/ComponentName;",
            &[JValue::Object(&intent)],
        )
        .map_err(|error| format!("{start_method}: {error}"))?;
        Ok(())
    }

    pub fn can_draw_overlays(env: &mut JNIEnv, context: &JObject) -> Result<bool, String> {
        if android_sdk_int(env)? < 23 {
            return Ok(true);
        }
        env.call_static_method(
            "android/provider/Settings",
            "canDrawOverlays",
            "(Landroid/content/Context;)Z",
            &[JValue::Object(context)],
        )
        .and_then(|value| value.z())
        .map_err(|error| format!("Settings.canDrawOverlays: {error}"))
    }

    pub fn check_self_permission(
        env: &mut JNIEnv,
        context: &JObject,
        permission: &str,
    ) -> Result<bool, String> {
        if android_sdk_int(env)? < 23 {
            return Ok(true);
        }
        let permission_obj = jobject_str(env, permission)?;
        let result = env
            .call_method(
                context,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&permission_obj)],
            )
            .and_then(|value| value.i())
            .map_err(|error| format!("Context.checkSelfPermission({permission}): {error}"))?;
        Ok(result == 0)
    }

    pub fn request_record_audio_permission<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
    ) -> Result<bool, String> {
        call_static_bool_with_context_class(
            env,
            context,
            "com.openless.app.OpenLessPermissionBridge",
            "requestRecordAudioPermission",
            "(Landroid/content/Context;)Z",
            &[JValue::Object(context)],
        )
    }

    pub fn launch_app_details_settings(env: &mut JNIEnv, context: &JObject) -> Result<(), String> {
        let action_obj = jobject_str(env, "android.settings.APPLICATION_DETAILS_SETTINGS")?;
        let null_obj = JObject::null();
        let package_name = env
            .call_method(context, "getPackageName", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("Context.getPackageName: {error}"))?;
        let package_prefix = jobject_str(env, "package")?;
        let uri = env
            .call_static_method(
                "android/net/Uri",
                "fromParts",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
                &[
                    JValue::Object(&package_prefix),
                    JValue::Object(&package_name),
                    JValue::Object(&null_obj),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Uri.fromParts(package): {error}"))?;
        start_settings_intent(env, context, &action_obj, Some(&uri))
    }

    pub fn launch_overlay_settings(env: &mut JNIEnv, context: &JObject) -> Result<(), String> {
        if android_sdk_int(env)? < 23 {
            return Ok(());
        }
        let action_obj = jobject_str(env, "android.settings.action.MANAGE_OVERLAY_PERMISSION")?;
        let null_obj = JObject::null();
        let package_name = env
            .call_method(context, "getPackageName", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("Context.getPackageName: {error}"))?;
        let package_prefix = jobject_str(env, "package")?;
        let uri = env
            .call_static_method(
                "android/net/Uri",
                "fromParts",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
                &[
                    JValue::Object(&package_prefix),
                    JValue::Object(&package_name),
                    JValue::Object(&null_obj),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Uri.fromParts(package): {error}"))?;
        start_settings_intent(env, context, &action_obj, Some(&uri))
    }

    pub fn android_sdk_int(env: &mut JNIEnv) -> Result<i32, String> {
        env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
            .and_then(|value| value.i())
            .map_err(|error| format!("read SDK_INT: {error}"))
    }

    /// 读取剪贴板当前的第一条纯文本内容，用于在粘贴后还原。
    /// 失败或剪贴板为空时返回 None（不返回错误，避免阻塞主流程）。
    pub fn get_primary_clip_text(
        env: &mut JNIEnv,
        context: &JObject,
    ) -> Option<String> {
        let clipboard_name = jobject_str(env, "clipboard").ok()?;
        let clipboard = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&clipboard_name)],
            )
            .and_then(|value| value.l())
            .ok()?;
        let clip = env
            .call_method(
                &clipboard,
                "getPrimaryClip",
                "()Landroid/content/ClipData;",
                &[],
            )
            .and_then(|value| value.l())
            .ok()?;
        if clip.is_null() {
            return None;
        }
        let item = env
            .call_method(
                &clip,
                "getItemAt",
                "(I)Landroid/content/ClipData$Item;",
                &[JValue::Int(0)],
            )
            .and_then(|value| value.l())
            .ok()?;
        if item.is_null() {
            return None;
        }
        let text_val = env
            .call_method(
                &item,
                "getText",
                "()Ljava/lang/CharSequence;",
                &[],
            )
            .and_then(|value| value.l())
            .ok()?;
        if text_val.is_null() {
            return None;
        }
        let text_str = env
            .call_method(&text_val, "toString", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .ok()?;
        let jstr = JString::from(text_str);
        env.get_string(&jstr)
            .map(|s| s.to_string_lossy().into_owned())
            .ok()
    }

    /// 将指定文本写回剪贴板，用于 accessibility 粘贴后还原用户原有内容。
    pub fn set_primary_clip_text(
        env: &mut JNIEnv,
        context: &JObject,
        text: &str,
    ) -> Result<(), String> {
        copy_to_clipboard(env, context, text).map(|_| ())
    }

    pub fn copy_to_clipboard(
        env: &mut JNIEnv,
        context: &JObject,
        text: &str,
    ) -> Result<bool, String> {
        let clipboard_name = jobject_str(env, "clipboard")?;
        let clipboard = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&clipboard_name)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("get clipboard service: {error}"))?;
        let label = jobject_str(env, "OpenLess")?;
        let text_obj = jobject_str(env, text)?;
        let clip = env
            .call_static_method(
                "android/content/ClipData",
                "newPlainText",
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
                &[JValue::Object(&label), JValue::Object(&text_obj)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("new ClipData: {error}"))?;
        env.call_method(
            &clipboard,
            "setPrimaryClip",
            "(Landroid/content/ClipData;)V",
            &[JValue::Object(&clip)],
        )
        .map_err(|error| format!("setPrimaryClip: {error}"))?;
        Ok(true)
    }

    pub fn notify_overlay_bridge<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
        state: &str,
        message: Option<&str>,
    ) -> Result<(), String> {
        let state_obj = jobject_str(env, state)?;
        let message_obj = jobject_str(env, message.unwrap_or(""))?;
        call_static_void_with_context_class(
            env,
            context,
            "com.openless.app.OpenLessOverlayBridge",
            "onCapsuleStateChanged",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&state_obj), JValue::Object(&message_obj)],
        )
    }

    pub fn show_overlay_toast<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
        message: &str,
    ) -> Result<(), String> {
        let message_obj = jobject_str(env, message)?;
        call_static_void_with_context_class(
            env,
            context,
            "com.openless.app.OpenLessOverlayBridge",
            "showToast",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&message_obj)],
        )
    }

    pub fn accessibility_paste<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
    ) -> Result<bool, String> {
        call_static_bool_with_context_class(
            env,
            context,
            "com.openless.app.OpenLessAccessibilityService",
            "pasteToFocusedField",
            "()Z",
            &[],
        )
    }

    pub fn accessibility_selected_text<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
    ) -> Result<Option<String>, String> {
        let class = load_context_class(
            env,
            context,
            "com.openless.app.OpenLessAccessibilityService",
        )?;
        let value = env
            .call_static_method(class, "captureSelectedText", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .map_err(|error| {
                format!("call com.openless.app.OpenLessAccessibilityService.captureSelectedText: {error}")
            })?;
        if value.is_null() {
            return Ok(None);
        }
        let text = env
            .get_string(&JString::from(value))
            .map_err(|error| format!("read selected text jstring: {error}"))?
            .to_string_lossy()
            .into_owned();
        if text.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(text))
        }
    }

    const ACCESSIBILITY_SERVICE_CLASS: &str = "com.openless.app.OpenLessAccessibilityService";
    const ACCESSIBILITY_PREFS_NAME: &str = "openless_accessibility";
    const ACCESSIBILITY_HEARTBEAT_KEY: &str = "last_heartbeat";
    const ACCESSIBILITY_HEARTBEAT_STALE_MS: i64 = 15_000;

    fn content_resolver<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
    ) -> Result<JObject<'local>, String> {
        env.call_method(context, "getContentResolver", "()Landroid/content/ContentResolver;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("Context.getContentResolver: {error}"))
    }

    fn jstring_object_to_option<'local>(
        env: &mut JNIEnv<'local>,
        value: JObject<'local>,
    ) -> Result<Option<String>, String> {
        if value.is_null() {
            return Ok(None);
        }
        let text = env
            .get_string(&JString::from(value))
            .map_err(|error| format!("read jstring: {error}"))?
            .to_string_lossy()
            .into_owned();
        Ok(Some(text))
    }

    fn settings_secure_get_int<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
        key: &str,
        default: i32,
    ) -> Result<i32, String> {
        let resolver = content_resolver(env, context)?;
        let key_obj = jobject_str(env, key)?;
        env.call_static_method(
            "android/provider/Settings$Secure",
            "getInt",
            "(Landroid/content/ContentResolver;Ljava/lang/String;I)I",
            &[
                JValue::Object(&resolver),
                JValue::Object(&key_obj),
                JValue::Int(default),
            ],
        )
        .and_then(|value| value.i())
        .map_err(|error| format!("Settings.Secure.getInt({key}): {error}"))
    }

    fn settings_secure_get_string<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
        key: &str,
    ) -> Result<Option<String>, String> {
        let resolver = content_resolver(env, context)?;
        let key_obj = jobject_str(env, key)?;
        let value = env
            .call_static_method(
                "android/provider/Settings$Secure",
                "getString",
                "(Landroid/content/ContentResolver;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&resolver), JValue::Object(&key_obj)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Settings.Secure.getString({key}): {error}"))?;
        jstring_object_to_option(env, value)
    }

    fn accessibility_service_component_id<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
    ) -> Result<String, String> {
        let package_name = env
            .call_method(context, "getPackageName", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("Context.getPackageName: {error}"))?;
        let package = env
            .get_string(&JString::from(package_name))
            .map_err(|error| format!("read package name: {error}"))?
            .to_string_lossy()
            .into_owned();
        Ok(format!("{package}/{ACCESSIBILITY_SERVICE_CLASS}"))
    }

    pub fn accessibility_enabled<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
    ) -> Result<bool, String> {
        // Read Settings.Secure directly — avoids Kotlin @JvmStatic drift on older APK dex.
        if settings_secure_get_int(env, context, "accessibility_enabled", 0)? != 1 {
            return Ok(false);
        }
        let services = settings_secure_get_string(env, context, "enabled_accessibility_services")?
            .unwrap_or_default();
        let component_id = accessibility_service_component_id(env, context)?;
        Ok(services.contains(&component_id))
    }

    pub fn accessibility_operational<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
    ) -> Result<bool, String> {
        if !accessibility_enabled(env, context)? {
            return Ok(false);
        }
        let prefs_name = jobject_str(env, ACCESSIBILITY_PREFS_NAME)?;
        let prefs = env
            .call_method(
                context,
                "getSharedPreferences",
                "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
                &[JValue::Object(&prefs_name), JValue::Int(0)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("Context.getSharedPreferences: {error}"))?;
        let heartbeat_key = jobject_str(env, ACCESSIBILITY_HEARTBEAT_KEY)?;
        let last_heartbeat = env
            .call_method(
                &prefs,
                "getLong",
                "(Ljava/lang/String;J)J",
                &[JValue::Object(&heartbeat_key), JValue::Long(0)],
            )
            .and_then(|value| value.j())
            .map_err(|error| format!("SharedPreferences.getLong: {error}"))?;
        if last_heartbeat <= 0 {
            return Ok(false);
        }
        let now = env
            .call_static_method(
                "java/lang/System",
                "currentTimeMillis",
                "()J",
                &[],
            )
            .and_then(|value| value.j())
            .map_err(|error| format!("System.currentTimeMillis: {error}"))?;
        Ok(now.saturating_sub(last_heartbeat) <= ACCESSIBILITY_HEARTBEAT_STALE_MS)
    }

    pub fn launch_accessibility_settings(
        env: &mut JNIEnv,
        context: &JObject,
    ) -> Result<(), String> {
        let action_obj = jobject_str(env, "android.settings.ACCESSIBILITY_SETTINGS")?;
        start_settings_intent(env, context, &action_obj, None)
    }

    fn start_settings_intent(
        env: &mut JNIEnv,
        context: &JObject,
        action_obj: &JObject,
        data_uri: Option<&JObject>,
    ) -> Result<(), String> {
        let intent = env
            .new_object(
                "android/content/Intent",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&action_obj)],
            )
            .map_err(|error| format!("create settings intent: {error}"))?;
        if let Some(uri) = data_uri {
            env.call_method(
                &intent,
                "setData",
                "(Landroid/net/Uri;)Landroid/content/Intent;",
                &[JValue::Object(uri)],
            )
            .map_err(|error| format!("set settings intent data: {error}"))?;
        }
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(0x10000000)],
        )
        .map_err(|error| format!("set intent flags: {error}"))?;
        env.call_method(
            context,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )
        .map_err(|error| format!("start settings activity: {error}"))?;
        Ok(())
    }

    pub fn export_jstring(env: &mut JNIEnv, value: &str) -> jni::sys::jstring {
        env.new_string(value)
            .map(|s| s.into_raw())
            .unwrap_or(std::ptr::null_mut())
    }

    pub fn export_jboolean(value: bool) -> jni::sys::jboolean {
        if value {
            1
        } else {
            0
        }
    }

    pub(crate) fn install_apk_from_path<'local>(
        env: &mut JNIEnv<'local>,
        context: &JObject<'local>,
        path_obj: &JObject<'local>,
    ) -> Result<bool, String> {
        call_static_bool_with_context_class(
            env,
            context,
            "com.openless.app.OpenLessUpdateInstaller",
            "installApk",
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[JValue::Object(context), JValue::Object(path_obj)],
        )
    }
}
