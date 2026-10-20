use std::sync::Arc;

use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use openless_core::cloud_sync_e2ee_documents::{DocumentError, DocumentResult, SecretJson};
use openless_core::local_migration_bundle::{
    AndroidMigrationLease, AndroidMigrationSnapshot, AndroidMigrationState,
};

use super::jni::android::{
    call_static_string_with_context_class, call_static_void_with_context_class, jobject_str,
    with_android_env,
};

const CLASS: &str = "com.openless.app.OpenLessMigrationState";

#[derive(Clone)]
pub(crate) struct AndroidMigrationHost {
    vm: Arc<JavaVM>,
    context: GlobalRef,
}

impl AndroidMigrationHost {
    pub(crate) fn new(recovering: bool) -> Result<Self, String> {
        let host = with_android_env(|env, context| {
            let app = env
                .call_method(
                    context,
                    "getApplicationContext",
                    "()Landroid/content/Context;",
                    &[],
                )
                .and_then(|value| value.l())
                .map_err(|_| "migration_application_context_unavailable")?;
            Ok(Self {
                vm: Arc::new(env.get_java_vm().map_err(|_| "migration_jvm_unavailable")?),
                context: env
                    .new_global_ref(app)
                    .map_err(|_| "migration_application_context_unavailable")?,
            })
        })?;
        host.call(|env, context| {
            call_static_void_with_context_class(
                env,
                context,
                CLASS,
                "initialize",
                "(Landroid/content/Context;Z)V",
                &[JValue::Object(context), JValue::Bool(recovering.into())],
            )
        })
        .map_err(|error| error.to_string())?;
        Ok(host)
    }

    fn call<T>(
        &self,
        work: impl for<'local> FnOnce(&mut JNIEnv<'local>, &JObject<'local>) -> Result<T, String>,
    ) -> DocumentResult<T> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|_| DocumentError::CaptureFailed)?;
        let result = work(&mut env, self.context.as_obj());
        if env.exception_check().unwrap_or(true) {
            let exception = env.exception_occurred().ok();
            let _ = env.exception_clear();
            let message = exception
                .and_then(|exception| {
                    env.call_method(exception, "getMessage", "()Ljava/lang/String;", &[])
                        .ok()
                })
                .and_then(|value| value.l().ok())
                .filter(|value| !value.is_null())
                .and_then(|value| {
                    env.get_string(&JString::from(value))
                        .ok()
                        .map(|message| message.to_string_lossy().into_owned())
                });
            let _ = env.exception_clear();
            return Err(match message.as_deref() {
                Some("migration_bundle_stale_preview") => DocumentError::StalePreview,
                Some("migration_android_busy") => DocumentError::RuntimeBusy,
                _ => DocumentError::CaptureFailed,
            });
        }
        result.map_err(|_| DocumentError::CaptureFailed)
    }
}

impl AndroidMigrationState for AndroidMigrationHost {
    fn capture(&self) -> DocumentResult<AndroidMigrationSnapshot> {
        let json = self.call(|env, context| {
            call_static_string_with_context_class(
                env,
                context,
                CLASS,
                "capture",
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[JValue::Object(context)],
            )
        })?;
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|_| DocumentError::InvalidDocument)?;
        Ok(AndroidMigrationSnapshot {
            revision: value["revision"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or(DocumentError::InvalidDocument)?
                .into(),
            value: SecretJson::new(value["value"].clone()),
        })
    }

    fn acquire(
        &self,
        expected_revision: Option<&str>,
    ) -> DocumentResult<Box<dyn AndroidMigrationLease>> {
        let token = self.call(|env, context| {
            let expected = jobject_str(env, expected_revision.unwrap_or_default())?;
            call_static_string_with_context_class(
                env,
                context,
                CLASS,
                "acquire",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&expected)],
            )
        })?;
        Ok(Box::new(AndroidLease {
            host: self.clone(),
            token,
        }))
    }
}

struct AndroidLease {
    host: AndroidMigrationHost,
    token: String,
}

impl AndroidMigrationLease for AndroidLease {
    fn capture(&self) -> DocumentResult<SecretJson> {
        let json = self.host.call(|env, context| {
            let token = jobject_str(env, &self.token)?;
            call_static_string_with_context_class(
                env,
                context,
                CLASS,
                "captureLocked",
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(context), JValue::Object(&token)],
            )
        })?;
        serde_json::from_str(&json)
            .map(SecretJson::new)
            .map_err(|_| DocumentError::InvalidDocument)
    }

    fn replace(&mut self, value: &SecretJson) -> DocumentResult<()> {
        self.host.call(|env, context| {
            let token = jobject_str(env, &self.token)?;
            let json = jobject_str(env, &value.expose().to_string())?;
            call_static_void_with_context_class(
                env,
                context,
                CLASS,
                "replace",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                &[
                    JValue::Object(context),
                    JValue::Object(&token),
                    JValue::Object(&json),
                ],
            )
        })
    }

    fn finish(&mut self, recovery_required: bool) -> DocumentResult<()> {
        self.host.call(|env, context| {
            let token = jobject_str(env, &self.token)?;
            call_static_void_with_context_class(
                env,
                context,
                CLASS,
                "finish",
                "(Ljava/lang/String;Z)V",
                &[
                    JValue::Object(&token),
                    JValue::Bool(recovery_required.into()),
                ],
            )
        })
    }
}
