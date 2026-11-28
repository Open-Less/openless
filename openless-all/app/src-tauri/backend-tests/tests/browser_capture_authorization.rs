//! Production CaptureAuthorization transaction + actual DesktopContext grants,
//! binding and turn consumption. Native AX work is counted, not executed.
#![allow(dead_code)]

#[path = "../../src/ask_window/browser_capture.rs"]
mod browser_capture;
#[path = "../../src/ask_window/browser_context.rs"]
mod browser_context;
#[path = "../../src/ask_window/context.rs"]
mod context;

use browser_capture::{CaptureAuthorization, CapturedAuthorization};
use openless_core::{
    ask::{AskCancellation, AskTurnKey},
    ask_context::{ContextAuthorization, ContextProvider},
    assistant_api::{AssistantContextProvider, AssistantContextQuery, AssistantContextResult},
    browser_scene::{AuthorizedBrowserReference, BrowserInputKind, SceneAuthorization},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Revoked {
    Browser,
    App,
}

fn allow(provider: &context::DesktopContext) {
    provider.set_authorization(ContextAuthorization {
        app_metadata: true,
        usage: false,
    });
    provider.set_read_permissions(context::ReadPermissions { browser_selection: false,
        browser_tab: true,
        voice_history: false,
    });
}

fn revoke_regrant(provider: &context::DesktopContext, revoked: Revoked) {
    match revoked {
        Revoked::Browser => provider.set_read_permissions(context::ReadPermissions::default()),
        Revoked::App => provider.set_authorization(ContextAuthorization::default()),
    }
    allow(provider);
}

fn key() -> AskTurnKey {
    AskTurnKey {
        conversation_id: Uuid::new_v4(),
        turn_id: Uuid::new_v4(),
    }
}

#[derive(Default)]
struct Calls {
    freeze: AtomicUsize,
    bind: AtomicUsize,
    page: AtomicUsize,
    ordinary: AtomicUsize,
}

// Same three production admission gates as Request::Capture / CapturedScope /
// BrowserBind. The opaque value stands for a native target already owned by the
// existing worker; no test-constructed ControlledBrowser can bypass these gates.
fn capture(
    authorization: Option<&CaptureAuthorization>,
    calls: &Calls,
    during_freeze: impl FnOnce(),
) -> Option<CapturedAuthorization<()>> {
    let result = authorization.and_then(|authorization| {
        authorization.freeze(|current| {
            calls.freeze.fetch_add(1, Ordering::AcqRel);
            assert!(current());
            during_freeze();
            Some(()) // Even a native call that returns success cannot repair expiry.
        })
    });
    calls.ordinary.fetch_add(1, Ordering::AcqRel);
    result.and_then(CapturedAuthorization::finish)
}

struct CapturedPage(Arc<Calls>);

impl browser_context::BrowserReader for CapturedPage {
    fn read(
        &self,
        key: AskTurnKey,
        grant: SceneAuthorization,
        cancellation: AskCancellation,
    ) -> futures_util::future::BoxFuture<'_, AssistantContextResult> {
        Box::pin(async move {
            let mut reference = AuthorizedBrowserReference::new(
                key,
                grant,
                (),
                "Chrome",
                "https://example.test/synthetic",
                BrowserInputKind::WebAskAnything,
                None,
            )
            .unwrap();
            browser_context::consume(&mut reference, key, grant, |_| {
                if cancellation.is_cancelled() {
                    return false;
                }
                self.0.page.fetch_add(1, Ordering::AcqRel);
                true
            })
        })
    }
}

fn bind(
    provider: &context::DesktopContext,
    key: AskTurnKey,
    captured: Option<&CapturedAuthorization<()>>,
    requested: &CaptureAuthorization,
    calls: &Arc<Calls>,
) -> bool {
    let reader = captured.and_then(|captured| {
        captured.bind(requested, |_, current| {
            if !current() {
                return None;
            }
            calls.bind.fetch_add(1, Ordering::AcqRel);
            Some(Arc::new(CapturedPage(calls.clone())) as Arc<dyn browser_context::BrowserReader>)
        })
    });
    if let Some(reader) = reader {
        provider.bind_browser_reference(key.conversation_id, requested.grant(), reader);
        true
    } else {
        false
    }
}

async fn read(provider: &context::DesktopContext, key: AskTurnKey) -> AssistantContextResult {
    provider.begin_turn(key);
    provider
        .read(
            key,
            AssistantContextQuery::BrowserTab,
            AskCancellation::default(),
        )
        .await
        .unwrap()
}

