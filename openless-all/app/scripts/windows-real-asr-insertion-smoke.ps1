param(
  [string]$ExePath = "",
  [ValidateSet("notepad", "browser", "wt-cmd", "wt-powershell", "win32edit")]
  [string]$Target = "notepad",
  [ValidateSet("volcengine", "foundry-local-whisper")]
  [string]$AsrProvider = "volcengine",
  [string]$Phrase = "OpenLess Windows real regression",
  [int]$TimeoutSeconds = 120,
  [int]$VirtualKey = 0xA2,
  [string]$InjectedTranscriptText = "",
  [int]$ManualSpeechSeconds = 8,
  [int]$PostSuccessDelaySeconds = 0,
  [switch]$ManualSpeech,
  [switch]$AllowClipboardFallback,
  [switch]$RequireJsonCredentials,
  [switch]$DebugHotkeyEvents
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $ExePath = Join-Path $appRoot ".artifacts\windows-gnu\dev\openless.exe"
}

if (-not $env:SystemDrive) {
  $env:SystemDrive = "C:"
}
if (-not $env:ProgramData) {
  $env:ProgramData = Join-Path $env:SystemDrive "ProgramData"
}

if (-not (Test-Path $ExePath)) {
  throw "OpenLess executable not found: $ExePath"
}

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class OpenLessRegressionWin32 {
  [DllImport("user32.dll")]
  public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

  [DllImport("user32.dll")]
  public static extern bool SetForegroundWindow(IntPtr hWnd);

  [DllImport("user32.dll")]
  public static extern void keybd_event(byte bVk, byte bScan, int dwFlags, UIntPtr dwExtraInfo);

  public const int KEYEVENTF_EXTENDEDKEY = 0x0001;
  public const int KEYEVENTF_KEYUP = 0x0002;
}
"@

function Test-CredentialValue($Value) {
  return ($null -ne $Value) -and ($Value -is [string]) -and ($Value.Trim().Length -gt 0)
}

function Get-OpenLessCredentialStatus {
  $path = Join-Path $env:APPDATA "OpenLess\credentials.json"
  if (-not (Test-Path $path)) {
    return [pscustomobject]@{ Path = $path; Present = $false; VolcengineConfigured = $false; ArkConfigured = $false }
  }

  $json = Get-Content -Raw $path | ConvertFrom-Json
  $asr = $json.providers.asr.volcengine
  $llm = $json.providers.llm.ark
  [pscustomobject]@{
    Path = $path
    Present = $true
    VolcengineConfigured = (Test-CredentialValue $asr.appKey) -and (Test-CredentialValue $asr.accessKey)
    ArkConfigured = Test-CredentialValue $llm.apiKey
  }
}

function Read-TextUtf8($Path) {
  if (-not (Test-Path $Path)) {
    return $null
  }
  return Get-Content -Raw -Encoding UTF8 $Path
}

function Write-TextUtf8($Path, $Text) {
  $dir = Split-Path $Path -Parent
  if (-not (Test-Path $dir)) {
    New-Item -ItemType Directory -Path $dir | Out-Null
  }
  [System.IO.File]::WriteAllText($Path, $Text, [System.Text.UTF8Encoding]::new($false))
}

function Restore-ClipboardValue($Value) {
  if ($null -eq $Value -or ($Value -is [string] -and $Value.Length -eq 0)) {
    cmd /c "echo off | clip" | Out-Null
    return
  }
  Set-Clipboard -Value $Value
}

function Set-HoldHotkeyPreference($Path) {
  $previous = Read-TextUtf8 $Path
  if ([string]::IsNullOrWhiteSpace($previous)) {
    $prefs = [pscustomobject]@{}
  } else {
    $prefs = $previous | ConvertFrom-Json
  }
  if ($null -eq $prefs.hotkey) {
    $prefs | Add-Member -NotePropertyName hotkey -NotePropertyValue ([pscustomobject]@{})
  }
  if ($null -eq $prefs.hotkey.PSObject.Properties["trigger"]) {
    $prefs.hotkey | Add-Member -NotePropertyName trigger -NotePropertyValue "leftControl"
  } else {
    $prefs.hotkey.trigger = "leftControl"
  }
  if ($null -eq $prefs.hotkey.PSObject.Properties["mode"]) {
    $prefs.hotkey | Add-Member -NotePropertyName mode -NotePropertyValue "hold"
  } else {
    $prefs.hotkey.mode = "hold"
  }
  $dictationBinding = [pscustomobject]@{
    primary = "LeftControl"
    modifiers = @()
  }
  if ($null -eq $prefs.PSObject.Properties["dictationHotkey"]) {
    $prefs | Add-Member -NotePropertyName dictationHotkey -NotePropertyValue $dictationBinding
  } else {
    $prefs.dictationHotkey = $dictationBinding
  }
  if ($null -eq $prefs.defaultMode) { $prefs | Add-Member -NotePropertyName defaultMode -NotePropertyValue "light" }
  if ($null -eq $prefs.enabledModes) { $prefs | Add-Member -NotePropertyName enabledModes -NotePropertyValue @("light", "structured", "formal", "raw") }
  if ($null -eq $prefs.launchAtLogin) { $prefs | Add-Member -NotePropertyName launchAtLogin -NotePropertyValue $false }
  if ($null -eq $prefs.showCapsule) { $prefs | Add-Member -NotePropertyName showCapsule -NotePropertyValue $true }
  if ($null -eq $prefs.PSObject.Properties["activeAsrProvider"]) {
    $prefs | Add-Member -NotePropertyName activeAsrProvider -NotePropertyValue $AsrProvider
  } else {
    $prefs.activeAsrProvider = $AsrProvider
  }
  if ($null -eq $prefs.activeLlmProvider) { $prefs | Add-Member -NotePropertyName activeLlmProvider -NotePropertyValue "ark" }
  if ($null -eq $prefs.restoreClipboardAfterPaste) {
    $prefs | Add-Member -NotePropertyName restoreClipboardAfterPaste -NotePropertyValue $true
  } else {
    $prefs.restoreClipboardAfterPaste = $true
  }
  Write-TextUtf8 $Path ($prefs | ConvertTo-Json -Depth 8)
  return $previous
}

