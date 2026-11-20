use super::*;

pub(super) struct Platform;
#[derive(Clone)]
pub(super) struct Identity;

impl Platform {
    pub(super) fn new(_: Arc<AtomicU64>) -> Option<Self> {
        None
    }

    pub(super) fn poll(&mut self) {}

    pub(super) fn foreground(&self) -> Foreground<()> {
        Foreground::Unknown
    }

    pub(super) fn bind(&mut self, _: ()) -> Option<Identity> {
        None
    }

    pub(super) fn capture(&self, _: &Identity) -> Option<(ExternalSelection, Option<Baseline>)> {
        None
    }

    pub(super) fn write(
        &self,
        _: &Identity,
        _: &Baseline,
        _: &str,
        _: impl FnOnce() -> Result<(), WriteOutcome>,
    ) -> WriteOutcome {
        WriteOutcome::NotWritten
    }
}
