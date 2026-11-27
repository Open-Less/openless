//! Production Ask adapters without a webview, microphone, or external editor.
#![allow(dead_code)]

#[path = "../../src/ask_window/context.rs"]
mod context;
#[path = "../../src/ask_window/browser_context.rs"]
mod browser_context;
#[path = "../../src/browser_scene/observation.rs"]
mod observation;
#[path = "../../src/ask_window/voice_types.rs"]
mod voice_types;
#[path = "../../src/ask_window/wire.rs"]
mod wire;

use openless_core::ask::{AskCancellation, AskSessionFence, AskTurnKey};
use openless_core::ask_context::ContextAuthorization;
use openless_core::assistant_api::{
    AssistantContextProvider, AssistantContextQuery as Query, AssistantContextResult as Result,
};
use uuid::Uuid;
use openless_core::ask_context::ContextProvider;
use openless_core::browser_scene::{AuthorizedBrowserReference, BrowserInputKind, SceneAuthorization};
use std::sync::{Arc, atomic::{AtomicU64, AtomicUsize, Ordering}};

struct ControlledBrowser {
    revision: Arc<AtomicU64>,
    captured_revision: u64,
    reads: Arc<AtomicUsize>,
    wait: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
}

impl browser_context::BrowserReader for ControlledBrowser {
    fn read(
        &self, key: AskTurnKey, grant: SceneAuthorization,
        cancellation: AskCancellation,
    ) -> futures_util::future::BoxFuture<'_, Result> {
        Box::pin(async move {
            if let Some((ready, release)) = &self.wait {
                ready.notify_one();
                release.notified().await;
            }
            if cancellation.is_cancelled() {
                return Result::Unavailable { reason: "browserReferenceExpired".into() };
            }
            let mut reference = AuthorizedBrowserReference::new(
                key, grant, self.revision.clone(), "Chrome",
                "https://example.test/task?private=synthetic#reference",
                BrowserInputKind::WebAskAnything, None,
            ).unwrap();
            browser_context::consume(&mut reference, key, grant, |revision| {
                if revision.load(Ordering::Acquire) != self.captured_revision { return false; }
                self.reads.fetch_add(1, Ordering::AcqRel);
                !cancellation.is_cancelled()
            })
        })
    }
}

fn browser_permissions(provider: &context::DesktopContext) {
    provider.set_authorization(ContextAuthorization { app_metadata: true, usage: false });
    provider.set_read_permissions(context::ReadPermissions { browser_tab: true, voice_history: false });
}

fn browser_source(provider: &context::DesktopContext, key: AskTurnKey) -> (Arc<AtomicU64>, Arc<AtomicUsize>) {
    let revision = Arc::new(AtomicU64::new(8));
    let reads = Arc::new(AtomicUsize::new(0));
    provider.bind_browser_reference(key.conversation_id, provider.browser_authorization(), Arc::new(ControlledBrowser {
        revision: revision.clone(), captured_revision: 8, reads: reads.clone(), wait: None,
    }));
    (revision, reads)
}

#[tokio::test]
async fn browser_reference_uses_the_actual_turn_and_original_invocation_once() {
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    browser_permissions(&provider);
    let (_, reads) = browser_source(&provider, key);
    provider.begin_turn(key);
    let Result::Available { data, source, .. } = provider
        .read(key, Query::BrowserTab, AskCancellation::default()).await.unwrap()
    else { panic!("original authorized reference unavailable") };
    assert_eq!(source, "originalInputAccessibility");
    assert_eq!(data["reference"]["inputKind"], "webAskAnything");
    assert_eq!(data["reference"]["location"], "https://example.test/task");
    assert_eq!(data["referenceType"], "untrustedForegroundBrowserData");
    assert!(data["reference"].get("title").is_none());
    assert!(data["reference"].get("pid").is_none());
    assert_eq!(reads.load(Ordering::Acquire), 1);
    assert!(matches!(provider.read(key, Query::BrowserTab, AskCancellation::default()).await.unwrap(), Result::Unavailable { .. }));
}