function Ensure-OpenLessCredentialNative {
  if ("OpenLessCredentialNative" -as [type]) {
    return
  }

  Add-Type @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class OpenLessCredentialNative {
  [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool CredRead(string target, UInt32 type, UInt32 reservedFlag, out IntPtr credentialPtr);

  [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool CredWrite(ref OpenLessCredentialNativeCredential credential, UInt32 flags);

  [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool CredDelete(string target, UInt32 type, UInt32 flags);

  [DllImport("advapi32.dll", SetLastError = true)]
  public static extern void CredFree(IntPtr buffer);
}

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct OpenLessCredentialNativeCredential {
  public UInt32 Flags;
  public UInt32 Type;
  public string TargetName;
  public string Comment;
  public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
  public UInt32 CredentialBlobSize;
  public IntPtr CredentialBlob;
  public UInt32 Persist;
  public UInt32 AttributeCount;
  public IntPtr Attributes;
  public string TargetAlias;
  public string UserName;
}
"@
}

function Get-OpenLessCredentialTarget($Account) {
  return "$Account.com.openless.app"
}

function Get-OpenLessKeyringPassword($Account) {
  Ensure-OpenLessCredentialNative
  $target = Get-OpenLessCredentialTarget $Account
  $ptr = [IntPtr]::Zero
  $ok = [OpenLessCredentialNative]::CredRead($target, 1, 0, [ref]$ptr)
  if (-not $ok) {
    $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    if ($errorCode -eq 1168) {
      return $null
    }
    throw (New-Object ComponentModel.Win32Exception($errorCode, "Read Windows Credential Manager entry $target failed"))
  }

  try {
    $credential = [Runtime.InteropServices.Marshal]::PtrToStructure($ptr, [type][OpenLessCredentialNativeCredential])
    if ($credential.CredentialBlobSize -eq 0) {
      return ""
    }
    $bytes = New-Object byte[] $credential.CredentialBlobSize
    [Runtime.InteropServices.Marshal]::Copy($credential.CredentialBlob, $bytes, 0, $bytes.Length)
    return [Text.Encoding]::Unicode.GetString($bytes)
  } finally {
    [OpenLessCredentialNative]::CredFree($ptr)
  }
}

function Set-OpenLessKeyringPassword($Account, $Password) {
  Ensure-OpenLessCredentialNative
  $target = Get-OpenLessCredentialTarget $Account
  $bytes = [Text.Encoding]::Unicode.GetBytes($Password)
  $blob = [IntPtr]::Zero
  if ($bytes.Length -gt 0) {
    $blob = [Runtime.InteropServices.Marshal]::AllocHGlobal($bytes.Length)
    [Runtime.InteropServices.Marshal]::Copy($bytes, 0, $blob, $bytes.Length)
  }

  try {
    $credential = [OpenLessCredentialNativeCredential]::new()
    $credential.Flags = 0
    $credential.Type = 1
    $credential.TargetName = $target
    $credential.Comment = "keyring v3.6.3"
    $credential.CredentialBlobSize = $bytes.Length
    $credential.CredentialBlob = $blob
    $credential.Persist = 3
    $credential.AttributeCount = 0
    $credential.Attributes = [IntPtr]::Zero
    $credential.TargetAlias = ""
    $credential.UserName = $Account
    $ok = [OpenLessCredentialNative]::CredWrite([ref]$credential, 0)
    if (-not $ok) {
      $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
      throw (New-Object ComponentModel.Win32Exception($errorCode, "Write Windows Credential Manager entry $target failed"))
    }
  } finally {
    if ($blob -ne [IntPtr]::Zero) {
      [Runtime.InteropServices.Marshal]::FreeHGlobal($blob)
    }
  }
}

function Remove-OpenLessKeyringPassword($Account) {
  Ensure-OpenLessCredentialNative
  $target = Get-OpenLessCredentialTarget $Account
  $ok = [OpenLessCredentialNative]::CredDelete($target, 1, 0)
  if (-not $ok) {
    $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    if ($errorCode -ne 1168) {
      throw (New-Object ComponentModel.Win32Exception($errorCode, "Delete Windows Credential Manager entry $target failed"))
    }
  }
}

function Split-OpenLessCredentialJson($Json) {
  $chunks = @()
  for ($start = 0; $start -lt $Json.Length; $start += 1000) {
    $len = [Math]::Min(1000, $Json.Length - $start)
    $chunks += $Json.Substring($start, $len)
  }
  if ($chunks.Count -eq 0) {
    $chunks += ""
  }
  return $chunks
}

function Get-OpenLessVaultCredentials {
  $manifestText = Get-OpenLessKeyringPassword "credentials.v1"
  if ([string]::IsNullOrWhiteSpace($manifestText)) {
    return $null
  }
  $manifest = $manifestText | ConvertFrom-Json
  if ($manifest.openless_credentials_storage -ne "chunked" -or $manifest.version -ne 1) {
    throw "Unsupported OpenLess credential vault manifest."
  }

  $json = ""
  for ($i = 0; $i -lt [int]$manifest.chunks; $i++) {
    $chunkAccount = if ($null -ne $manifest.PSObject.Properties["generation"] -and -not [string]::IsNullOrWhiteSpace($manifest.generation)) {
      "credentials.v1.chunk.$($manifest.generation).$i"
    } else {
      "credentials.v1.chunk.$i"
    }
    $chunk = Get-OpenLessKeyringPassword $chunkAccount
    if ($null -eq $chunk) {
      throw "Missing OpenLess credential vault chunk $i."
    }
    $json += $chunk
  }
  return $json
}

function Get-OpenLessVaultSnapshot {
  $manifestJson = Get-OpenLessKeyringPassword "credentials.v1"
  if ([string]::IsNullOrWhiteSpace($manifestJson)) {
    return [pscustomobject]@{
      HadVault = $false
      ManifestJson = $null
      ChunkAccounts = @()
      ChunkValues = @()
      VaultJson = $null
      JsonValid = $false
      Warning = $null
    }
  }

  try {
    $manifest = $manifestJson | ConvertFrom-Json
  } catch {
    return [pscustomobject]@{
      HadVault = $true
      ManifestJson = $manifestJson
      ChunkAccounts = @()
      ChunkValues = @()
      VaultJson = $null
      JsonValid = $false
      Warning = "credential vault manifest is invalid JSON: $($_.Exception.Message)"
    }
  }

  if ($manifest.openless_credentials_storage -ne "chunked" -or $manifest.version -ne 1) {
    return [pscustomobject]@{
      HadVault = $true
      ManifestJson = $manifestJson
      ChunkAccounts = @()
      ChunkValues = @()
      VaultJson = $null
      JsonValid = $false
      Warning = "unsupported credential vault manifest"
    }
  }

  $chunkAccounts = @()
  $chunkValues = @()
  $chunksMissing = $false
  for ($i = 0; $i -lt [int]$manifest.chunks; $i++) {
    $account = if ($null -ne $manifest.PSObject.Properties["generation"] -and -not [string]::IsNullOrWhiteSpace($manifest.generation)) {
      "credentials.v1.chunk.$($manifest.generation).$i"
    } else {
      "credentials.v1.chunk.$i"
    }
    $chunkValue = Get-OpenLessKeyringPassword $account
    $chunkAccounts += $account
    $chunkValues += $chunkValue
    if ($null -eq $chunkValue) {
      $chunksMissing = $true
    }
  }

  if ($chunksMissing) {
    return [pscustomobject]@{
      HadVault = $true
      ManifestJson = $manifestJson
      ChunkAccounts = $chunkAccounts
      ChunkValues = $chunkValues
      VaultJson = $null
      JsonValid = $false
      Warning = "credential vault chunk is missing"
    }
  }

  $vaultJson = ($chunkValues -join "")
  try {
    $null = $vaultJson | ConvertFrom-Json
    $jsonValid = $true
    $warning = $null
  } catch {
    $jsonValid = $false
    $warning = "credential vault JSON is invalid: $($_.Exception.Message)"
  }

  return [pscustomobject]@{
    HadVault = $true
    ManifestJson = $manifestJson
    ChunkAccounts = $chunkAccounts
    ChunkValues = $chunkValues
    VaultJson = $vaultJson
    JsonValid = $jsonValid
    Warning = $warning
  }
}

function Set-OpenLessVaultCredentials($Json, $PreviousManifestJson) {
  $previousManifest = $null
  if (-not [string]::IsNullOrWhiteSpace($PreviousManifestJson)) {
    $previousManifest = $PreviousManifestJson | ConvertFrom-Json
  }

  $chunks = Split-OpenLessCredentialJson $Json
  for ($i = 0; $i -lt $chunks.Count; $i++) {
    Set-OpenLessKeyringPassword "credentials.v1.chunk.$i" $chunks[$i]
  }

  $manifest = [pscustomobject]@{
    openless_credentials_storage = "chunked"
    version = 1
    chunks = $chunks.Count
  }
  Set-OpenLessKeyringPassword "credentials.v1" ($manifest | ConvertTo-Json -Compress)

  if ($null -ne $previousManifest -and $null -ne $previousManifest.PSObject.Properties["chunks"]) {
    if ($null -ne $previousManifest.PSObject.Properties["generation"] -and -not [string]::IsNullOrWhiteSpace($previousManifest.generation)) {
      for ($i = 0; $i -lt [int]$previousManifest.chunks; $i++) {
        Remove-OpenLessKeyringPassword "credentials.v1.chunk.$($previousManifest.generation).$i"
      }
    } else {
      for ($i = $chunks.Count; $i -lt [int]$previousManifest.chunks; $i++) {
        Remove-OpenLessKeyringPassword "credentials.v1.chunk.$i"
      }
    }
  }
}

function Restore-ActiveAsrCredential($Snapshot, $Path) {
  if ($null -eq $Snapshot) {
    return
  }
  if ($Snapshot.HadVault) {
    for ($i = 0; $i -lt $Snapshot.VaultChunkAccounts.Count; $i++) {
      Set-OpenLessKeyringPassword $Snapshot.VaultChunkAccounts[$i] $Snapshot.VaultChunkValues[$i]
    }
    Set-OpenLessKeyringPassword "credentials.v1" $Snapshot.VaultManifestJson
    for ($i = 0; $i -lt $Snapshot.WrittenVaultChunks; $i++) {
      $generatedAccount = "credentials.v1.chunk.$i"
      if ($Snapshot.VaultChunkAccounts -notcontains $generatedAccount) {
        Remove-OpenLessKeyringPassword $generatedAccount
      }
    }
  } else {
    Remove-OpenLessKeyringPassword "credentials.v1"
    for ($i = 0; $i -lt $Snapshot.WrittenVaultChunks; $i++) {
      Remove-OpenLessKeyringPassword "credentials.v1.chunk.$i"
    }
  }

  if ($null -eq $Snapshot.LegacyJson) {
    Remove-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
  } else {
    Write-TextUtf8 $Path $Snapshot.LegacyJson
  }
}

function Set-ActiveAsrCredential($Path) {
  $previousLegacy = Read-TextUtf8 $Path
  $vaultSnapshot = Get-OpenLessVaultSnapshot
  if (-not [string]::IsNullOrWhiteSpace($vaultSnapshot.Warning)) {
    Write-Warning "OpenLess credential vault is unreadable for smoke bootstrap; falling back to legacy/blank credentials. $($vaultSnapshot.Warning)"
  }
  $source = if ($vaultSnapshot.JsonValid -and -not [string]::IsNullOrWhiteSpace($vaultSnapshot.VaultJson)) {
    $vaultSnapshot.VaultJson
  } else {
    $previousLegacy
  }
  if ([string]::IsNullOrWhiteSpace($source)) {
    $credentials = [pscustomobject]@{
      version = 1
      active = [pscustomobject]@{
        asr = $AsrProvider
        llm = "ark"
      }
      providers = [pscustomobject]@{
        asr = [pscustomobject]@{}
        llm = [pscustomobject]@{}
      }
    }
  } else {
    $credentials = $source | ConvertFrom-Json
    if ($null -eq $credentials.PSObject.Properties["active"]) {
      $credentials | Add-Member -NotePropertyName active -NotePropertyValue ([pscustomobject]@{})
    } elseif ($null -eq $credentials.active) {
      $credentials.active = [pscustomobject]@{}
    }
    if ($null -eq $credentials.active.PSObject.Properties["asr"]) {
      $credentials.active | Add-Member -NotePropertyName asr -NotePropertyValue $AsrProvider
    } else {
      $credentials.active.asr = $AsrProvider
    }
    if ($null -eq $credentials.active.PSObject.Properties["llm"]) {
      $credentials.active | Add-Member -NotePropertyName llm -NotePropertyValue "ark"
    }
    if ($null -eq $credentials.PSObject.Properties["providers"]) {
      $credentials | Add-Member -NotePropertyName providers -NotePropertyValue ([pscustomobject]@{})
    } elseif ($null -eq $credentials.providers) {
      $credentials.providers = [pscustomobject]@{}
    }
    if ($null -eq $credentials.providers.PSObject.Properties["asr"]) {
      $credentials.providers | Add-Member -NotePropertyName asr -NotePropertyValue ([pscustomobject]@{})
    } elseif ($null -eq $credentials.providers.asr) {
      $credentials.providers.asr = [pscustomobject]@{}
    }
    if ($null -eq $credentials.providers.PSObject.Properties["llm"]) {
      $credentials.providers | Add-Member -NotePropertyName llm -NotePropertyValue ([pscustomobject]@{})
    } elseif ($null -eq $credentials.providers.llm) {
      $credentials.providers.llm = [pscustomobject]@{}
    }
  }
  $json = $credentials | ConvertTo-Json -Depth 12 -Compress
  $chunks = Split-OpenLessCredentialJson $json
  Set-OpenLessVaultCredentials $json $vaultSnapshot.ManifestJson
  if (-not $vaultSnapshot.JsonValid) {
    Write-TextUtf8 $Path ($credentials | ConvertTo-Json -Depth 12)
  }
  return [pscustomobject]@{
    LegacyJson = $previousLegacy
    VaultManifestJson = $vaultSnapshot.ManifestJson
    VaultChunkAccounts = @($vaultSnapshot.ChunkAccounts)
    VaultChunkValues = @($vaultSnapshot.ChunkValues)
    HadVault = [bool]$vaultSnapshot.HadVault
    WrittenVaultChunks = $chunks.Count
  }
}

function Wait-LogPattern($Path, $Pattern, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if (Test-Path $Path) {
      $text = Get-Content -Raw $Path
      if ($text -match $Pattern) {
        return $true
      }
    }
    Start-Sleep -Milliseconds 300
  }
  return $false
}

function Get-HistoryCount($Path) {
  if (-not (Test-Path $Path)) {
    return 0
  }
  $json = Get-Content -Raw -Encoding UTF8 $Path | ConvertFrom-Json
  if ($null -eq $json) {
    return 0
  }
  return @($json).Count
}

function Get-LatestHistory($Path) {
  if (-not (Test-Path $Path)) {
    return $null
  }
  $json = Get-Content -Raw -Encoding UTF8 $Path | ConvertFrom-Json
  return @($json) | Select-Object -First 1
}

function Wait-HistoryAdvance($Path, $BaselineCount, $BaselineLatestId, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    $count = Get-HistoryCount $Path
    if ($count -gt $BaselineCount) {
      return $true
    }
    $latest = Get-LatestHistory $Path
    if ($null -ne $latest -and -not [string]::IsNullOrWhiteSpace($latest.id) -and $latest.id -ne $BaselineLatestId) {
      return $true
    }
    Start-Sleep -Milliseconds 500
  }
  return $false
}

function Send-KeyEdge($Vk, $KeyUp, $Extended = $true) {
  $flags = 0
  if ($Extended) {
    $flags = $flags -bor [OpenLessRegressionWin32]::KEYEVENTF_EXTENDEDKEY
  }
  if ($KeyUp) {
    $flags = $flags -bor [OpenLessRegressionWin32]::KEYEVENTF_KEYUP
  }
  $scanCode = if ($Vk -eq 0xA3 -or $Vk -eq 0xA2) { 0x1D } else { 0 }
  [OpenLessRegressionWin32]::keybd_event([byte]$Vk, [byte]$scanCode, $flags, [UIntPtr]::Zero)
}

function Test-ExtendedVirtualKey($Vk) {
  return $Vk -in @(0xA3, 0xA5, 0x5C)
}

function Tap-Hotkey {
  $extended = Test-ExtendedVirtualKey $VirtualKey
  Send-KeyEdge $VirtualKey $false $extended
  Start-Sleep -Milliseconds 180
  Send-KeyEdge $VirtualKey $true $extended
}

function Press-Hotkey {
  Send-KeyEdge $VirtualKey $false (Test-ExtendedVirtualKey $VirtualKey)
}

function Release-Hotkey {
  Send-KeyEdge $VirtualKey $true (Test-ExtendedVirtualKey $VirtualKey)
}

function Ensure-TargetFocused($TargetInfo) {
  if ($null -eq $TargetInfo) {
    return $false
  }
  if ($TargetInfo.TargetKind -eq "browser" -and $null -ne $TargetInfo.Process) {
    if (-not (Focus-Window $TargetInfo.Process)) {
      return $false
    }
    return (Focus-BrowserTextarea $TargetInfo.Process)
  }
  if ($TargetInfo.TargetTitle) {
    $wshell = New-Object -ComObject WScript.Shell
    if ($wshell.AppActivate($TargetInfo.TargetTitle)) {
      Start-Sleep -Milliseconds 500
      return $true
    }
  }
  if ($null -ne $TargetInfo.Process) {
    return (Focus-Window $TargetInfo.Process)
  }
  return $false
}

function Focus-Window($Process) {
  if ($null -eq $Process) {
    return $false
  }
  $handle = 0
  if ($null -ne $Process.PSObject.Properties["MainWindowHandleOverride"]) {
    $handle = [int64]$Process.MainWindowHandleOverride
  } else {
    $handle = [int64]$Process.MainWindowHandle
  }
  if ($handle -eq 0) {
    return $false
  }
  [OpenLessRegressionWin32]::ShowWindow([IntPtr]$handle, 9) | Out-Null
  [OpenLessRegressionWin32]::SetForegroundWindow([IntPtr]$handle) | Out-Null
  Start-Sleep -Milliseconds 500
  return $true
}

function Focus-BrowserTextarea($Process) {
  if ($null -eq $Process) {
    return $false
  }
  $focusScript = @"
import sys
import time
from pywinauto import Application

pid = int(sys.argv[1])
app = Application(backend='uia').connect(process=pid)
win = app.top_window()
win.set_focus()
time.sleep(0.2)

candidate = None
for descendant in win.descendants():
    try:
        if descendant.element_info.control_type != 'Edit':
            continue
        if descendant.class_name() == 'OmniboxViewViews':
            continue
        rect = descendant.rectangle()
        if rect.width() <= 0 or rect.height() <= 0:
            continue
        candidate = descendant
        if descendant.class_name() == '':
            break
    except Exception:
        continue

if candidate is None:
    raise SystemExit(1)

try:
    candidate.set_focus()
except Exception:
    pass
time.sleep(0.1)
candidate.click_input()
time.sleep(0.2)
raise SystemExit(0)
"@
  $focusPath = Join-Path $env:TEMP "openless-browser-focus.py"
  Write-TextUtf8 $focusPath $focusScript
  try {
    python -X utf8 $focusPath $Process.Id | Out-Null
    return $true
  } catch {
    return $false
  } finally {
    Remove-Item -LiteralPath $focusPath -Force -ErrorAction SilentlyContinue
  }
}

function Wait-ProcessWindow($ProcessName, $After, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    $candidates = Get-Process $ProcessName -ErrorAction SilentlyContinue |
      Where-Object { $_.StartTime -ge $After -and $_.MainWindowHandle -ne 0 } |
      Sort-Object StartTime -Descending
    $windowProcess = @($candidates) | Select-Object -First 1
    if ($null -ne $windowProcess) {
      return $windowProcess
    }
    Start-Sleep -Milliseconds 300
  }
  return $null
}

