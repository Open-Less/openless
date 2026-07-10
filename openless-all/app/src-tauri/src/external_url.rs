pub fn open_external_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid URL: {error}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("unsupported URL scheme: {scheme}")),
    }

    platform_open_external_url(parsed.as_str())
}

#[cfg(target_os = "android")]
fn platform_open_external_url(url: &str) -> Result<(), String> {
    use jni::objects::{JObject, JValue};

    let android_context = ndk_context::android_context();
    let vm = unsafe {
        jni::JavaVM::from_raw(android_context.vm().cast())
            .map_err(|error| format!("attach Android JVM: {error}"))?
    };
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("attach Android thread: {error}"))?;
    let context = unsafe { JObject::from_raw(android_context.context() as jni::sys::jobject) };

    let action = env
        .new_string("android.intent.action.VIEW")
        .map_err(|error| format!("create Intent action: {error}"))?;
    let url = env
        .new_string(url)
        .map_err(|error| format!("create URL string: {error}"))?;
    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&JObject::from(url))],
        )
        .and_then(|value| value.l())
        .map_err(|error| format!("parse URL into Android Uri: {error}"))?;
    let intent = env
        .new_object(
            "android/content/Intent",
            "(Ljava/lang/String;Landroid/net/Uri;)V",
            &[JValue::Object(&JObject::from(action)), JValue::Object(&uri)],
        )
        .map_err(|error| format!("create Android Intent: {error}"))?;

    // Context may be an application context; NEW_TASK keeps startActivity valid there.
    env.call_method(
        &intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[JValue::Int(0x10000000)],
    )
    .map_err(|error| format!("set Android Intent flags: {error}"))?;
    env.call_method(
        &context,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&intent)],
    )
    .map_err(|error| format!("start Android URL activity: {error}"))?;

    Ok(())
}

#[cfg(not(target_os = "android"))]
fn platform_open_external_url(_url: &str) -> Result<(), String> {
    Err("native external URL fallback is only wired on Android".to_string())
}