async fn fresh_invocation_can_read(provider: &context::DesktopContext, calls: &Arc<Calls>) {
    let key = key();
    provider.open_conversation(key.conversation_id);
    let authorization = provider.capture_browser_authorization().unwrap();
    let captured = capture(Some(&authorization), calls, || {});
    assert!(bind(
        provider,
        key,
        captured.as_ref(),
        &authorization,
        calls
    ));
    assert!(matches!(
        read(provider, key).await,
        AssistantContextResult::Available { .. }
    ));
    assert_eq!(calls.page.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn queued_capture_revoke_regrant_cannot_start_freezing_but_new_invocation_can_read() {
    for revoked in [Revoked::Browser, Revoked::App] {
        let provider = context::DesktopContext::default();
        let key = key();
        provider.open_conversation(key.conversation_id);
        allow(&provider);
        // This is the exact original authorization that prepare_open submits.
        let authorization = provider.capture_browser_authorization().unwrap();
        let queued = authorization.clone();
        let calls = Arc::new(Calls::default());
        let (release, waiting) = tokio::sync::oneshot::channel();
        let worker = async {
            waiting.await.unwrap();
            capture(Some(&queued), &calls, || {})
        };
        let (captured, ()) = tokio::join!(worker, async {
            revoke_regrant(&provider, revoked);
            release.send(()).unwrap();
        });
        assert!(captured.is_none());
        assert_eq!(calls.freeze.load(Ordering::Acquire), 0);
        assert_eq!(calls.ordinary.load(Ordering::Acquire), 1);
        assert!(!bind(
            &provider,
            key,
            captured.as_ref(),
            &authorization,
            &calls
        ));
        assert!(matches!(
            read(&provider, key).await,
            AssistantContextResult::Unavailable { .. }
        ));
        assert_eq!(calls.page.load(Ordering::Acquire), 0);
        fresh_invocation_can_read(&provider, &calls).await;
    }
}

#[tokio::test]
async fn revoke_regrant_inside_freeze_discards_successful_native_result_and_preserves_ordinary_capture(
) {
    for revoked in [Revoked::Browser, Revoked::App] {
        let provider = context::DesktopContext::default();
        let key = key();
        provider.open_conversation(key.conversation_id);
        allow(&provider);
        let authorization = provider.capture_browser_authorization().unwrap();
        let calls = Arc::new(Calls::default());
        let captured = capture(Some(&authorization), &calls, || {
            revoke_regrant(&provider, revoked)
        });
        assert!(captured.is_none());
        assert_eq!(calls.freeze.load(Ordering::Acquire), 1);
        assert_eq!(calls.ordinary.load(Ordering::Acquire), 1);
        assert!(!bind(
            &provider,
            key,
            captured.as_ref(),
            &authorization,
            &calls
        ));
        assert!(matches!(
            read(&provider, key).await,
            AssistantContextResult::Unavailable { .. }
        ));
        assert_eq!(calls.page.load(Ordering::Acquire), 0);
        fresh_invocation_can_read(&provider, &calls).await;
    }
}

#[tokio::test]
async fn capture_waiting_for_ordinary_work_cannot_publish_a_regranted_page_capability() {
    for revoked in [Revoked::Browser, Revoked::App] {
        let provider = context::DesktopContext::default();
        allow(&provider);
        let authorization = provider.capture_browser_authorization().unwrap();
        let captured = authorization
            .freeze(|current| {
                assert!(current());
                Some(())
            })
            .unwrap();
        revoke_regrant(&provider, revoked);
        assert!(captured.finish().is_none());
        let calls = Arc::new(Calls::default());
        fresh_invocation_can_read(&provider, &calls).await;
    }
}

#[tokio::test]
async fn completed_capture_before_bind_cannot_be_repaired_with_a_new_epoch_or_a_new_invocation() {
    for revoked in [Revoked::Browser, Revoked::App] {
        let provider = context::DesktopContext::default();
        let key = key();
        provider.open_conversation(key.conversation_id);
        allow(&provider);
        let authorization = provider.capture_browser_authorization().unwrap();
        let calls = Arc::new(Calls::default());
        let captured = capture(Some(&authorization), &calls, || {}).unwrap();
        revoke_regrant(&provider, revoked);
        let regrant = provider.capture_browser_authorization().unwrap();
        assert_ne!(authorization.grant(), regrant.grant());
        assert!(!bind(
            &provider,
            key,
            Some(&captured),
            &authorization,
            &calls
        ));
        assert!(!bind(&provider, key, Some(&captured), &regrant, &calls));
        assert_eq!(calls.bind.load(Ordering::Acquire), 0);
        assert!(matches!(
            read(&provider, key).await,
            AssistantContextResult::Unavailable { .. }
        ));
        assert_eq!(calls.page.load(Ordering::Acquire), 0);
        fresh_invocation_can_read(&provider, &calls).await;
    }
}

#[tokio::test]
async fn unchanged_original_grant_flows_from_capture_through_bind_to_the_real_turn() {
    let provider = context::DesktopContext::default();
    allow(&provider);
    let calls = Arc::new(Calls::default());
    fresh_invocation_can_read(&provider, &calls).await;
    assert_eq!(calls.freeze.load(Ordering::Acquire), 1);
    assert_eq!(calls.bind.load(Ordering::Acquire), 1);
    assert_eq!(calls.ordinary.load(Ordering::Acquire), 1);
}

#[test]
fn a_second_invocation_with_the_same_epoch_cannot_borrow_the_first_capture() {
    let provider = context::DesktopContext::default();
    allow(&provider);
    let original = provider.capture_browser_authorization().unwrap();
    let other = provider.capture_browser_authorization().unwrap();
    assert_eq!(original.grant(), other.grant());
    let captured = original.freeze(|_| Some(())).unwrap().finish().unwrap();
    assert!(captured.bind(&other, |_, _| Some(())).is_none());
    assert!(captured.bind(&original, |_, _| Some(())).is_some());
}

#[test]
fn no_page_grant_leaves_ordinary_capture_available_without_any_native_freeze() {
    let provider = context::DesktopContext::default();
    let authorization = provider.capture_browser_authorization();
    assert!(authorization.is_none());
    let calls = Calls::default();
    assert!(capture(authorization.as_ref(), &calls, || panic!(
        "ungranted freeze"
    ))
    .is_none());
    assert_eq!(calls.freeze.load(Ordering::Acquire), 0);
    assert_eq!(calls.ordinary.load(Ordering::Acquire), 1);
}
