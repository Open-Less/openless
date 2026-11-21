//! Bounded iOS command admission. Prompts, provider snapshots, ASR history and
//! compose execution stay in Core; a help-write result is never an insertion.
use super::*;
use openless_core::auxiliary::PreparedVoiceCompose;
use openless_core::shared_types::{effective_pipeline_mode, translation_effective, PipelineMode};
use serde::Serialize;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VoiceIntent {
    kind: IntentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target_language: Option<String>,
    #[serde(
        default,
        rename = "stylePackID",
        skip_serializing_if = "Option::is_none"
    )]
    style_pack_id: Option<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum IntentKind {
    Dictation,
    Translation,
    HelpWrite,
}

#[derive(Default)]
pub(super) struct Execution {
    intent: Option<VoiceIntent>,
    ready: tokio::sync::Notify,
    started: Mutex<(bool, Option<SessionId>)>,
    compose: Mutex<Option<Arc<PreparedVoiceCompose>>>,
}
impl Execution {
    fn set_started(&self, session: Option<SessionId>) {
        *self.started.lock().unwrap() = (true, session);
        self.ready.notify_waiters();
    }
    async fn session(&self) -> Option<SessionId> {
        loop {
            let changed = self.ready.notified();
            let state = *self.started.lock().unwrap();
            if state.0 {
                return state.1;
            }
            changed.await;
        }
    }
    fn cancel_compose(&self) {
        if let Some(compose) = self.compose.lock().unwrap().as_ref() {
            compose.cancel();
        }
    }
}
impl Host {
    fn owns_execution(&self, client: Uuid, execution: &Arc<Execution>) -> bool {
        self.admission
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|active| {
                active.client == client
                    && !active.cancelled
                    && Arc::ptr_eq(&active.execution, execution)
            })
    }
    fn release_execution(&self, client: Uuid, execution: &Arc<Execution>) -> bool {
        let mut admission = self.admission.lock().unwrap();
        let matched = admission.as_ref().is_some_and(|active| {
            active.client == client && Arc::ptr_eq(&active.execution, execution)
        });
        let publish = matched && !admission.as_ref().unwrap().cancelled;
        if matched {
            *admission = None;
        }
        drop(admission);
        let _ = self.maintain_recordings_if_idle();
        publish
    }
    async fn intent_options(
        &self,
        intent: Option<&VoiceIntent>,
        insert: bool,
    ) -> Result<DictationStartOptions, BackendError> {
        let mut options = DictationStartOptions {
            insert_text: insert,
            ..Default::default()
        };
        let Some(intent) = intent else {
            return Ok(options);
        };
        let invalid = |message| error(BackendErrorCode::InvalidArgument, message);
        for field in [&intent.target_language, &intent.style_pack_id]
            .into_iter()
            .flatten()
        {
            if field.is_empty() || field.len() > 128 || field.chars().any(char::is_control) {
                return Err(invalid("invalid voice intent field"));
            }
        }
        if intent.kind != IntentKind::Translation && intent.target_language.is_some() {
            return Err(invalid("targetLanguage is only valid for translation"));
        }
        if intent.kind == IntentKind::HelpWrite && intent.style_pack_id.is_some() {
            return Err(invalid("helpWrite does not support stylePackID"));
        }
        if !self.developer_mode
            && (intent.kind != IntentKind::Dictation
                || intent
                    .style_pack_id
                    .as_deref()
                    .is_some_and(|style| style != "builtin.raw"))
        {
            return Err(error(
                BackendErrorCode::PermissionDenied,
                "ordinary mode supports local raw dictation only",
            ));
        }
        if let Some(style) = &intent.style_pack_id {
            if !self.backend.get_style_pack(style)?.enabled {
                return Err(invalid("voice style is disabled"));
            }
            options.style_pack_id = Some(style.clone());
        }
        let prefs = self.backend.get_preferences();
        let selected = options
            .style_pack_id
            .as_deref()
            .unwrap_or(&prefs.active_style_pack_id);
        if self.developer_mode
            && (intent.kind != IntentKind::Dictation || selected != "builtin.raw")
        {
            // Configured is only admission, never proof of model/network access.
            let capability = self.capability_snapshot().await?;
            let required = if intent.kind == IntentKind::Translation {
                "canAttemptTranslation"
            } else { "canAttemptPolish" };
            if capability["llm"][required] != true {
                return Err(error(
                    BackendErrorCode::InvalidState,
                    "LLM is not configured and enabled",
                ));
            }
        }
        if intent.kind == IntentKind::Translation {
            let target = intent
                .target_language
                .as_deref()
                .ok_or_else(|| invalid("translation requires targetLanguage"))?;
            if !prefs
                .translation_target_languages
                .iter()
                .any(|configured| configured == target)
                || !translation_effective(true, target, &prefs.working_languages)
            {
                return Err(invalid("translation target is not configured or effective"));
            }
            options.translation_requested = true;
            options.translation_target_language = Some(target.into());
        }
        if intent.kind == IntentKind::HelpWrite {
            if effective_pipeline_mode(prefs.multimodal_pipeline_enabled, prefs.pipeline_mode)
                != PipelineMode::Traditional
            {
                return Err(invalid("helpWrite requires traditional LLM"));
            }
            options.insert_text = false;
        }
        Ok(options)
    }
    pub(super) fn close_voice(&self) {
        if let Some(active) = self.admission.lock().unwrap().as_mut() {
            active.cancelled = true;
            active.execution.cancel_compose();
        }
    }

    pub(super) fn start_voice(
        self: &Arc<Self>,
        client: Uuid,
        insert: bool,
        intent: Option<VoiceIntent>,
    ) -> Result<(), BackendError> {
        let execution = Arc::new(Execution {
            intent,
            ..Default::default()
        });
        let mut admission = self.admission.lock().unwrap();
        if admission.is_some() || self.control_busy.load(Ordering::Acquire) {
            return Err(error(BackendErrorCode::Busy, "another recording is active"));
        }
        *admission = Some(Admission {
            client,
            finishing: false,
            cancelled: false,
            execution: execution.clone(),
        });
        drop(admission);
        let owner = self.clone();
        self.executor.spawn(async move {
            let start = async {
                let options = owner.intent_options(execution.intent.as_ref(), insert).await?;
                let help_write = execution.intent.as_ref().is_some_and(|intent| intent.kind == IntentKind::HelpWrite);
                if help_write {
                    let compose = owner.backend.services().auxiliary.prepare_voice_compose().await?;
                    *execution.compose.lock().unwrap() = Some(compose);
                }
                if !owner.owns_execution(client, &execution) { execution.cancel_compose(); return Err(cancelled()); }
                if help_write || !owner.developer_mode {
                    owner.backend.start_local_raw_dictation("ios-sensevoice", options.insert_text).await
                } else {
                    owner.backend.start_dictation_with_options(options).await
                }
            }.await;
            // Publish cleanup ownership before the started callback. A reentrant
            // cancel must not mistake an already-started Core session for a
            // starter that will still clean it up.
            if let Ok(session) = &start {
                execution.started.lock().unwrap().1 = Some(*session);
            }
            match start {
                Ok(session) if owner.owns_execution(client, &execution) => {
                    owner.broker.emit(json!({"kind":"started","clientSessionID":client,"sessionID":session.to_string()}));
                    execution.set_started(Some(session));
                }
                Ok(session) => {
                    execution.cancel_compose();
                    let _ = owner.backend.cancel_dictation(Some(session)).await;
                    execution.set_started(None);
                    owner.release_execution(client, &execution);
                }
                Err(err) => {
                    execution.set_started(None);
                    if owner.release_execution(client, &execution) { owner.failed(client, &err); }
                }
            }
        });
        Ok(())
    }
    pub(super) fn stop_voice(self: &Arc<Self>, client: Uuid) -> Result<(), BackendError> {
        let mut admission = self.admission.lock().unwrap();
        let active = admission
            .as_mut()
            .filter(|active| active.client == client)
            .ok_or_else(|| error(BackendErrorCode::InvalidState, "stale stop"))?;
        if active.finishing || active.cancelled {
            return Ok(());
        }
        active.finishing = true;
        let execution = active.execution.clone();
        drop(admission);
        let owner = self.clone();
        self.executor.spawn(async move {
            let result = async {
                let session = execution.session().await.ok_or_else(cancelled)?;
                if !owner.owns_execution(client, &execution) { return Err(cancelled()); }
                let result = owner.backend.stop_dictation_session(session).await?;
                if !owner.owns_execution(client, &execution) { return Err(cancelled()); }
                let compose = execution.compose.lock().unwrap().clone();
                if let Some(compose) = compose {
                    let preview = compose.compose(result.raw_text.clone()).await?;
                    Ok(json!({"kind":"completed","clientSessionID":client,"intent":execution.intent,
                        "outputDisposition":"preview","result":result,
                        "preview":{"sessionID":session.to_string(),"text":preview.text,
                            "providerType":preview.provider_type,"model":preview.model,"promptKind":preview.prompt_kind}}))
                } else {
                    let mut event = json!({"kind":"completed","clientSessionID":client,"result":result});
                    if let Some(intent) = &execution.intent { event["intent"] = json!(intent); }
                    Ok(event)
                }
            }.await;
            if owner.release_execution(client, &execution) {
                match result { Ok(event) => owner.broker.emit(event), Err(err) => owner.failed(client, &err) }
            }
        });
        Ok(())
    }
    pub(super) fn cancel_voice(self: &Arc<Self>, client: Uuid) -> Result<(), BackendError> {
        let mut admission = self.admission.lock().unwrap();
        let active = admission
            .as_mut()
            .filter(|active| active.client == client)
            .ok_or_else(|| error(BackendErrorCode::InvalidState, "stale cancel"))?;
        if active.cancelled {
            return Ok(());
        }
        active.cancelled = true;
        let execution = active.execution.clone();
        let finishing = active.finishing;
        let started = execution.started.lock().unwrap().1.is_some();
        let session = self.backend.snapshot().dictation.session_id;
        execution.cancel_compose();
        self.broker.abort_pending();
        drop(admission);
        let owner = self.clone();
        self.executor.spawn(async move {
            if let Some(session) = session {
                let _ = owner.backend.cancel_dictation(Some(session)).await;
            }
            owner
                .broker
                .emit(json!({"kind":"cancelled","clientSessionID":client}));
            // Starting/finishing owners release only after all their cleanup.
            if started && !finishing {
                owner.release_execution(client, &execution);
            }
        });
        Ok(())
    }
}

#[cfg(test)]
#[path = "voice_intent_tests.rs"]
mod tests;
