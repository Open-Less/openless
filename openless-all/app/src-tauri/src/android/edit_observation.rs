use jni::{
    objects::{JClass, JString, JValue},
    sys::{jboolean, jlong},
    JNIEnv,
};
use openless_core::host_document::{EditPair, ObservedInsertion};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

struct Observation {
    generation: u64,
    text: String,
    anchor: Option<ObservedInsertion>,
    started: Instant,
    lifetime: Duration,
    callback: Box<dyn Fn(EditPair) -> bool + Send + Sync>,
}
static OBSERVATION: Mutex<Option<Observation>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn arm(
    text: String,
    lifetime: Duration,
    callback: Box<dyn Fn(EditPair) -> bool + Send + Sync>,
) -> Option<u64> {
    let generation = NEXT.fetch_add(1, Ordering::AcqRel);
    *OBSERVATION.lock().ok()? = Some(Observation {
        generation,
        text,
        anchor: None,
        started: Instant::now(),
        lifetime,
        callback,
    });
    let result = super::jni::android::with_android_env(|env, context| {
        let class = super::jni::android::load_context_class(
            env,
            context,
            "com.openless.app.OpenLessAccessibilityService",
        )?;
        env.call_static_method(
            class,
            "armVocabularyObservation",
            "(J)V",
            &[JValue::Long(generation as i64)],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    });
    if result.is_err() {
        disarm(generation);
        return None;
    }
    // A disconnected accessibility process may never answer. Release retained
    // text even when no callback arrives to enforce the deadline for us.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let unanchored = OBSERVATION.lock().ok().is_some_and(|slot| {
            slot.as_ref()
                .is_some_and(|o| o.generation == generation && o.anchor.is_none())
        });
        if unanchored {
            disarm(generation);
            return;
        }
        tokio::time::sleep(lifetime.saturating_sub(Duration::from_secs(1))).await;
        disarm(generation);
    });
    Some(generation)
}

pub fn disarm(generation: u64) {
    let removed = if let Ok(mut current) = OBSERVATION.lock() {
        if current.as_ref().is_some_and(|o| o.generation == generation) {
            *current = None;
            true
        } else {
            false
        }
    } else {
        false
    };
    if !removed {
        return;
    }
    let _ = super::jni::android::with_android_env(|env, context| {
        let class = super::jni::android::load_context_class(
            env,
            context,
            "com.openless.app.OpenLessAccessibilityService",
        )?;
        env.call_static_method(
            class,
            "disarmVocabularyObservation",
            "(J)V",
            &[JValue::Long(generation as i64)],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    });
}

#[no_mangle]
pub extern "system" fn Java_com_openless_app_OpenLessNative_nativeCurrentVocabularyObservation(
    _: JNIEnv,
    _: JClass,
) -> jlong {
    let Ok(mut slot) = OBSERVATION.lock() else {
        return 0;
    };
    if slot.as_ref().is_some_and(|o| {
        o.started.elapsed() >= o.lifetime
            || (o.anchor.is_none() && o.started.elapsed() >= Duration::from_secs(1))
    }) {
        *slot = None;
    }
    slot.as_ref().map_or(0, |o| o.generation as jlong)
}

#[no_mangle]
pub extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopVocabularyObservation(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
) {
    disarm(generation as u64);
}

#[no_mangle]
pub extern "system" fn Java_com_openless_app_OpenLessNative_nativeVocabularyObservationActive(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
) -> jboolean {
    OBSERVATION.lock().ok().is_some_and(|slot| {
        slot.as_ref()
            .is_some_and(|o| o.generation == generation as u64 && o.started.elapsed() < o.lifetime)
    }) as jboolean
}

#[no_mangle]
pub extern "system" fn Java_com_openless_app_OpenLessNative_nativeVocabularyObservationRemainingMs(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
) -> jlong {
    OBSERVATION
        .lock()
        .ok()
        .and_then(|slot| {
            slot.as_ref()
                .filter(|o| o.generation == generation as u64)
                .map(|o| {
                    let elapsed = o.started.elapsed();
                    if o.anchor.is_none() && elapsed >= Duration::from_secs(1) {
                        0
                    } else {
                        o.lifetime.saturating_sub(elapsed).as_millis() as jlong
                    }
                })
        })
        .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_openless_app_OpenLessNative_nativeObserveVocabularyText(
    mut env: JNIEnv,
    _: JClass,
    generation: jlong,
    text: JString,
) -> jboolean {
    let Ok(text) = env.get_string(&text) else {
        return 0;
    };
    let text: String = text.into();
    let Ok(mut slot) = OBSERVATION.lock() else {
        return 0;
    };
    let Some(observation) = slot.as_mut().filter(|o| o.generation == generation as u64) else {
        return 0;
    };
    if observation.started.elapsed() >= observation.lifetime
        || (observation.anchor.is_none() && observation.started.elapsed() >= Duration::from_secs(1))
    {
        *slot = None;
        return 0;
    }
    let alive = if let Some(anchor) = &mut observation.anchor {
        anchor.observe(&text, &observation.callback)
    } else {
        observation.anchor = ObservedInsertion::new(text, &observation.text);
        observation.anchor.is_some() || observation.started.elapsed() < Duration::from_secs(1)
    };
    if !alive {
        *slot = None;
    }
    alive as jboolean
}

pub fn show_suggestions(suggestions: &[crate::types::PendingCorrection]) {
    let Ok(json) = serde_json::to_string(suggestions) else {
        return;
    };
    let _ = super::jni::android::with_android_env(|env, context| {
        let class = super::jni::android::load_context_class(
            env,
            context,
            "com.openless.app.OpenLessAccessibilityService",
        )?;
        let json = env.new_string(json).map_err(|e| e.to_string())?;
        env.call_static_method(
            class,
            "showVocabularySuggestions",
            "(Ljava/lang/String;)V",
            &[JValue::Object(json.as_ref())],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    });
}