function Wait-BrowserWindow($TitleFragment, $TimeoutSeconds) {
  $probeScript = @"
import json
import sys
from pywinauto import Desktop

title_fragment = sys.argv[1]
for win in Desktop(backend='uia').windows():
    try:
        title = win.window_text()
        if not title or title_fragment not in title:
            continue
        payload = {
            "title": title,
            "pid": win.process_id(),
            "handle": int(win.handle),
        }
        print(json.dumps(payload, ensure_ascii=False))
        raise SystemExit(0)
    except Exception:
        continue
raise SystemExit(1)
"@
  $probePath = Join-Path $env:TEMP "openless-browser-window-probe.py"
  Write-TextUtf8 $probePath $probeScript
  try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
      try {
        $json = python -X utf8 $probePath $TitleFragment
        if (-not [string]::IsNullOrWhiteSpace($json)) {
          return ($json | ConvertFrom-Json)
        }
      } catch {
      }
      Start-Sleep -Milliseconds 300
    }
    return $null
  } finally {
    Remove-Item -LiteralPath $probePath -Force -ErrorAction SilentlyContinue
  }
}

function Resolve-BrowserPath {
  $programFiles = if ($env:ProgramFiles) { $env:ProgramFiles } else { Join-Path $env:SystemDrive "Program Files" }
  $programFilesX86 = if (${env:ProgramFiles(x86)}) { ${env:ProgramFiles(x86)} } else { Join-Path $env:SystemDrive "Program Files (x86)" }
  $roots = @(
    $programFilesX86,
    $programFiles,
    (Join-Path $env:LOCALAPPDATA "Microsoft\Edge\Application"),
    (Join-Path $env:LOCALAPPDATA "Google\Chrome\Application"),
    (Join-Path $env:LOCALAPPDATA "BraveSoftware\Brave-Browser\Application")
  ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
  $candidates = @()
  foreach ($root in $roots) {
    $candidates += Join-Path $root "Microsoft\Edge\Application\msedge.exe"
    $candidates += Join-Path $root "Google\Chrome\Application\chrome.exe"
    $candidates += Join-Path $root "BraveSoftware\Brave-Browser\Application\brave.exe"
    $candidates += Join-Path $root "msedge.exe"
    $candidates += Join-Path $root "chrome.exe"
    $candidates += Join-Path $root "brave.exe"
  }
  foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path $candidate)) {
      return $candidate
    }
  }
  throw "Neither Microsoft Edge nor Google Chrome was found."
}

