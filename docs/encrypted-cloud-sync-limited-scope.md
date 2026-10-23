# Opt-in limited sync domain (SYNC-01/02/04/07/08)

Status: **local domain/store seam implemented; remote synchronization blocked by the current singleton-vault contract.** This is not iOS, cloud deployment, platform-vault, two-device, or runtime acceptance. No Host/Swift changes are included.

## Development checkpoint (2026-10-23)

This unfinished experiment was moved onto Beta `60f67a9b` for a Draft PR. The transfer includes only the limited-sync domain/store implementation, its tests, and this document. Latest Beta local-migration storage and document parsing APIs are retained. Translation-target changes already live in the iOS Core patch and are not copied from the old worktree.

Remote collection negotiation, authenticated transport, the iOS secure-vault adapter, confirmation metadata/UI, and real two-device acceptance remain unfinished. iOS continues to report `sync_scope_unavailable`; this branch does not enable remote synchronization. Keep the PR in Draft; do not merge this checkpoint.

### Validation after the Beta transfer

- `rustup run 1.99.0 cargo test --manifest-path openless-all/app/Cargo.toml -p openless-core --locked --offline --lib cloud_sync_e2ee --no-fail-fast`: compilation succeeded; **200 passed, 6 failed, 1201 filtered out**.
- All six failures occur in the limited-store tests at the shared `seeded()` fixture's full-store restore, with `RestoreRolledBack`, before the limited operation under test. Investigate compatibility with the current Beta restore/fixture contract; do not treat the original worktree's green result as validation of this branch.
- `python3 openless-all/ios/scripts/prepare-core.py`: passed; the existing isolated iOS Core patch still applies.
- `git diff --cached --check`: passed.
- No iOS device build, remote sync, real user-data restore, or two-device acceptance was performed for this checkpoint.

## Unchanged default

The desktop `EncryptedSyncConfig`, full service, consent `encrypted-full-snapshot-v1`, v1 wire DTOs/crypto, and full exporter retain their existing behavior. The only production change on the desktop construction path factors the same local-store composition into a shared helper; its root remains `encrypted-sync`. Full export still includes provider credentials, channels, private prompts, history and activity, as before. **Do not enable that service for the limited iOS requirement.**

`AllowedSyncScope::default()` is `FullSnapshot`. Constructing the limited facade without the explicit `ConfirmedVocabularyModeStyleReferences` permission fails; it never falls back to full export.

## Public contracts

Existing public modules export these seams (no root `lib.rs` edit is needed):

- `cloud_sync_e2ee::{LimitedSyncStore, LimitedSyncExport, SyncAccountProvider, VerifiedSyncAccount}`.
- `cloud_sync_e2ee_documents::{AllowedSyncScope, ConfirmedVocabularySelection, LimitedSyncDocuments, LimitedMergePreview, LIMITED_SYNC_NAMESPACE}`.
- The document module also re-exports its existing public argument types `Revision`, `SourceDevice`, `DocumentSet`, `LogicalDocument`, `Tombstone`, `DocumentKind`; callers no longer need inaccessible protocol-module paths.

`MarketplaceService::verified_sync_account().await -> Result<VerifiedSyncAccount, BackendError>` reuses the existing verified `/user` identity routine. `github_id() -> &str` returns the canonical positive numeric ID. Serialization is only `{"githubId":"42"}`. There is no public constructor/deserializer, login authority, token accessor, or full-sync enablement. Every call rechecks the stored login token; previously returned values are immutable observations, not cached authorization.

`SyncAccountProvider::verified_sync_account(&self) -> BoxFuture<'_, Result<VerifiedSyncAccount, BackendError>>` is implemented by `MarketplaceService`. Hosts normally inject the existing `Arc<MarketplaceService>`, or an authority adapter delegating to it. The limited facade verifies the current account before/after capture and before restore. Host account-switch/sign-out orchestration must serialize/cancel its own in-flight operations; this domain facade does not replace that lifecycle.

`LimitedSyncStore::build(config: EncryptedSyncConfig, data_dir: &Path, repositories: BackendRepositories, credentials: Arc<dyn CredentialStore>, tasks: Arc<dyn TaskSpawner>, allowed: AllowedSyncScope, accounts: Arc<dyn SyncAccountProvider>) -> Result<Self, BackendError>` composes **actual Core** local encryption, write gate, protected extension storage and journal backend without constructing the full service or starting network/automatic synchronization. It uses `encrypted-sync-limited-v1`, with its own durable device ID and hence distinct local key AAD, extension and journal identity. A Host selects one policy per repository instance: activity and credential stores bind the exact gate/device identity. Running both factories on the same already-bound repository is not an automatic migration path. Missing device IDs over existing protected ciphertext fail closed via the existing loader. Lost generation-state detection also checks the retained limited root; loss of the shared gate directory cannot silently reset the counter over limited protected state.

