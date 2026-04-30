param(
  [string]$ExePath = "",
  [int]$StartupTimeoutSeconds = 12,
  [int]$PhysicalHotkeyTimeoutSeconds = 45,
  [switch]$RequireCredentials,
  [switch]$PhysicalHotkey,
  [switch]$InsertionFallback,
  [switch]$MicrophonePrivacy
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
      Present = $false
      VolcengineConfigured = $false
      ArkConfigured = $false
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

function Wait-HistoryChange($Path, $BaselineWriteTime, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if ((Test-Path $Path)) {
      $current = (Get-Item $Path).LastWriteTimeUtc
      if ($null -eq $BaselineWriteTime -or $current -gt $BaselineWriteTime) {
        return $true
      }
    }
    Start-Sleep -Milliseconds 500
  }
  return $false
}

if (-not (Test-Path $ExePath)) {
  throw "OpenLess executable not found: $ExePath"
}

$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"
$historyPath = Join-Path $env:APPDATA "OpenLess\history.json"
$credentialStatus = Get-OpenLessCredentialStatus

Write-Host "== Credential gate =="
$credentialStatus | Format-List
if ($RequireCredentials -and (-not $credentialStatus.VolcengineConfigured -or -not $credentialStatus.ArkConfigured)) {
  throw "Real regression requires configured Volcengine ASR and Ark LLM credentials."
}

Write-Host ""
Write-Host "== Launch gate =="
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

  if ($PhysicalHotkey) {
    Write-Host ""
    Write-Host "== Physical hotkey gate =="
    Write-Host "Press the configured physical OpenLess hotkey now. Synthetic SendInput is not accepted for this gate."
    if (-not (Wait-LogPattern $logPath "\[coord\] hotkey pressed" $PhysicalHotkeyTimeoutSeconds)) {
      throw "No physical hotkey press was observed in the log within $PhysicalHotkeyTimeoutSeconds seconds."
    }
    Write-Host "[ok] Physical hotkey press observed."
  }

  if ($InsertionFallback) {
    Write-Host ""
    Write-Host "== Insertion fallback gate =="
    $baseline = $null
    if (Test-Path $historyPath) {
      $baseline = (Get-Item $historyPath).LastWriteTimeUtc
    }
    $notepad = Start-Process notepad.exe -PassThru
    Write-Host "Notepad launched. Focus the edit area, use the physical hotkey, speak a short phrase, then finish recording."
    if (-not (Wait-HistoryChange $historyPath $baseline 120)) {
      throw "History did not change within 120 seconds after manual recording."
    }
    Write-Host "[ok] History changed after manual recording. Inspect the capsule/history insert status for inserted vs copiedFallback."
    Stop-Process -Id $notepad.Id -Force -ErrorAction SilentlyContinue
  }

  if ($MicrophonePrivacy) {
    Write-Host ""
    Write-Host "== Microphone privacy gate =="
    Start-Process "ms-settings:privacy-microphone"
    Write-Host "Toggle microphone privacy off, return to OpenLess Settings -> Permissions, confirm it no longer reports granted, then toggle it back on and rerun this script."
  }
} finally {
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
}

Write-Host ""
Write-Host "Windows real regression script completed."
