//! Revocable, process-local permission for request-only document data.
//! It never changes the recording's frozen style/language or grants page access.

use std::future::Future;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

tokio::task_local! {
    static DISPATCH_PERMISSION: DocumentPermit;
}

/// A last check at each actual transport send/retry. Outside a document-bound
/// request this is a no-op, including ordinary translation fallback.
pub(crate) fn check_dispatch() -> Result<(), crate::BackendError> {
    if DISPATCH_PERMISSION
        .try_with(DocumentPermit::is_valid)
        .unwrap_or(true)
    {
        Ok(())
    } else {
        Err(revoked())
    }
}

#[derive(Default)]
pub(crate) struct DocumentAuthority {
    generation: AtomicU64,
    changed: tokio::sync::Notify,
}

impl DocumentAuthority {
    pub(crate) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_waiters();
    }

    /// Called only while the PreferencesStore lock proves opt-in is enabled.
    pub(crate) fn permit(self: &Arc<Self>) -> DocumentPermit {
        DocumentPermit {
            authority: Arc::clone(self),
            generation: self.generation.load(Ordering::Acquire),
        }
    }
}

#[derive(Clone)]
pub(crate) struct DocumentPermit {
    authority: Arc<DocumentAuthority>,
    generation: u64,
}

impl std::fmt::Debug for DocumentPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DocumentPermit([local])")
    }
}
impl PartialEq for DocumentPermit {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.authority, &other.authority) && self.generation == other.generation
    }
}
impl Eq for DocumentPermit {}

impl DocumentPermit {
    pub(crate) fn is_valid(&self) -> bool {
        self.authority.generation.load(Ordering::Acquire) == self.generation
    }
}

/// Use on the actual provider future containing document data. Check before
/// every poll, so credentials/retries/transport awaits cannot revive a stale
/// prepared payload. Only this request is cancelled; document-free calls and
/// the plain-translation fallback have no such permit.
pub(crate) async fn dispatch<T>(
    permit: Option<DocumentPermit>,
    future: impl Future<Output = T>,
) -> Result<T, crate::BackendError> {
    let Some(permit) = permit else {
        return Ok(future.await);
    };
    let changed = permit.authority.changed.notified();
    tokio::pin!(changed);
    let _ = changed.as_mut().enable();
    tokio::pin!(future);
    let guarded = std::future::poll_fn(|cx| {
        if !permit.is_valid() {
            return std::task::Poll::Ready(Err(revoked()));
        }
        match future.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(value) => std::task::Poll::Ready(if permit.is_valid() {
                Ok(value)
            } else {
                Err(revoked())
            }),
        }
    });
    let guarded = DISPATCH_PERMISSION.scope(permit.clone(), guarded);
    tokio::select! {
        biased;
        _ = &mut changed => Err(revoked()),
        output = guarded => output,
    }
}

fn revoked() -> crate::BackendError {
    crate::BackendError::new(
        crate::BackendErrorCode::Cancelled,
        "document context permission expired",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn revocation_between_provider_polls_prevents_dispatch_after_regrant() {
        let authority = Arc::new(DocumentAuthority::default());
        let permit = authority.permit();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pending = tokio::spawn({
            let entered = entered.clone();
            let release = release.clone();
            let sent = sent.clone();
            async move {
                dispatch(Some(permit), async move {
                    entered.notify_one();
                    release.notified().await;
                    sent.fetch_add(1, Ordering::AcqRel);
                })
                .await
            }
        });
        entered.notified().await;
        authority.invalidate();
        assert!(
            authority.permit().is_valid(),
            "a fresh grant must not restore the old permit"
        );
        release.notify_one();
        assert_eq!(
            pending.await.unwrap().unwrap_err().code,
            crate::BackendErrorCode::Cancelled
        );
        assert_eq!(sent.load(Ordering::Acquire), 0);
        // No-document and plain translation requests do not use this grant.
        dispatch(None, async {
            sent.fetch_add(1, Ordering::AcqRel);
        })
        .await
        .unwrap();
        assert_eq!(sent.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn synchronous_preparation_revocation_is_rechecked_at_the_transport_boundary() {
        let authority = Arc::new(DocumentAuthority::default());
        let permit = authority.permit();
        let sent = std::sync::atomic::AtomicUsize::new(0);
        let result = dispatch(Some(permit), async {
            // A preparation future can be Ready without yielding to the outer
            // poll guard. The actual send site must still check the same token.
            authority.invalidate();
            if check_dispatch().is_ok() {
                sent.fetch_add(1, Ordering::AcqRel);
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(sent.load(Ordering::Acquire), 0);
        assert!(
            check_dispatch().is_ok(),
            "no ambient permission outside the request"
        );
    }
}
