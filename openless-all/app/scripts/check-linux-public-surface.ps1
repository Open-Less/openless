[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\")).Path
$surfaceFile = Join-Path $appRoot "linux-host/src/lib.rs"
$source = Get-Content -Raw -LiteralPath $surfaceFile
$linuxManifest = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-host/Cargo.toml")
$coreManifest = Get-Content -Raw -LiteralPath (Join-Path $appRoot "crates/openless-core/Cargo.toml")
$coreApi = Get-Content -Raw -LiteralPath (Join-Path $appRoot "crates/openless-core/src/api.rs")
# Linux 宿主已经拆成两半：`linux-host` 是纯后端库，`linux-web` 才是可执行入口
# （Qt 宿主通过 HTTP 命令桥驱动它）。窗口装配的检查跟着入口走。
$mainSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-web/src/main.rs")
$mainSource += (Get-ChildItem -LiteralPath (Join-Path $appRoot "linux-web/src") -Filter "*.rs" | Sort-Object Name | ForEach-Object { Get-Content -Raw -LiteralPath $_.FullName }) -join "`n"
$backendSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-host/src/backend.rs")
$qaSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-host/src/qa.rs")
$selectionSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-host/src/selection.rs")
$remoteSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-host/src/remote_input.rs")

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
    Write-Error "Linux host public surface exposes core implementation details: $($violations -join ', ')"
    exit 1
}

if ($linuxManifest -match 'legacy-preferences-write' -or $coreManifest -match 'legacy-preferences-write') {
    Write-Error "The legacy whole-document preferences feature must not exist in Core or Linux manifests"
    exit 1
}

# 新的宿主链路：单实例锁 + fcitx5 热键监听都在 linux-web 入口里装配。
if ($mainSource -notmatch 'claim_single_instance' -or
    $mainSource -notmatch 'spawn_headless') {
    Write-Error "Linux backend entrypoint must wire single-instance and fcitx5 hotkeys"
    exit 1
}

if ($backendSource -match 'qa_runtime:\s*None' -or
    $backendSource -notmatch 'RemoteInputService::new') {
    Write-Error "Linux production builder must inject QA and Remote Input runtimes"
    exit 1
}

# fcitx5 插件的安装/就绪在 `linux-host` 的 fcitx5 模块里：先装插件，再 prepare
# （prepare 内部先 require_ready，再重载插件）。
$fcitxSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-host/src/fcitx5.rs")
$installer = $fcitxSource.IndexOf('pub fn ensure_plugin_installed')
$prepared = $fcitxSource.IndexOf('pub fn prepare_fcitx5')
if ($installer -lt 0 -or $prepared -lt 0 -or $installer -gt $prepared) {
    Write-Error "Linux fcitx5 readiness must run before the hotkey listener"
    exit 1
}

foreach ($action in @('ShowQa', 'HideQa', 'ShowSelectionPreview', 'HideSelectionPreview')) {
    if ($mainSource -match ("HostAction::{0}\s*=>\s*\{{\s*\}}" -f $action)) {
        Write-Error "Linux host action '$action' must drive a visible host surface"
        exit 1
    }
}

# 事件不再由宿主的 reducer 重放：`linux-web` 把 Core 的事件流直接转成 SSE，
# 渲染层（Qt / 浏览器）自己消费。这里锁住这条通路的两端。
$eventsSource = Get-Content -Raw -LiteralPath (Join-Path $appRoot "linux-web/src/events.rs")
foreach ($bridgeToken in @('/api/events', '/api/invoke')) {
    if ($mainSource -notmatch [regex]::Escape($bridgeToken)) {
        Write-Error "Linux backend bridge is missing '$bridgeToken'"
        exit 1
    }
}
foreach ($eventName in @('dictation:state', 'qa:state', 'selection:state', 'cloud-sync-e2ee:restored')) {
    if ($eventsSource -notmatch [regex]::Escape($eventName)) {
        Write-Error "Linux event bridge does not map '$eventName'"
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
        Write-Error "Linux command surface is missing Core operation '$providerToken'"
        exit 1
    }
}
if ($mainSource -notmatch '(?s)provider\.validate\(' -or
    $mainSource -notmatch '(?s)provider\.list_models\(') {
    Write-Error "Linux command surface must validate and list models through ProviderApi"
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

Write-Output "Linux host public surface gate passed (facade/DTO/event/host-interface/fixture only)."
