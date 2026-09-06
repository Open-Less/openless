[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\")).Path
$surfaceFile = Join-Path $appRoot "linux-egui/src/lib.rs"
$source = Get-Content -Raw -LiteralPath $surfaceFile
$linuxManifest = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-egui/Cargo.toml")
$coreManifest = Get-Content -Raw -LiteralPath (Join-Path $appRoot "crates/openless-core/Cargo.toml")
$coreApi = Get-Content -Raw -LiteralPath (Join-Path $appRoot "crates/openless-core/src/api.rs")
$mainSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-egui/src/main.rs")
$backendSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-egui/src/backend.rs")
$qaSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-egui/src/qa.rs")
$selectionSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-egui/src/selection.rs")
$remoteSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-egui/src/remote_input.rs")

$forbidden = @(
    'pub use openless_core::domains::\*',
    'ActivityStore',
    'CorrectionRuleStore',
    'DictionaryStore',
    'HistoryStore',
    'StylePackStore',
    'HISTORY_CAP',
    'pub use openless_core::\{\s*activity',
    'style_pack_store',
    'selection_voice_intent'
)

$violations = @($forbidden | Where-Object { $source -match $_ })
if ($violations.Count -gt 0) {
    Write-Error "Linux egui public surface exposes core implementation details: $($violations -join ', ')"
    exit 1
}

if ($linuxManifest -match 'legacy-preferences-write' -or $coreManifest -match 'legacy-preferences-write') {
    Write-Error "The legacy whole-document preferences feature must not exist in Core or Linux manifests"
    exit 1
}

if ($mainSource -notmatch 'SingleInstanceBroker::acquire_or_forward' -or
    $mainSource -notmatch 'Fcitx5HotkeyListener::start' -or
    $mainSource -notmatch 'drain_native_events()') {
    Write-Error "Linux eframe production UI must wire single-instance and fcitx5 native events"
    exit 1
}
if ($mainSource -match 'LinuxNativeRuntime::start(backend,s*None,s*None)') {
    Write-Error "Linux eframe production UI must not disable all native adapters"
    exit 1
}

if ($backendSource -match 'qa_runtime:\s*None' -or
    $backendSource -notmatch 'RemoteInputService::new') {
    Write-Error "Linux production builder must inject QA and Remote Input runtimes"
    exit 1
}

$installer = $mainSource.IndexOf('ensure_fcitx5_ready(&config)?')
$listener = $mainSource.IndexOf('Fcitx5HotkeyListener::start')
if ($installer -lt 0 -or $listener -lt 0 -or $installer -gt $listener) {
    Write-Error "Linux AppImage fcitx5 installation must run before the hotkey listener"
    exit 1
}

foreach ($action in @('ShowQa', 'HideQa', 'ShowSelectionPreview', 'HideSelectionPreview')) {
    if ($mainSource -match ("HostAction::{0}\s*=>\s*\{{\s*\}}" -f $action)) {
        Write-Error "Linux host action '$action' must drive a visible egui surface"
        exit 1
    }
}

foreach ($replayToken in @(
    'replay_events_after',
    'replay.truncated',
    'LessComputerEventKind::User',
    'LessComputerEventKind::Started',
    'LessComputerEventKind::Tool',
    'LessComputerEventKind::Compaction',
    'LessComputerEventKind::Approval'
)) {
    if ($mainSource -notmatch [regex]::Escape($replayToken)) {
        Write-Error "Linux event reducer/replay is missing '$replayToken'"
        exit 1
    }
}

# Linux's first-run screen must be a real client of the Core credential and
# provider modules. These calls are intentionally checked together: a status-
# only screen or a local-model-only shortcut is not a usable setup path.
foreach ($providerToken in @(
    'provider_descriptors',
    'list_channels',
    'create_channel',
    'rename_channel',
    'set_channel_provider_type',
    'set_channel_enabled',
    'reorder_channels',
    'set_active_provider',
    'set_credential',
    'remove_credential'
)) {
    if ($mainSource -notmatch [regex]::Escape($providerToken)) {
        Write-Error "Linux provider setup UI is missing Core operation '$providerToken'"
        exit 1
    }
}
if ($mainSource -notmatch '(?s)\.provider\s*\.validate\(' -or
    $mainSource -notmatch '(?s)\.provider\s*\.list_models\(') {
    Write-Error "Linux provider setup UI must validate and list models through ProviderApi"
    exit 1
}
if ($mainSource -match 'ASR_PRESETS|LLM_PRESETS|OMNI_PRESETS|https://[^\s\"]+/v1') {
    Write-Error "Linux UI must not own provider endpoint/model/auth defaults"
    exit 1
}

if ($selectionSource -match 'preview cannot safely retain|replacement cannot be safely reverted') {
    Write-Error "Linux Selection PreviewConfirm/revert must not fall back to Unsupported"
    exit 1
}

# QA branching (answer versus edit/Selection Voice) belongs to QaService. The
# Linux adapter may execute the provider request and bind the opaque native
# selection ticket, but it must not rediscover product intent or disposition.
if ($qaSource -notmatch 'answer_qa_with_context' -or
    $qaSource -match 'SelectionVoiceDisposition|SelectionVoiceIntent|edit_instruction_mode|resolve_disposition') {
    Write-Error "Linux QA adapter must remain provider/recording/selection effects only"
    exit 1
}

# RemoteInputService owns connection/session/sequence/replay state. The Linux
# transport may decode the canonical frame and forward it, never keep a second
# sequence state machine beside Core.
if ($remoteSource -match 'RemoteStreamSequence|next_sequence|expected_sequence') {
    Write-Error "Linux Remote Input transport must not retain Core sequence policy"
    exit 1
}

foreach ($transportToken in @('RemoteFrameCodec::decode', '\.authenticate\(', 'TlsAcceptor')) {
    if ($remoteSource -notmatch $transportToken) {
        Write-Error "Linux Remote Input production transport is missing '$transportToken'"
        exit 1
    }
}

$legacyPreferenceWriters = @(
    'set_preferences',
    'set_preferences_validated',
    'set_preferences_preserving_style',
    'set_preferences_preserving_style_validated'
)
$legacyGate = '#\[cfg\(test\)\]\s*pub\(crate\) fn {0}\s*\('
foreach ($method in $legacyPreferenceWriters) {
    if ($coreApi -notmatch ($legacyGate -f $method)) {
        Write-Error "Legacy preferences writer '$method' must remain crate-private and test-only"
        exit 1
    }
    if ($coreApi -match ("pub\s+fn\s+{0}\s*\(" -f $method)) {
        Write-Error "Legacy preferences writer '$method' is exposed on the public Core facade"
        exit 1
    }
}

Write-Output "Linux egui public surface gate passed (facade/DTO/event/host-interface/fixture only)."