`LimitedSyncStore::new(CoreSyncStore, AllowedSyncScope, Arc<dyn SyncAccountProvider>) -> DocumentResult<Self>` is the custom composition seam. Its injected store must have a dedicated limited-policy local device identity, not the full service's identity. Prefer `build`.

Other facade methods:

- `generation() -> DocumentResult<Revision>`; `device() -> SourceDevice`.
- `bind_runtime_idle_probe(...)`, `bind_runtime_effects(...)`, `is_recovering()`, `recover_registered().await`: delegate to existing Core fences/recovery. Bind real runtime adapters before recovery/restore.
- `export(scope: SyncScope, confirmed: ConfirmedVocabularySelection).await -> DocumentResult<LimitedSyncExport>`.
- `restore(desired: LimitedSyncDocuments, context: RestoreContext, confirmed: ConfirmedVocabularySelection).await -> DocumentResult<LimitedSyncExport>`.
- `LimitedSyncExport { generation: Revision, documents: LimitedSyncDocuments }` contains no full snapshot accessor.

### Confirmation is explicit, not inferred

There is currently no universal confirmation bit in `DictionaryEntry`. In particular, the exact `LEARNED_VOCAB_NOTE` marker identifies auto-collected provenance, **not user consent**. `ConfirmedVocabularySelection::new(scope, generation, Vec<String>)` takes the Host's explicit confirmed IDs; an empty list means none, never all. It rejects duplicate/invalid IDs and binds the exact origin/account/vault/key/device plus generation. Export/restore reject stale scope/generation (`StalePreview`) and selected IDs missing from that coherent local image (`SourceChanged`). After a local deletion, the Host supplies the remaining live confirmed IDs and materializes deletion marks from the separately protected limited baseline.

**Required parent/Host work:** durable account/scope-bound confirmation metadata, confirmation UI, atomic/recoverable reconciliation of that metadata after import/deletion, and protected limited baseline persistence. Supplying IDs to this API alone is not completed confirmed-vocabulary synchronization. No ledger is invented in this component.

## Limited plaintext domain format

`LIMITED_SYNC_NAMESPACE = "confirmed-vocabulary-mode-style-references-v1"`.

`LimitedSyncDocuments::to_secret_json() -> DocumentResult<Zeroizing<Vec<u8>>>` emits a bounded **plaintext domain envelope for future authenticated encryption, never an HTTP request body**:

```json
{
  "manifest": {
    "schemaVersion": 1,
    "namespace": "confirmed-vocabulary-mode-style-references-v1",
    "serviceOrigin": "https://sync.example.test",
    "ownerGithubId": "42",
    "vaultId": "<uuid-v4>",
    "keyId": "<uuid-v4>"
  },
  "documents": {
    "schemaVersion": 1,
    "exportedAt": "<UTC RFC3339>",
    "sourceDevice": {"id":"...","os":"...","arch":"...","appVersion":"..."},
    "documents": [
      {"kind":"dictionary","id":"word-id","schemaVersion":1,
       "value":{"id":"word-id","phrase":"confirmed word","enabled":true,"createdAt":"...","sortIndex":0}},
      {"kind":"preferences","id":"defaultMode","schemaVersion":1,"value":"raw"},
      {"kind":"preferences","id":"enabledModes","schemaVersion":1,"value":["raw","light"]},
      {"kind":"preferences","id":"marketStyleReference","schemaVersion":1,
       "value":{"id":"builtin.light","version":"2.0.0","originPackId":null}}
    ],
    "tombstones": []
  }
}
```

`marketStyleReference` is null for explicit no-selection, otherwise installed ID/version/optional upstream marketplace `originPackId`. It never carries prompts, examples, icons, model/provider/channel settings, author login, or style contents. No translation-target/private preference expansion is implied. The mode enum is the existing Core enum; duplicate mode entries are rejected.

The dictionary whitelist is exactly ID, phrase, enabled, createdAt, sortIndex. Notes (including the learned marker), usage counts, and unconfirmed rows are excluded. New remote rows get `note = null`, `hits = 0`; matching local IDs retain their receiver-local note/count. Thus learned provenance is not transferred as implied confirmation; Host metadata integration remains required. Phrase equality across different stable IDs does not silently deduplicate or discard a user's row; existing stable-ID merge semantics remain authoritative.

`LimitedSyncDocuments::from_authenticated_json(bytes, &scope, observed_revision)` is **post-authenticated-decryption** input, not a decryption/authentication API. The expected scope and revision must come from the future authenticated transport, never the untrusted envelope. Strict duplicate-key/depth/size decoding and manifest equality precede validation. Wrong namespace/schema/origin/account/vault/key and unknown envelope fields fail; all other document kinds, dictionary extension fields, private preferences and preference tombstones fail. Full `DocumentSet` input is not accepted as a limited envelope, nor is the limited envelope accepted by the current full protocol decoder. No migration/downgrade is automatic.

