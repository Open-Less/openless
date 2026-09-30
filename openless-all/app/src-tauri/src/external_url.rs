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

    // Routed through the shared registered-Activity Context (see
    // android::jni::android::ACTIVE_CONTEXT's doc comment) rather than
    // deriving a Context here directly — both alternatives tried for that
    // (a once-only cached ndk_context registry, and tao's live-but-
    // resumed-only tracked-Activity map) have real failure modes for a
    // Context used outside the exact moment an Activity is both alive and
    // foregrounded.
    crate::android::jni::android::with_android_env(|env, context| {
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
            context,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )
        .map_err(|error| format!("start Android URL activity: {error}"))?;

        Ok(())
    })
}

#[cfg(not(target_os = "android"))]
fn platform_open_external_url(_url: &str) -> Result<(), String> {
    Err("native external URL fallback is only wired on Android".to_string())
}
