//! Invocation-scoped webpage authorization shared by Capture and BrowserBind.
//! This is an in-memory grant, never a native target, IPC token or model input.

use openless_core::browser_scene::SceneAuthorization;
use std::sync::Arc;

struct InvocationGrant {
    grant: SceneAuthorization,
    live: Arc<dyn Fn() -> bool + Send + Sync>,
}

#[derive(Clone)]
pub(crate) struct CaptureAuthorization(Arc<InvocationGrant>);

impl CaptureAuthorization {
    pub(crate) fn new(
        grant: SceneAuthorization,
        live: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self(Arc::new(InvocationGrant { grant, live }))
    }

    pub(crate) fn grant(&self) -> SceneAuthorization {
        self.0.grant
    }

    pub(crate) fn is_current(&self) -> bool {
        self.0.grant.app_metadata && self.0.grant.browser_metadata && (self.0.live)()
    }

    /// Used at the production native freeze boundary. A queued old capture
    /// never starts freezing after revocation; an in-flight result is dropped.
    pub(crate) fn freeze<T>(
        &self,
        freeze: impl FnOnce(&dyn Fn() -> bool) -> Option<T>,
    ) -> Option<CapturedAuthorization<T>> {
        if !self.is_current() {
            return None;
        }
        let value = freeze(&|| self.is_current())?;
        self.is_current().then(|| CapturedAuthorization {
            authorization: self.clone(),
            value,
        })
    }
}

pub(crate) struct CapturedAuthorization<T> {
    authorization: CaptureAuthorization,
    value: T,
}

impl<T> CapturedAuthorization<T> {
    /// Recheck after the ordinary capture/awaiting work, before publishing the
    /// page capability. Failure does not invalidate ordinary selection capture.
    pub(crate) fn finish(self) -> Option<Self> {
        self.authorization.is_current().then_some(self)
    }

    /// Only this invocation's original grant can bind its captured page
    /// capability. A new live epoch or another invocation cannot repair it.
    pub(crate) fn bind<R>(
        &self,
        requested: &CaptureAuthorization,
        bind: impl FnOnce(&T, &dyn Fn() -> bool) -> Option<R>,
    ) -> Option<R> {
        let current = || {
            Arc::ptr_eq(&self.authorization.0, &requested.0)
                && self.authorization.is_current()
                && requested.is_current()
        };
        if !current() {
            return None;
        }
        let result = bind(&self.value, &current)?;
        current().then_some(result)
    }
}