#[tokio::test]
async fn same_window_tab_away_and_back_cannot_revive_a_same_looking_reference() {
        let provider = context::DesktopContext::default();
        let key = key();
        provider.open_conversation(key.conversation_id);
        browser_permissions(&provider);
        let (revision, reads) = browser_source(&provider, key);
        provider.begin_turn(key);
        observation::transition(&revision); // A -> B
        observation::transition(&revision); // B -> A, same current snapshot
        assert!(matches!(provider.read(key, Query::BrowserTab, AskCancellation::default()).await.unwrap(), Result::Unavailable { .. }));
        assert_eq!(reads.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn a_failed_native_tab_subscription_keeps_the_formal_context_port_unavailable_after_regrant() {
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    browser_permissions(&provider);
    let certificate = observation::establish(|subject, event| {
        !(subject == observation::Subject::TabGroup && event == "AXSelectedChildrenChanged")
    });
    // The same transaction gates PageWatch construction in production.
    if certificate.is_some() { browser_source(&provider, key); }
    provider.begin_turn(key);
    assert!(matches!(provider.read(key, Query::BrowserTab, AskCancellation::default()).await.unwrap(), Result::Unavailable { .. }));
    provider.set_read_permissions(context::ReadPermissions::default());
    browser_permissions(&provider);
    let next = AskTurnKey { turn_id: Uuid::new_v4(), ..key };
    provider.begin_turn(next);
    assert!(matches!(provider.read(next, Query::BrowserTab, AskCancellation::default()).await.unwrap(), Result::Unavailable { .. }));
}

#[tokio::test]
async fn ungranted_sources_and_later_turns_never_read_a_page() {
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    let (_, reads) = browser_source(&provider, key);
    provider.begin_turn(key);
    assert!(matches!(provider.read(key, Query::BrowserTab, AskCancellation::default()).await.unwrap(), Result::Denied));
    browser_permissions(&provider);
    assert!(matches!(provider.read(key, Query::BrowserTab, AskCancellation::default()).await.unwrap(), Result::Unavailable { .. }));
    assert_eq!(reads.load(Ordering::Acquire), 0);
    let (_, reads) = browser_source(&provider, key);
    provider.begin_turn(key);
    let next = AskTurnKey { turn_id: Uuid::new_v4(), ..key };
    provider.begin_turn(next);
    assert!(matches!(provider.read(next, Query::BrowserTab, AskCancellation::default()).await.unwrap(), Result::Unavailable { .. }));
    assert_eq!(reads.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn revoke_and_regrant_while_the_native_owner_is_queued_discards_before_reading() {
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    browser_permissions(&provider);
    let ready = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let reads = Arc::new(AtomicUsize::new(0));
    provider.bind_browser_reference(key.conversation_id, provider.browser_authorization(), Arc::new(ControlledBrowser {
        revision: Arc::new(AtomicU64::new(8)), captured_revision: 8, reads: reads.clone(),
        wait: Some((ready.clone(), release.clone())),
    }));
    provider.begin_turn(key);
    let (result, ()) = tokio::join!(
        provider.read(key, Query::BrowserTab, AskCancellation::default()),
        async {
            ready.notified().await;
            provider.set_read_permissions(context::ReadPermissions::default());
            browser_permissions(&provider);
            release.notify_one();
        }
    );
    assert!(matches!(result.unwrap(), Result::Denied));
    assert_eq!(reads.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn another_turn_while_the_owner_is_queued_cannot_receive_the_old_page() {
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    browser_permissions(&provider);
    let ready = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let reads = Arc::new(AtomicUsize::new(0));
    provider.bind_browser_reference(key.conversation_id, provider.browser_authorization(), Arc::new(ControlledBrowser {
        revision: Arc::new(AtomicU64::new(8)), captured_revision: 8, reads: reads.clone(),
        wait: Some((ready.clone(), release.clone())),
    }));
    provider.begin_turn(key);
    let (result, ()) = tokio::join!(
        provider.read(key, Query::BrowserTab, AskCancellation::default()),
        async {
            ready.notified().await;
            provider.begin_turn(AskTurnKey { turn_id: Uuid::new_v4(), ..key });
            release.notify_one();
        }
    );
    assert!(matches!(result.unwrap(), Result::Unavailable { .. }));
    assert_eq!(reads.load(Ordering::Acquire), 0);
}

fn key() -> AskTurnKey {
    AskTurnKey {
        conversation_id: Uuid::new_v4(),
        turn_id: Uuid::new_v4(),
    }
}

#[tokio::test]
async fn context_reads_use_the_external_invocation_and_respect_revocation() {
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    assert!(matches!(
        provider
            .read(key, Query::ForegroundApp, AskCancellation::default())
            .await
            .unwrap(),
        Result::Denied
    ));
    provider.set_authorization(ContextAuthorization {
        app_metadata: true,
        usage: false,
    });
    provider.record_external_app(
        key.conversation_id,
        Uuid::new_v4(),
        chrono::Utc::now(),
        Some("Editor A"),
        None,
    );
    for _ in 0..2 {
        let Result::Available { data, source, .. } = provider
            .read(key, Query::ForegroundApp, AskCancellation::default())
            .await
            .unwrap()
        else {
            panic!("authorized external app was not available");
        };
        assert_eq!(data["app"]["displayName"], "Editor A");
        assert_eq!(source, "externalInvocation");
        assert!(data.get("pid").is_none());
    }
    provider.set_authorization(ContextAuthorization::default());
    assert!(matches!(
        provider
            .read(key, Query::ForegroundApp, AskCancellation::default())
            .await
            .unwrap(),
        Result::Denied
    ));
    provider.set_authorization(ContextAuthorization {
        app_metadata: true,
        usage: false,
    });
    assert!(matches!(
        provider
            .read(key, Query::ForegroundApp, AskCancellation::default())
            .await
            .unwrap(),
        Result::Unavailable { .. }
    ));
}

#[tokio::test]
async fn context_queries_report_actual_capabilities_and_reject_expired_scope() {
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    for query in [Query::CurrentTime, Query::System] {
        assert!(matches!(
            provider
                .read(key, query, AskCancellation::default())
                .await
                .unwrap(),
            Result::Available { .. }
        ));
    }
    assert!(matches!(
        provider
            .read(key, Query::BrowserTab, AskCancellation::default())
            .await
            .unwrap(),
        Result::Denied
    ));
    assert!(matches!(
        provider
            .read(
                key,
                Query::VoiceHistory {
                    query: "recent".into(),
                    limit: 1000
                },
                AskCancellation::default()
            )
            .await
            .unwrap(),
        Result::Denied
    ));
    let fence = AskSessionFence::default();
    let cancelled = fence.issue();
    fence.revoke();
    assert_eq!(
        provider
            .read(key, Query::CurrentTime, cancelled)
            .await
            .unwrap_err()
            .code,
        openless_core::BackendErrorCode::Cancelled
    );
    provider.close_conversation(key.conversation_id);
    assert!(matches!(
        provider
            .read(key, Query::System, AskCancellation::default())
            .await
            .unwrap(),
        Result::Unavailable { .. }
    ));
}

#[tokio::test]
async fn browser_and_voice_grants_do_not_follow_app_or_chat_permissions() {
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    provider.set_authorization(ContextAuthorization {
        app_metadata: true,
        usage: false,
    });
    assert!(matches!(
        provider
            .read(key, Query::BrowserTab, AskCancellation::default())
            .await
            .unwrap(),
        Result::Denied
    ));
    provider.set_read_permissions(context::ReadPermissions {
        browser_tab: true,
        voice_history: false,
    });
    assert!(matches!(
        provider
            .read(key, Query::BrowserTab, AskCancellation::default())
            .await
            .unwrap(),
        Result::Unavailable { .. }
    ));
    assert!(matches!(
        provider
            .read(
                key,
                Query::VoiceHistory {
                    query: "".into(),
                    limit: 4
                },
                AskCancellation::default()
            )
            .await
            .unwrap(),
        Result::Denied
    ));
    provider.set_read_permissions(context::ReadPermissions::default());
    assert!(matches!(
        provider
            .read(key, Query::BrowserTab, AskCancellation::default())
            .await
            .unwrap(),
        Result::Denied
    ));
    let Result::Available { data, .. } = provider
        .read(key, Query::System, AskCancellation::default())
        .await
        .unwrap()
    else {
        panic!("system metadata unavailable");
    };
    assert!(!data["architecture"].as_str().unwrap().is_empty());
    #[cfg(target_os = "macos")]
    assert!(!data["version"].as_str().unwrap().is_empty());
}

#[test]
fn asynchronous_transcription_cannot_cross_composer_identity_or_text_revision() {
    let original = voice_types::Target {
        focus_epoch: 1,
        editor: voice_types::Editor {
            conversation_id: "conversation-a".into(),
            mount_id: "mount-a".into(),
            editor_id: "composer".into(),
            revision: 2,
            editable: true,
            selection_start: 3,
            selection_end: 3,
        },
    };
    assert!(original.matches(&original));
    for change in 0..8 {
        let mut changed = original.clone();
        match change {
            0 => changed.focus_epoch += 1,
            1 => changed.editor.conversation_id = "conversation-b".into(),
            2 => changed.editor.mount_id = "mount-b".into(),
            3 => changed.editor.editor_id = "other-input".into(),
            4 => changed.editor.revision += 1,
            5 => changed.editor.editable = false,
            6 => changed.editor.selection_start += 1,
            _ => changed.editor.selection_end += 1,
        }
        assert!(!original.matches(&changed));
    }
}