## Merge, apply, and recovery semantics

- `local.diff(optional_baseline, remote)` delegates to the existing Core three-way logical merge and returns `LimitedMergePreview` with redacted conflicts. It rejects differing manifests (including account/key); key-rotation baseline migration is not silently guessed.
- `preview.conflicts()` and `preview.resolve(&[ConflictChoice])` preserve the existing explicit-choice rules: stable IDs, no timestamp winner, edit/edit and delete/edit conflicts, missing tombstones rejected. `record_local_deletions(&baseline, deleted_at)` delegates to the existing tombstone ledger, scoped only to previously confirmed limited IDs. Full-snapshot tombstones are never treated as confirmation evidence.
- Restore overlays the selected dictionary IDs and exactly `defaultMode`, `enabledModes`, `activeStylePackId` onto a coherent **local-only full before image**. Every unselected local row and all unrelated preferences/history/activity/styles/channels/credentials are retained. A live remote ID or delete mark colliding with an unselected local row fails before writes. A selected local row omitted without an explicit remote tombstone fails `MissingTombstone` rather than being silently deleted.
- Style selection requires an installed, enabled pack with matching ID/version/origin reference. Missing/mismatched/disabled packs fail `InvalidReference` before writes; the Host may offer the existing marketplace download/verification and then capture a fresh preview. No fallback pack, provider switch, or raw prompt import occurs. This is reference/version identity, not a content-hash guarantee for locally edited packages.
- The final full local image uses existing `CoreSyncStore::restore_scope`: preflight, exact generation, runtime fence, exclusive shared gate, encrypted before/after journal, real runtime effects, complete read-back verification, rollback and startup recovery. It does not perform a sequence of independent settings saves. All settings outside the three whitelisted keys are preserved; the local full image **never becomes a limited remote payload**.
- An intervening ordinary mutation fails the preview generation check (and the existing restore lease checks again after overlay). Once restoring, normal repository/credential mutation permits are blocked. Partial credential/effect failure rolls back; rollback failure retains recovery-required and encrypted material. Wrong recovery keys cannot replay or clear the pending journal.

## iOS secure-vault requirements (not implemented here)

Even a limited restore journals/reapplies a complete **local** coherent before/after image to preserve unrelated data. Therefore the actual iOS `CredentialStore` must implement the existing fail-closed hooks, not enable full remote sync or return empty credentials:

1. `bind_sync_gate`: retain the exact shared gate; reject rebinding to another gate. Every ordinary credential/channel mutation must hold and complete its registered mutation permit through actual asynchronous persistence, including uncertain results.
2. `export_sync_credentials_readonly`: one atomic vault/channel batch; missing/denied vault reads are errors. Core discards a mixed generation rather than exporting it.
3. `export_sync_credentials(&ExclusivePermit)` and `replace_sync_credentials(SyncCredentials, &ExclusivePermit)`: verify permit ownership, operate on the complete registered provider/channel batch, preserve unrelated OAuth/application entries, and surface partial/uncertain failure for the existing rollback/read-back path. Never return success from a no-op implementation.
4. `read_sync_secret`, `write_sync_secret`, `remove_sync_secret`: real app-private Keychain handling for the existing restricted `SyncSecretAccount` names. Local wrapping keys and remembered keys stay out of preference files, keyboard/shared containers, plaintext logs, backups-as-config, and remote payloads. Existing ciphertext with a missing key remains recovery-required; never generate a replacement over it.
5. Bind real runtime idle/effects adapters, recover before ordinary writes/recording, and keep account switch/sign-out serialized with in-flight Host operations. A platform-vault test and real device lifecycle verification remain separate acceptance gates.

## Why no remote sync method is exposed

Evidence in current client contract:

- `cloud_sync_e2ee_protocol/transport.rs` hardcodes metadata `/v1/me/vault`, snapshot `/v1/me/vault/snapshot`, delete `/v1/me/vault`, receipts `/v1/me/operations/{id}`. There is no named collection parameter/header.
- `types.rs` makes `Capabilities` and `SnapshotUpload` strict unknown-field-rejecting DTOs. Snapshot `payloadSchemaVersion` is fixed at 1; capability negotiation does not advertise namespaces. Opaque ciphertext is still stored under the one vault/CAS identity.
- Local mock test `v1_contract_has_a_singleton_vault_and_no_named_scope_capability` records the actual singleton request and rejects an unnegotiated `collections` extension before auth. Existing mock upload/crypto tests establish v1 CAS/receipt/opaque encrypted-frame client behavior, **not** an actual server's acceptance of this new envelope.

