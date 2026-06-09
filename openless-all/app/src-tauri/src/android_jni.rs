//! Shared JNI helpers for Android Rust modules.

#[cfg(target_os = "android")]
pub mod android {
    use jni::objects::{JObject, JString, JValue};
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
        let context = unsafe { JObject::from_raw(android_context.context() as jni::sys::jobject) };
        f(&mut env, &context)
    }

    pub fn call_static_bool(
        env: &mut JNIEnv,
        class_name: &str,
        method: &str,
        sig: &str,
        args: &[JValue],
    ) -> Result<bool, String> {
        let class = env
            .find_class(class_name)
            .map_err(|error| format!("find class {class_name}: {error}"))?;
        env.call_static_method(class, method, sig, args)
            .and_then(|value| value.z())
            .map_err(|error| format!("call {class_name}.{method}: {error}"))
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

    pub fn jstring<'local>(env: &mut JNIEnv<'local>, value: &str) -> Result<JString<'local>, String> {
        env.new_string(value)
            .map_err(|error| format!("create jstring: {error}"))
    }

    fn java_string(env: &mut JNIEnv, obj: JObject) -> Result<String, String> {
        let jstr = JString::from(obj);
        env.get_string(&jstr)
            .map(|value| value.into())
            .map_err(|error| format!("decode jstring: {error}"))
    }

    pub fn start_activity_class(
        env: &mut JNIEnv,
        context: &JObject,
        class_name: &str,
    ) -> Result<(), String> {
        let intent = env
            .new_object("android/content/Intent", "()V", &[])
            .map_err(|error| format!("create activity intent: {error}"))?;
        let component = env
            .new_object(
                "android/content/ComponentName",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[
                    JValue::Object(context),
                    JValue::Object(&jstring(env, class_name)?),
                ],
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
            &[JValue::Int(0x10000000)],
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
        let component = env
            .new_object(
                "android/content/ComponentName",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[
                    JValue::Object(context),
                    JValue::Object(&jstring(env, service_class)?),
                ],
            )
            .map_err(|error| format!("create component name: {error}"))?;
        env.call_method(
            &intent,
            "setComponent",
            "(Landroid/content/ComponentName;)Landroid/content/Intent;",
            &[JValue::Object(&component)],
        )
        .map_err(|error| format!("set service component: {error}"))?;
        env.call_method(
            &intent,
            "setAction",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&jstring(env, action)?)],
        )
        .map_err(|error| format!("set service action: {error}"))?;
        if android_sdk_int(env)? >= 26 {
            env.call_method(
                context,
                "startForegroundService",
                "(Landroid/content/Intent;)Landroid/content/ComponentName;",
                &[JValue::Object(&intent)],
            )
            .map_err(|error| format!("startForegroundService: {error}"))?;
        } else {
            env.call_method(
                context,
                "startService",
                "(Landroid/content/Intent;)Landroid/content/ComponentName;",
                &[JValue::Object(&intent)],
            )
            .map_err(|error| format!("startService: {error}"))?;
        }
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

    pub fn android_sdk_int(env: &mut JNIEnv) -> Result<i32, String> {
        env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
            .and_then(|value| value.i())
            .map_err(|error| format!("read SDK_INT: {error}"))
    }

    pub fn copy_to_clipboard(env: &mut JNIEnv, context: &JObject, text: &str) -> Result<bool, String> {
        let clipboard = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&jstring(env, "clipboard")?)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("get clipboard service: {error}"))?;
        let item = env
            .call_static_method(
                "android/content/ClipData",
                "newPlainText",
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData$Item;",
                &[
                    JValue::Object(&jstring(env, "OpenLess")?),
                    JValue::Object(&jstring(env, text)?),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("newPlainText: {error}"))?;
        let clip = env
            .call_static_method(
                "android/content/ClipData",
                "newPlainText",
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
                &[
                    JValue::Object(&jstring(env, "OpenLess")?),
                    JValue::Object(&jstring(env, text)?),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("new ClipData: {error}"))?;
        let _ = item;
        env.call_method(
            &clipboard,
            "setPrimaryClip",
            "(Landroid/content/ClipData;)V",
            &[JValue::Object(&clip)],
        )
        .map_err(|error| format!("setPrimaryClip: {error}"))?;
        Ok(true)
    }

    pub fn notify_overlay_bridge(env: &mut JNIEnv, state: &str, message: Option<&str>) -> Result<(), String> {
        call_static_void(
            env,
            "com/openless/app/OpenLessOverlayBridge",
            "onCapsuleStateChanged",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[
                JValue::Object(&jstring(env, state)?),
                JValue::Object(&jstring(env, message.unwrap_or(""))?),
            ],
        )
    }

    pub fn show_overlay_toast(env: &mut JNIEnv, message: &str) -> Result<(), String> {
        call_static_void(
            env,
            "com/openless/app/OpenLessOverlayBridge",
            "showToast",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&jstring(env, message)?)],
        )
    }

    pub fn ime_commit_text(env: &mut JNIEnv, text: &str) -> Result<bool, String> {
        call_static_bool(
            env,
            "com/openless/app/OpenLessImeService",
            "commitText",
            "(Ljava/lang/String;)Z",
            &[JValue::Object(&jstring(env, text)?)],
        )
    }

    pub fn accessibility_paste(env: &mut JNIEnv) -> Result<bool, String> {
        call_static_bool(
            env,
            "com/openless/app/OpenLessAccessibilityService",
            "pasteToFocusedField",
            "()Z",
            &[],
        )
    }

    pub fn accessibility_enabled(env: &mut JNIEnv, context: &JObject) -> Result<bool, String> {
        call_static_bool(
            env,
            "com/openless/app/OpenLessAccessibilityService",
            "isEnabled",
            "(Landroid/content/Context;)Z",
            &[JValue::Object(context)],
        )
    }

    pub fn launch_accessibility_settings(env: &mut JNIEnv, context: &JObject) -> Result<(), String> {
        let intent = env
            .new_object(
                "android/content/Intent",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&jstring(
                    env,
                    "android.settings.ACCESSIBILITY_SETTINGS",
                )?)],
            )
            .map_err(|error| format!("create accessibility settings intent: {error}"))?;
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
        .map_err(|error| format!("start accessibility settings: {error}"))?;
        Ok(())
    }

    pub fn launch_input_method_settings(env: &mut JNIEnv, context: &JObject) -> Result<(), String> {
        let intent = env
            .new_object(
                "android/content/Intent",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&jstring(
                    env,
                    "android.settings.INPUT_METHOD_SETTINGS",
                )?)],
            )
            .map_err(|error| format!("create IME settings intent: {error}"))?;
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
        .map_err(|error| format!("start IME settings: {error}"))?;
        Ok(())
    }

    pub fn ime_status(env: &mut JNIEnv, context: &JObject) -> Result<(bool, bool), String> {
        let imm = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&jstring(env, "input_method")?)],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("get InputMethodManager: {error}"))?;
        let package_obj = env
            .call_method(context, "getPackageName", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("getPackageName: {error}"))?;
        let package = java_string(env, package_obj)?;
        let service_id = format!("{package}/.OpenLessImeService");
        let enabled_list = env
            .call_method(&imm, "getEnabledInputMethodList", "()Ljava/util/List;", &[])
            .and_then(|value| value.l())
            .map_err(|error| format!("getEnabledInputMethodList: {error}"))?;
        let enabled = list_contains_id(env, &enabled_list, &service_id)?;
        let current = env
            .call_method(
                &imm,
                "getCurrentInputMethodInfo",
                "()Landroid/view/inputmethod/InputMethodInfo;",
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| format!("getCurrentInputMethodInfo: {error}"))?;
        let selected = if current.is_null() {
            false
        } else {
            let id_obj = env
                .call_method(&current, "getId", "()Ljava/lang/String;", &[])
                .and_then(|value| value.l())
                .map_err(|error| format!("getId: {error}"))?;
            let id = java_string(env, id_obj)?;
            id == service_id
        };
        Ok((enabled, selected))
    }

    fn list_contains_id(env: &mut JNIEnv, list: &JObject, id: &str) -> Result<bool, String> {
        let size = env
            .call_method(list, "size", "()I", &[])
            .and_then(|value| value.i())
            .map_err(|error| format!("list.size: {error}"))?;
        for index in 0..size {
            let item = env
                .call_method(
                    list,
                    "get",
                    "(I)Ljava/lang/Object;",
                    &[JValue::Int(index)],
                )
                .and_then(|value| value.l())
                .map_err(|error| format!("list.get: {error}"))?;
            let item_id_obj = env
                .call_method(&item, "getId", "()Ljava/lang/String;", &[])
                .and_then(|value| value.l())
                .map_err(|error| format!("InputMethodInfo.getId: {error}"))?;
            let item_id = java_string(env, item_id_obj)?;
            if item_id == id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn export_jstring(env: &mut JNIEnv, value: &str) -> jni::sys::jstring {
        env.new_string(value)
            .map(|s| s.into_raw())
            .unwrap_or(std::ptr::null_mut())
    }

    pub fn export_jboolean(value: bool) -> jni::sys::jboolean {
        if value { 1 } else { 0 }
    }
}
