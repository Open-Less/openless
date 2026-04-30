param(
  [string]$ExePath = "",
  [int]$StartupTimeoutSeconds = 12,
  [switch]$RequireCredentials
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

function Test-CredentialValue($Value) {
  return ($null -ne $Value) -and ($Value -is [string]) -and ($Value.Trim().Length -gt 0)
}

function Get-OpenLessCredentialStatus {
  $path = Join-Path $env:APPDATA "OpenLess\credentials.json"
  if (-not (Test-Path $path)) {
    return [pscustomobject]@{
      Path = $path
      VolcengineConfigured = $false
      ArkConfigured = $false
      Present = $false
    }
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

function Wait-LogPattern($Path, $Pattern, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if ((Test-Path $Path) -and ((Get-Content -Raw $Path) -match $Pattern)) {
      return $true
    }
    Start-Sleep -Milliseconds 500
  }
  return $false
}

if (-not (Test-Path $ExePath)) {
  throw "OpenLess executable not found: $ExePath"
}

$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"
$credentialStatus = Get-OpenLessCredentialStatus

Write-Host "== Credential status =="
$credentialStatus | Format-List
if (-not $credentialStatus.VolcengineConfigured) {
  Write-Host "[warn] Volcengine ASR credentials are not configured; real transcription cannot be completed."
}
if (-not $credentialStatus.ArkConfigured) {
  Write-Host "[warn] Ark LLM credentials are not configured; polishing will fall back or fail depending on mode."
}
if ($RequireCredentials -and (-not $credentialStatus.VolcengineConfigured -or -not $credentialStatus.ArkConfigured)) {
  throw "Real regression requires configured Volcengine ASR and Ark LLM credentials."
}

Write-Host ""
Write-Host "== Launch smoke =="
$process = Start-Process -FilePath $ExePath -PassThru
try {
  Start-Sleep -Seconds 4
  $live = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
  if (-not $live) {
    throw "OpenLess exited during startup."
  }
  if (-not $live.Responding) {
    throw "OpenLess process is not responding."
  }
  Write-Host "[ok] Process responding: id=$($live.Id), title='$($live.MainWindowTitle)'"

  if (Wait-LogPattern $logPath "hotkey listener installed" $StartupTimeoutSeconds) {
    Write-Host "[ok] Hotkey listener installed according to log."
  } else {
    throw "Hotkey listener did not report installed within $StartupTimeoutSeconds seconds."
  }

  Write-Host ""
  Write-Host "Manual checks still required:"
  Write-Host "- Press the configured physical global hotkey to start/stop recording."
  Write-Host "- Speak a short phrase with valid ASR credentials configured."
  Write-Host "- Focus Notepad or another text field and verify Windows insert status falls back to copied/Ctrl+V when insertion cannot be confirmed."
  Write-Host "- Toggle Windows microphone privacy off/on and rerun Settings -> Permissions."
} finally {
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
}

Write-Host ""
Write-Host "Runtime smoke passed."