function New-BrowserInputFixture {
  $path = Join-Path $env:TEMP "openless-browser-input-fixture.html"
  $html = @"
<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>OpenLess Browser Input Fixture</title>
  <style>
    body { font: 16px system-ui, sans-serif; margin: 32px; }
    textarea { width: 720px; height: 220px; font: 18px Consolas, monospace; }
  </style>
</head>
<body>
  <textarea id="target" autofocus></textarea>
  <script>
    const target = document.getElementById('target');
    target.focus();
    target.select();
    window.addEventListener('focus', () => target.focus());
    document.body.addEventListener('click', () => target.focus());
  </script>
</body>
</html>
"@
  Write-TextUtf8 $path $html
  return $path
}

function New-Win32EditHost {
  $sourcePath = Join-Path $env:TEMP "OpenLessWin32EditHost.cs"
  $exePath = Join-Path $env:TEMP "OpenLessWin32EditHost.exe"
  $source = @"
using System;
using System.Windows.Forms;

public static class OpenLessWin32EditHost {
  [STAThread]
  public static void Main() {
    Application.EnableVisualStyles();
    Application.SetCompatibleTextRenderingDefault(false);
    var form = new Form();
    form.Text = "OpenLess Win32 Edit Host";
    form.Width = 820;
    form.Height = 320;
    var box = new TextBox();
    box.Multiline = true;
    box.AcceptsReturn = true;
    box.AcceptsTab = true;
    box.Dock = DockStyle.Fill;
    box.Font = new System.Drawing.Font("Consolas", 18);
    form.Controls.Add(box);
    form.Shown += (sender, args) => box.Focus();
    Application.Run(form);
  }
}
"@
  $needsBuild = $true
  if ((Test-Path $exePath) -and (Test-Path $sourcePath)) {
    $needsBuild = (Get-Item $sourcePath).LastWriteTimeUtc -gt (Get-Item $exePath).LastWriteTimeUtc
  }
  if ($needsBuild) {
    [System.IO.File]::WriteAllText($sourcePath, $source, [System.Text.UTF8Encoding]::new($false))
    Add-Type -TypeDefinition $source `
      -ReferencedAssemblies @("System.Windows.Forms", "System.Drawing") `
      -OutputAssembly $exePath `
      -OutputType WindowsApplication
  }
  return $exePath
}

function Stop-BrowserProfileProcesses($ProfilePath) {
  if ([string]::IsNullOrWhiteSpace($ProfilePath)) {
    return
  }
  $escaped = [Regex]::Escape($ProfilePath)
  $processes = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match "--user-data-dir=`"?$escaped`"?" }
  foreach ($process in $processes) {
    Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
  }
}

function Start-InputTarget($TargetName) {
  $startedAt = Get-Date
  if ($TargetName -eq "notepad") {
    $fixture = Join-Path $env:TEMP "openless-notepad-input-fixture.txt"
    Write-TextUtf8 $fixture ""
    $launcher = Start-Process notepad.exe -ArgumentList $fixture -PassThru
    $process = Wait-ProcessWindow "Notepad" $startedAt 15
    if ($null -eq $process) {
      throw "Notepad window process was not found."
    }
    $title = $process.MainWindowTitle
    if ([string]::IsNullOrWhiteSpace($title)) {
      $title = "openless-notepad-input-fixture.txt - Notepad"
    }
    $activateScript = @"
import sys, time, win32com.client
title = sys.argv[1]
shell = win32com.client.Dispatch('WScript.Shell')
deadline = time.time() + 10
while time.time() < deadline:
    if shell.AppActivate(title):
        print('activated')
        raise SystemExit(0)
    time.sleep(0.2)
raise SystemExit(1)
"@
    $activatePath = Join-Path $env:TEMP "openless-activate-notepad.py"
    Write-TextUtf8 $activatePath $activateScript
    try {
      python $activatePath $title | Out-Null
    } finally {
      Remove-Item -LiteralPath $activatePath -Force -ErrorAction SilentlyContinue
      if ($null -ne $launcher) {
        $launcher | Out-Null
      }
    }
    Start-Sleep -Milliseconds 800
    return [pscustomobject]@{
      Process = $process
      FixturePath = $fixture
      ProfilePath = $null
      TargetTitle = $title
      TargetPid = $process.Id
      TargetKind = "notepad"
    }
  }

  if ($TargetName -in @("wt-cmd", "wt-powershell")) {
    $wt = Get-Command wt.exe -ErrorAction SilentlyContinue
    if ($null -eq $wt) {
      throw "wt.exe was not found."
    }
    $profile = if ($TargetName -eq "wt-cmd") { "cmd.exe" } else { "powershell.exe" }
    Start-Process -FilePath $wt.Source -ArgumentList @("new-tab", $profile) | Out-Null
    Start-Sleep -Seconds 2
    $title = if ($TargetName -eq "wt-cmd") { "C:\WINDOWS\system32\cmd.exe" } else { "Windows PowerShell" }
    $activateScript = @"
import sys, time, win32com.client
title = sys.argv[1]
shell = win32com.client.Dispatch('WScript.Shell')
deadline = time.time() + 10
while time.time() < deadline:
    if shell.AppActivate(title):
        print('activated')
        raise SystemExit(0)
    time.sleep(0.2)
raise SystemExit(1)
"@
    $activatePath = Join-Path $env:TEMP "openless-activate-target.py"
    Write-TextUtf8 $activatePath $activateScript
    try {
      python $activatePath $title | Out-Null
    } finally {
      Remove-Item -LiteralPath $activatePath -Force -ErrorAction SilentlyContinue
    }
    $handleLookup = @"
import sys
from pywinauto import Desktop

title = sys.argv[1]
for window in Desktop(backend='uia').windows():
    if window.class_name() == 'CASCADIA_HOSTING_WINDOW_CLASS' and window.window_text() == title:
        print(window.handle)
        raise SystemExit(0)
raise SystemExit(1)
"@
    $handlePath = Join-Path $env:TEMP "openless-terminal-handle.py"
    Write-TextUtf8 $handlePath $handleLookup
    try {
      $targetHandle = [int](python -X utf8 $handlePath $title)
    } finally {
      Remove-Item -LiteralPath $handlePath -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Milliseconds 800
    return [pscustomobject]@{
      Process = $null
      FixturePath = $null
      ProfilePath = $null
      TargetTitle = $title
      TargetHandle = $targetHandle
      TargetKind = "terminal"
    }
  }
  if ($TargetName -eq "win32edit") {
    $hostExe = New-Win32EditHost
    Start-Process -FilePath $hostExe | Out-Null
    $process = Wait-ProcessWindow "OpenLessWin32EditHost" $startedAt 15
    if (-not (Focus-Window $process)) {
      throw "Win32 edit host window could not be focused."
    }
    return [pscustomobject]@{ Process = $process; FixturePath = $null; ProfilePath = $null }
  }

  $browserPath = Resolve-BrowserPath
  $fixture = New-BrowserInputFixture
  $url = ([System.Uri]$fixture).AbsoluteUri
  $processName = [System.IO.Path]::GetFileNameWithoutExtension($browserPath)
  $launcher = Start-Process -FilePath $browserPath -ArgumentList @(
    "--guest",
    "--new-window",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-extensions",
    $url
  ) -PassThru
  $window = Wait-BrowserWindow "OpenLess Browser Input Fixture" 20
  if ($null -eq $window) {
    throw "Browser window process was not found."
  }
  $process = Get-Process -Id $window.pid -ErrorAction Stop
  $process | Add-Member -NotePropertyName MainWindowHandleOverride -NotePropertyValue ([int64]$window.handle) -Force
  if (-not (Focus-Window $process)) {
    throw "Browser window could not be focused."
  }
  if (-not (Focus-BrowserTextarea $process)) {
    throw "Browser textarea could not be focused."
  }
  Start-Sleep -Seconds 1
  return [pscustomobject]@{
    Process = $process
    FixturePath = $fixture
    ProfilePath = $null
    TargetKind = "browser"
    TargetPid = $process.Id
    TargetHandle = [int64]$window.handle
    TargetTitle = $window.title
  }
}

function Read-TargetContent($TargetInfo, $TargetName) {
  if ($TargetName -eq "notepad") {
    $readbackScript = @"
import json
import sys
import time
from pywinauto import Desktop

pid = int(sys.argv[1])
out = sys.argv[2]
debug_out = sys.argv[3]

def collect_debug():
    payload = {"pid": pid, "windows": []}
    for win in [w for w in Desktop(backend='uia').windows() if getattr(w, 'process_id', lambda: None)() == pid]:
        win_info = {
            "title": "",
            "class_name": "",
            "descendants": [],
        }
        try:
            win_info["title"] = win.window_text()
        except Exception as exc:
            win_info["title"] = f"<title error: {exc}>"
        try:
            win_info["class_name"] = win.class_name()
        except Exception as exc:
            win_info["class_name"] = f"<class error: {exc}>"
        for descendant in win.descendants():
            try:
                cls = descendant.class_name()
            except Exception as exc:
                cls = f"<class error: {exc}>"
            try:
                control_type = descendant.element_info.control_type
            except Exception as exc:
                control_type = f"<type error: {exc}>"
            try:
                name = descendant.window_text()
            except Exception as exc:
                name = f"<name error: {exc}>"
            try:
                value = descendant.iface_value.CurrentValue
            except Exception:
                value = None
            if cls == 'RichEditD2DPT' or control_type == 'Document':
                win_info["descendants"].append({
                    "class_name": cls,
                    "control_type": control_type,
                    "name": name,
                    "value": value,
                })
        payload["windows"].append(win_info)
    with open(debug_out, 'w', encoding='utf-8') as fh:
        json.dump(payload, fh, ensure_ascii=False, indent=2)

def read_notepad_text():
    windows = [w for w in Desktop(backend='uia').windows() if getattr(w, 'process_id', lambda: None)() == pid]
    if not windows:
        return None

    win = None
    for candidate in windows:
        try:
            if candidate.class_name() == 'Notepad':
                win = candidate
                break
        except Exception:
            continue
    if win is None:
        win = windows[0]

    for descendant in win.descendants():
        try:
            cls = descendant.class_name()
            control_type = descendant.element_info.control_type
        except Exception:
            continue
        if cls != 'RichEditD2DPT' and control_type != 'Document':
            continue

        for getter in (
            lambda: descendant.iface_value.CurrentValue,
            lambda: descendant.window_text(),
            lambda: getattr(descendant.element_info, 'name', ''),
        ):
            try:
                value = getter()
            except Exception:
                continue
            if value is not None and str(value).strip():
                return str(value)
    return ''

deadline = time.time() + 5
last_text = None
while time.time() < deadline:
    last_text = read_notepad_text()
    if last_text is None:
        time.sleep(0.2)
        continue
    if last_text.strip():
        open(out, 'w', encoding='utf-8').write(last_text)
        raise SystemExit(0)
    time.sleep(0.2)

if last_text is None:
    collect_debug()
    raise SystemExit(2)
collect_debug()
open(out, 'w', encoding='utf-8').write(last_text)
raise SystemExit(1)
"@
    $readbackPath = Join-Path $env:TEMP "openless-notepad-readback.py"
    $outputPath = Join-Path $env:TEMP "openless-notepad-readback.txt"
    $debugPath = Join-Path $env:TEMP "openless-notepad-readback-debug.json"
    Write-TextUtf8 $readbackPath $readbackScript
    try {
      Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
      Remove-Item -LiteralPath $debugPath -Force -ErrorAction SilentlyContinue
      python -X utf8 $readbackPath $TargetInfo.TargetPid $outputPath $debugPath | Out-Null
      if (Test-Path $outputPath) {
        return Get-Content -Raw -Encoding UTF8 $outputPath
      }
      if (Test-Path $debugPath) {
        Write-Warning "notepad readback debug: $(Get-Content -Raw -Encoding UTF8 $debugPath)"
      }
      return $null
    } finally {
      Remove-Item -LiteralPath $readbackPath -Force -ErrorAction SilentlyContinue
      Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
      Remove-Item -LiteralPath $debugPath -Force -ErrorAction SilentlyContinue
    }
  }

  if ($TargetName -eq "browser") {
    $readbackScript = @"
import json
import sys
from pywinauto import Application

pid = int(sys.argv[1])
out = sys.argv[2]
debug_out = sys.argv[3]

app = Application(backend='uia').connect(process=pid)
win = app.top_window()
payload = {"pid": pid, "window": win.window_text(), "candidates": []}

def rank(descendant):
    cls = descendant.class_name()
    if cls == 'OmniboxViewViews':
        return -1
    rect = descendant.rectangle()
    area = max(rect.width(), 0) * max(rect.height(), 0)
    score = area
    if cls == '':
        score += 1000000
    return score

candidates = []
for descendant in win.descendants():
    try:
        if descendant.element_info.control_type != 'Edit':
            continue
        rect = descendant.rectangle()
        value = ''
        try:
            value = descendant.iface_value.CurrentValue or ''
        except Exception:
            value = ''
        info = {
            "class_name": descendant.class_name(),
            "name": descendant.window_text(),
            "value": value,
            "rect": [rect.left, rect.top, rect.right, rect.bottom],
            "score": rank(descendant),
        }
        payload["candidates"].append(info)
        if info["score"] >= 0:
            candidates.append((info["score"], info, descendant))
    except Exception:
        continue

with open(debug_out, 'w', encoding='utf-8') as fh:
    json.dump(payload, fh, ensure_ascii=False, indent=2)

if not candidates:
    raise SystemExit(1)

candidates.sort(key=lambda item: item[0], reverse=True)
best_info = candidates[0][1]
best = candidates[0][2]
payload["selected"] = best_info

for getter in (
    lambda: best.iface_value.CurrentValue,
    lambda: best.window_text(),
    lambda: getattr(best.element_info, 'name', ''),
):
    try:
        value = getter()
    except Exception:
        continue
    if value is not None:
        open(out, 'w', encoding='utf-8').write(str(value))
        raise SystemExit(0)

raise SystemExit(1)
"@
    $readbackPath = Join-Path $env:TEMP "openless-browser-readback.py"
    $outputPath = Join-Path $env:TEMP "openless-browser-readback.txt"
    $debugPath = Join-Path $env:TEMP "openless-browser-readback-debug.json"
    Write-TextUtf8 $readbackPath $readbackScript
    try {
      Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
      Remove-Item -LiteralPath $debugPath -Force -ErrorAction SilentlyContinue
      python -X utf8 $readbackPath $TargetInfo.TargetPid $outputPath $debugPath | Out-Null
      if (Test-Path $outputPath) {
        $value = Get-Content -Raw -Encoding UTF8 $outputPath
        if ([string]::IsNullOrWhiteSpace($value) -and (Test-Path $debugPath)) {
          Write-Warning "browser readback debug: $(Get-Content -Raw -Encoding UTF8 $debugPath)"
        }
        return $value
      }
      if (Test-Path $debugPath) {
        Write-Warning "browser readback debug: $(Get-Content -Raw -Encoding UTF8 $debugPath)"
      }
      return $null
    } finally {
      Remove-Item -LiteralPath $readbackPath -Force -ErrorAction SilentlyContinue
      Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
      Remove-Item -LiteralPath $debugPath -Force -ErrorAction SilentlyContinue
    }
  }

  if ($TargetName -in @("wt-cmd", "wt-powershell")) {
    $readbackScript = @"
import sys
from pywinauto import Desktop

handle = int(sys.argv[1])
out = sys.argv[2]
win = Desktop(backend='uia').window(handle=handle)
for descendant in win.descendants():
    if descendant.class_name() == 'TermControl':
        open(out, 'w', encoding='utf-8').write(descendant.window_text())
        raise SystemExit(0)
raise SystemExit(1)
"@
    $readbackPath = Join-Path $env:TEMP "openless-terminal-readback.py"
    $outputPath = Join-Path $env:TEMP "openless-terminal-readback.txt"
    Write-TextUtf8 $readbackPath $readbackScript
    try {
      Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
      python -X utf8 $readbackPath $TargetInfo.TargetHandle $outputPath | Out-Null
      if (Test-Path $outputPath) {
        return Get-Content -Raw -Encoding UTF8 $outputPath
      }
      return $null
    } finally {
      Remove-Item -LiteralPath $readbackPath -Force -ErrorAction SilentlyContinue
      Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
    }
  }

  if ($TargetName -eq "win32edit") {
    Focus-Window $TargetInfo.Process | Out-Null
    Start-Sleep -Milliseconds 400
    Send-CtrlChord 0x41
    Start-Sleep -Milliseconds 200
    Send-CtrlChord 0x43
    Start-Sleep -Milliseconds 400
    return Get-Clipboard -Raw -ErrorAction SilentlyContinue
  }

  return $null
}

function Send-CtrlChord($Vk) {
  Send-KeyEdge 0xA2 $false $false
  Start-Sleep -Milliseconds 80
  Send-KeyEdge $Vk $false $false
  Start-Sleep -Milliseconds 80
  Send-KeyEdge $Vk $true $false
  Start-Sleep -Milliseconds 80
  Send-KeyEdge 0xA2 $true $false
}

function Speak-TestPhrase($Text) {
  Add-Type -AssemblyName System.Speech
  $speaker = New-Object System.Speech.Synthesis.SpeechSynthesizer
  $speaker.Rate = -1
  $speaker.Volume = 100
  $speaker.Speak($Text)
}

$credentialStatus = Get-OpenLessCredentialStatus
if ($RequireJsonCredentials) {
  if ($AsrProvider -eq "volcengine" -and (-not $credentialStatus.VolcengineConfigured -or -not $credentialStatus.ArkConfigured)) {
    throw "Real ASR regression requires configured Volcengine ASR and Ark LLM credentials when ASR=volcengine."
  }
  if ($AsrProvider -eq "foundry-local-whisper" -and -not $credentialStatus.ArkConfigured) {
    Write-Warning "Ark LLM credentials are not configured; local ASR smoke accepts the existing raw transcript fallback when LLM is unconfigured."
  }
}
if (-not $credentialStatus.VolcengineConfigured -or -not $credentialStatus.ArkConfigured) {
  $missingCredentialParts = @()
  if (-not $credentialStatus.VolcengineConfigured) { $missingCredentialParts += "Volcengine ASR" }
  if (-not $credentialStatus.ArkConfigured) { $missingCredentialParts += "Ark LLM" }
  $providerCredentialNote = if ($AsrProvider -eq "volcengine") {
    "ASR=volcengine needs Volcengine ASR and Ark LLM credentials unless the app resolves them from the OS credential vault."
  } else {
    "ASR=foundry-local-whisper does not require Volcengine credentials; Ark LLM is optional because raw transcript fallback is accepted."
  }
  Write-Warning "Legacy credentials.json is incomplete ($($missingCredentialParts -join ', ')); $providerCredentialNote Continuing because the app may use the OS credential vault."
}

$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"
$historyPath = Join-Path $env:APPDATA "OpenLess\history.json"
$preferencesPath = Join-Path $env:APPDATA "OpenLess\preferences.json"
$credentialsPath = Join-Path $env:APPDATA "OpenLess\credentials.json"
$inputTarget = $null
$openless = $null
$previousPreferences = $null
$previousCredentials = $null
$previousClipboard = $null
$debugTranscriptPath = $null
$preferencesRewritten = $false
$credentialsRewritten = $false
$clipboardCaptured = $false

try {
  $baselineCount = Get-HistoryCount $historyPath
  $baselineLatest = Get-LatestHistory $historyPath
  $baselineLatestId = if ($null -ne $baselineLatest) { $baselineLatest.id } else { $null }
  $previousClipboard = Get-Clipboard -Raw -ErrorAction SilentlyContinue
  $clipboardCaptured = $true
  $previousPreferences = Set-HoldHotkeyPreference $preferencesPath
  $preferencesRewritten = $true
  $previousCredentials = Set-ActiveAsrCredential $credentialsPath
  $credentialsRewritten = $true
  $clipboardSentinel = "OPENLESS_OLD_CLIPBOARD_SENTINEL_$(Get-Date -Format 'yyyyMMddHHmmssfff')"
  Restore-ClipboardValue $clipboardSentinel
  if (-not [string]::IsNullOrWhiteSpace($InjectedTranscriptText)) {
    $debugTranscriptPath = Join-Path $env:TEMP "openless-debug-transcript.txt"
    Write-TextUtf8 $debugTranscriptPath $InjectedTranscriptText
  }

  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
  Remove-Item -LiteralPath $logPath -Force -ErrorAction SilentlyContinue

  Write-Host "== Real ASR + direct insertion smoke ($Target, ASR=$AsrProvider) =="
  $env:OPENLESS_SHOW_MAIN_ON_START = "1"
  $env:OPENLESS_ACCEPT_SYNTHETIC_HOTKEY_EVENTS = "1"
  if ($DebugHotkeyEvents) {
    $env:OPENLESS_DEBUG_HOTKEY_EVENTS = "1"
  }
  if ($debugTranscriptPath) {
    $env:OPENLESS_DEBUG_TRANSCRIPT_FILE = $debugTranscriptPath
  }
  try {
    $openless = Start-Process -FilePath $ExePath -WorkingDirectory (Split-Path $ExePath -Parent) -PassThru
  } finally {
    Remove-Item Env:OPENLESS_SHOW_MAIN_ON_START -ErrorAction SilentlyContinue
    Remove-Item Env:OPENLESS_ACCEPT_SYNTHETIC_HOTKEY_EVENTS -ErrorAction SilentlyContinue
    Remove-Item Env:OPENLESS_DEBUG_HOTKEY_EVENTS -ErrorAction SilentlyContinue
    Remove-Item Env:OPENLESS_DEBUG_TRANSCRIPT_FILE -ErrorAction SilentlyContinue
  }

  if (-not (Wait-LogPattern $logPath "hotkey listener installed|Windows low-level keyboard hook" 20)) {
    throw "Windows low-level keyboard hook was not installed."
  }

  $inputTarget = Start-InputTarget $Target

  $observedPress = $false
  for ($attempt = 1; $attempt -le 3 -and -not $observedPress; $attempt++) {
    Ensure-TargetFocused $inputTarget | Out-Null
    Press-Hotkey
    $observedPress = Wait-LogPattern $logPath "\[hotkey\] Windows trigger pressed" 4
    if (-not $observedPress) {
      Release-Hotkey
      Start-Sleep -Milliseconds 500
    }
  }
  if (-not $observedPress) {
    throw "Windows low-level hook did not observe the synthetic Control press."
  }
  if (-not (Wait-LogPattern $logPath "\[coord\] session started" 30)) {
    throw "OpenLess recording session did not start."
  }

  if ($ManualSpeech) {
    Write-Host "[action] Please speak into the real microphone for $ManualSpeechSeconds seconds."
    Start-Sleep -Seconds $ManualSpeechSeconds
  } else {
    Speak-TestPhrase $Phrase
  }
  Start-Sleep -Milliseconds 800
  Release-Hotkey

  if (-not (Wait-HistoryAdvance $historyPath $baselineCount $baselineLatestId $TimeoutSeconds)) {
    throw "History did not receive a new dictation session within $TimeoutSeconds seconds."
  }

  $latest = Get-LatestHistory $historyPath
  if ($null -eq $latest) {
    throw "History changed but latest item could not be read."
  }
  if ($latest.errorCode -eq "emptyTranscript") {
    throw "ASR returned an empty transcript. Hotkey, recorder, ASR session, history, and error status were exercised; real transcription still needs a microphone/audio route that captures the spoken phrase."
  }
  if ([string]::IsNullOrWhiteSpace($latest.rawTranscript) -or [string]::IsNullOrWhiteSpace($latest.finalText)) {
    throw "Latest history item is missing rawTranscript or finalText."
  }
  if ($latest.insertStatus -ne "inserted") {
    if (-not $AllowClipboardFallback -or @("copiedFallback", "pasteSent") -notcontains $latest.insertStatus) {
      throw "Expected Windows insertStatus inserted, got '$($latest.insertStatus)'."
    }
    Write-Warning "Clipboard fallback was allowed for this run. insertStatus=$($latest.insertStatus)"
  }

  $targetText = Read-TargetContent $inputTarget $Target

  if ([string]::IsNullOrWhiteSpace($targetText)) {
    throw "$Target readback is empty."
  }
  if (-not $targetText.Contains($latest.finalText)) {
    if ($targetText.Contains($clipboardSentinel)) {
      throw "$Target readback contains the pre-dictation clipboard sentinel instead of latest finalText."
    }
    throw "$Target readback does not contain latest finalText; insertion was not proven at the target caret."
  }

  Write-Host "[ok] History updated. raw='$($latest.rawTranscript)'"
  Write-Host "[ok] Final text length=$($latest.finalText.Length), insertStatus=$($latest.insertStatus)"
  Write-Host "[ok] $Target readback length=$($targetText.Length)"
  if ($PostSuccessDelaySeconds -gt 0) {
    Write-Host "[hold] Keeping OpenLess and target alive for $PostSuccessDelaySeconds seconds after insertion verification."
    Start-Sleep -Seconds $PostSuccessDelaySeconds
  }

  if (Test-Path $logPath) {
    $logText = Get-Content -Raw -Encoding UTF8 $logPath
    $forbiddenNativeDictationPattern = "Win\+H|Voice Typing|Windows\.Media\.SpeechRecognition|SpeechRecognizer|SAPI"
    if ($logText -match $forbiddenNativeDictationPattern) {
      throw "OpenLess log contains a native Windows dictation route marker; this smoke must use the OpenLess pipeline."
    }
  }
} finally {
  Release-Hotkey
  if ($null -ne $inputTarget) {
    if ($inputTarget.ProfilePath) {
      Stop-BrowserProfileProcesses $inputTarget.ProfilePath
    } elseif ($null -ne $inputTarget.Process) {
      Stop-Process -Id $inputTarget.Process.Id -Force -ErrorAction SilentlyContinue
    }
    if ($inputTarget.FixturePath) {
      Remove-Item -LiteralPath $inputTarget.FixturePath -Force -ErrorAction SilentlyContinue
    }
    if ($inputTarget.ProfilePath) {
      Remove-Item -LiteralPath $inputTarget.ProfilePath -Recurse -Force -ErrorAction SilentlyContinue
    }
  }
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
  if ($preferencesRewritten) {
    if ($null -eq $previousPreferences) {
      Remove-Item -LiteralPath $preferencesPath -Force -ErrorAction SilentlyContinue
    } else {
      Write-TextUtf8 $preferencesPath $previousPreferences
    }
  }
  if ($credentialsRewritten) {
    Restore-ActiveAsrCredential $previousCredentials $credentialsPath
  }
  if ($clipboardCaptured) {
    Restore-ClipboardValue $previousClipboard
  }
  if ($debugTranscriptPath) {
    Remove-Item -LiteralPath $debugTranscriptPath -Force -ErrorAction SilentlyContinue
  }
}

Write-Host "Real ASR + direct insertion smoke ($Target, ASR=$AsrProvider) passed."
