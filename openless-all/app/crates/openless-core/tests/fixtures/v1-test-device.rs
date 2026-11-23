use super::*;

pub(super) fn fixture_for_device(id: &str) -> Fixture {
    let mut fixture = Fixture::new();
    let mut device = fixture.store.device();
    device.id = id.into();
    fixture.scope.device_id = id.into();
    fixture.scope.owner_github_id = "11".into();
    // Reopen caches before binding this test device's activity provenance.
    fixture.repo = BackendRepositories::open(&fixture._temp.0).unwrap();
    fixture.store = CoreSyncStore::new(
        fixture.repo.clone(),
        Arc::new(fixture.credentials.clone()),
        fixture._temp.0.clone(),
        device,
        fixture.store.inner.gate.clone(),
        fixture.extensions.clone(),
        Arc::new(crate::TokioTaskSpawner),
    )
    .unwrap();
    fixture
        .store
        .bind_runtime_idle_probe(Arc::new(|| true))
        .unwrap();
    fixture
        .store
        .bind_runtime_effects(Arc::new(fixture.effects.clone()))
        .unwrap();
    fixture
}
