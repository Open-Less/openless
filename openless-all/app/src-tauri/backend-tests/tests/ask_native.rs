//! Production Ask adapters without a webview, microphone, or external editor.
#![allow(dead_code)]

#[path = "../../src/ask_window/context.rs"]
mod context;
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

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires the controlled public Releases page in a supported browser and Accessibility permission"]
async fn reads_the_controlled_browser_tab_through_the_production_context_port() {
    let pid: u32 = std::env::var("OPENLESS_BROWSER_FIXTURE_PID")
        .expect("controlled browser process ID")
        .parse()
        .unwrap();
    let bundle =
        std::env::var("OPENLESS_BROWSER_FIXTURE_BUNDLE").expect("controlled browser bundle ID");
    let provider = context::DesktopContext::default();
    let key = key();
    provider.open_conversation(key.conversation_id);
    provider.set_read_permissions(context::ReadPermissions {
        browser_tab: true,
        voice_history: false,
    });
    provider.bind_browser_source(key.conversation_id, pid, &bundle);
    let result = provider
        .read(key, Query::BrowserTab, AskCancellation::default())
        .await
        .unwrap();
    let Result::Available { data, source, .. } = result else {
        panic!("controlled browser tab unavailable: {result:?}");
    };
    assert_eq!(
        data["url"],
        "https://github.com/Open-Less/openless/releases"
    );
    assert_eq!(source, "browserAccessibility");
    assert!(!data["title"].as_str().unwrap_or_default().is_empty());
    assert!(data.get("pid").is_none());
}