Therefore sending a limited encrypted payload to the current endpoint could replace the desktop full snapshot. A new consent string or encrypted manifest alone does not isolate that storage. This component deliberately has **no limited upload/sync-now/auto-sync path or success status**, and does not call the full service as a substitute. No real cloud was contacted and no server deployment is claimed.

### Exact server/protocol work required

A separately negotiated collection contract (e.g. a new v2 endpoint family) must specify all of:

- Explicit namespace/schema advertisement and request selection; never default an unknown namespace to full.
- Account + namespace isolated vault allocation, metadata, CAS revision/ETag, snapshot create/read/replace, password rotation, delete and retention; existing full routes and remote data unchanged.
- Namespace-bound operations/idempotency keys/receipts and retries; a receipt from a different collection must not reconcile a pending operation.
- Namespace/schema bound into authenticated crypto context and the encrypted manifest; reuse the actual Core crypto algorithms/framing/auth/transport/recovery machinery, with vectors and wrong-scope tests. No ad-hoc new crypto.
- Distinct local baseline/pending/key/device bindings. Standard limited local construction already separates its root/device identity; the full baseline must never be reused as a limited confirmation/deletion baseline. Define key-rotation and any user-approved migration explicitly.
- Local mock contracts proving independent full and limited snapshots, conflicts, delete/rotation/unknown outcomes and rejection of wrong scope/account/key. Then independent deployment and real two-device/platform acceptance.

The endpoint example is a required design shape, **not an invented claim that a server implements it**. Backend ownership/negotiation is still external to this component.

## Parent/root integration and desktop opt-in

- No shared root entry patch is needed for Core `lib.rs`: existing modules are public. Integrate only the changed E2EE files and new scope document; do not replace inherited dirty `credentials.rs`, `lib.rs`, desktop, Android or iOS files from this checkout.
- Optional documentation entry, parent-owned: append a link from `docs/encrypted-cloud-sync.md` to this file, explicitly labeled domain-only / remote blocked.
- Parent-owned T integration: review/register `translation_target_languages` for the existing full policy and exclude serde-skipped `translation_target_languages_present` from the persistent-field coverage scanner. Do not serialize the marker or add either field to this limited whitelist. T files are not copied or modified here; this checkout's scoped test result predates their integration.
- Do not set the existing backend `encrypted_sync` option to enable iOS limited sync. The account facade can be consumed independently now; limited local domain composition additionally requires the real vault/effects/confirmation seams above.
- A future desktop opt-in consumer must explicitly choose the limited policy/factory instead of the full factory for that repository lifetime, keep its limited confirmed ledger/baseline separate, and negotiate the new namespace. Existing desktop users remain on the full default without wire/local-store migration.

## Remaining domain limitations

The minimal adapter deliberately captures the existing complete coherent local image before projecting the whitelist. Thus excluded-store readability, existing full-capture size limits, real vault hooks and platform effects are still prerequisites; an oversized/unreadable history can block limited capture rather than being silently truncated or bypassed. Narrow read-only capture optimization is not included. This does not export the excluded stores.

The standard factories are alternatives, not simultaneous full/limited services on one already-bound repository. There is no automatic migration of an active full local store/journal, and no transport/key-rotation baseline migration. The custom `new` seam requires the caller to respect the dedicated-identity contract. Imported confirmation/provenance metadata and account lifecycle still require parent/Host integration. None of these gaps is represented as cloud success.

## Validation boundary

Tests use temporary repositories, memory/test vaults, real local Core encryption/journals and loopback protocol/account servers. They do not use user data, real cloud, microphone, simulator, permissions, or immutable dependency inputs. Test/build success is not product/runtime acceptance; the parent owns integration, independent review and final acceptance.

Historical validation in the source worktree, before the Beta transfer (not the result for this branch):

```sh
RUSTC="$(rustup which --toolchain 1.98.0 rustc)" \
RUSTDOC="$(rustup which --toolchain 1.98.0 rustdoc)" \
CARGO_TARGET_DIR="$PWD/target/scoped-sync" \
cargo test --manifest-path openless-all/app/Cargo.toml -p openless-core \
  --locked --offline --lib cloud_sync_e2ee --no-fail-fast
```

Result: **181 passed, 0 failed, 0 ignored; 997 unrelated tests filtered**. Includes 14 new source-named tests across limited document/store tests, numeric identity, singleton protocol mock and retained-limited-root lost-counter regression. Existing full default service/crypto/vector/transport/restore tests remain green. Separate `cargo test --doc` passed with **0 doctests** (not extra behavioral coverage). `git diff --check` and empty staged diff passed. Full unfiltered Core/Host/iOS and the combined T registration seam were not validated in this lane.
