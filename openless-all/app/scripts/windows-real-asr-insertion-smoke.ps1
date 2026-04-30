param(
  [string]$ExePath = "",
  [ValidateSet("notepad", "browser")]
  [string]$Target = "notepad",
  [string]$Phrase = "OpenLess Windows real regression",
  [int]$TimeoutSeconds = 120,
  [int]$VirtualKey = 0xA3,
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
    $prefs.hotkey | Add-Member -NotePropertyName trigger -NotePropertyValue "rightControl"
  } else {
    $prefs.hotkey.trigger = "rightControl"
  }
  if ($null -eq $prefs.hotkey.PSObject.Properties["mode"]) {
    $prefs.hotkey | Add-Member -NotePropertyName mode -NotePropertyValue "hold"
  } else {
    $prefs.hotkey.mode = "hold"
  }
  if ($null -eq $prefs.defaultMode) { $prefs | Add-Member -NotePropertyName defaultMode -NotePropertyValue "light" }
  if ($null -eq $prefs.enabledModes) { $prefs | Add-Member -NotePropertyName enabledModes -NotePropertyValue @("light", "structured", "formal", "raw") }
  if ($null -eq $prefs.launchAtLogin) { $prefs | Add-Member -NotePropertyName launchAtLogin -NotePropertyValue $false }
  if ($null -eq $prefs.showCapsule) { $prefs | Add-Member -NotePropertyName showCapsule -NotePropertyValue $true }
  if ($null -eq $prefs.activeAsrProvider) { $prefs | Add-Member -NotePropertyName activeAsrProvider -NotePropertyValue "volcengine" }
  if ($null -eq $prefs.activeLlmProvider) { $prefs | Add-Member -NotePropertyName activeLlmProvider -NotePropertyValue "ark" }
  Write-TextUtf8 $Path ($prefs | ConvertTo-Json -Depth 8)
  return $previous
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

function Wait-HistoryCountGreaterThan($Path, $Baseline, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    $count = Get-HistoryCount $Path
    if ($count -gt $Baseline) {
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

function Tap-Hotkey {
  Send-KeyEdge $VirtualKey $false $true
  Start-Sleep -Milliseconds 180
  Send-KeyEdge $VirtualKey $true $true
}

function Press-Hotkey {
  Send-KeyEdge $VirtualKey $false $true
}

function Release-Hotkey {
  Send-KeyEdge $VirtualKey $true $true
}

function Focus-Window($Process) {
  if ($null -eq $Process -or $Process.MainWindowHandle -eq 0) {
    return $false
  }
  [OpenLessRegressionWin32]::ShowWindow($Process.MainWindowHandle, 9) | Out-Null
  [OpenLessRegressionWin32]::SetForegroundWindow($Process.MainWindowHandle) | Out-Null
  Start-Sleep -Milliseconds 500
  return $true
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
    Start-Process notepad.exe | Out-Null
    $process = Wait-ProcessWindow "notepad" $startedAt 15
    if (-not (Focus-Window $process)) {
      throw "Notepad window could not be focused."
    }
    return [pscustomobject]@{ Process = $process; FixturePath = $null; ProfilePath = $null }
  }

  $browserPath = Resolve-BrowserPath
  $fixture = New-BrowserInputFixture
  $url = ([System.Uri]$fixture).AbsoluteUri
  $processName = [System.IO.Path]::GetFileNameWithoutExtension($browserPath)
  $profilePath = Join-Path $env:TEMP "openless-browser-smoke-profile"
  Stop-BrowserProfileProcesses $profilePath
  Remove-Item -LiteralPath $profilePath -Recurse -Force -ErrorAction SilentlyContinue
  Start-Process -FilePath $browserPath -ArgumentList @(
    "--new-window",
    "--user-data-dir=$profilePath",
    "--no-first-run",
    "--disable-extensions",
    $url
  ) | Out-Null
  $process = Wait-ProcessWindow $processName $startedAt 20
  if (-not (Focus-Window $process)) {
    throw "Browser window could not be focused."
  }
  Start-Sleep -Seconds 1
  return [pscustomobject]@{ Process = $process; FixturePath = $fixture; ProfilePath = $profilePath }
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
if (-not $credentialStatus.VolcengineConfigured -or -not $credentialStatus.ArkConfigured) {
  throw "Real ASR regression requires configured Volcengine ASR and Ark LLM credentials."
}

$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"
$historyPath = Join-Path $env:APPDATA "OpenLess\history.json"
$preferencesPath = Join-Path $env:APPDATA "OpenLess\preferences.json"
$baselineCount = Get-HistoryCount $historyPath
$previousPreferences = Set-HoldHotkeyPreference $preferencesPath

Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item -LiteralPath $logPath -Force -ErrorAction SilentlyContinue

Write-Host "== Real ASR + insertion fallback smoke ($Target) =="
$env:OPENLESS_SHOW_MAIN_ON_START = "1"
$env:OPENLESS_ACCEPT_SYNTHETIC_HOTKEY_EVENTS = "1"
if ($DebugHotkeyEvents) {
  $env:OPENLESS_DEBUG_HOTKEY_EVENTS = "1"
}
try {
  $openless = Start-Process -FilePath $ExePath -WorkingDirectory (Split-Path $ExePath -Parent) -PassThru
} finally {
  Remove-Item Env:OPENLESS_SHOW_MAIN_ON_START -ErrorAction SilentlyContinue
  Remove-Item Env:OPENLESS_ACCEPT_SYNTHETIC_HOTKEY_EVENTS -ErrorAction SilentlyContinue
  Remove-Item Env:OPENLESS_DEBUG_HOTKEY_EVENTS -ErrorAction SilentlyContinue
}

$inputTarget = $null
try {
  if (-not (Wait-LogPattern $logPath "WH_KEYBOARD_LL installed" 20)) {
    throw "Windows low-level keyboard hook was not installed."
  }

  $inputTarget = Start-InputTarget $Target

  Press-Hotkey
  if (-not (Wait-LogPattern $logPath "\[hotkey\] Windows trigger pressed" 10)) {
    throw "Windows low-level hook did not observe the right Control press."
  }
  if (-not (Wait-LogPattern $logPath "\[coord\] session started" 30)) {
    throw "OpenLess recording session did not start."
  }

  Speak-TestPhrase $Phrase
  Start-Sleep -Milliseconds 800
  Release-Hotkey

  if (-not (Wait-HistoryCountGreaterThan $historyPath $baselineCount $TimeoutSeconds)) {
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
  if ($latest.insertStatus -ne "copiedFallback") {
    throw "Expected Windows insertStatus copiedFallback, got '$($latest.insertStatus)'."
  }

  Focus-Window $inputTarget.Process | Out-Null
  Start-Sleep -Milliseconds 400
  Send-CtrlChord 0x41
  Start-Sleep -Milliseconds 200
  Send-CtrlChord 0x43
  Start-Sleep -Milliseconds 400
  $targetText = Get-Clipboard -Raw -ErrorAction SilentlyContinue

  if ([string]::IsNullOrWhiteSpace($targetText)) {
    throw "$Target clipboard readback is empty after Ctrl+A/C."
  }

  Write-Host "[ok] History updated. raw='$($latest.rawTranscript)'"
  Write-Host "[ok] Final text length=$($latest.finalText.Length), insertStatus=$($latest.insertStatus)"
  Write-Host "[ok] $Target readback length=$($targetText.Length)"
} finally {
  Release-Hotkey
  if ($null -ne $inputTarget) {
    if ($inputTarget.ProfilePath) {
      Stop-BrowserProfileProcesses $inputTarget.ProfilePath
    } else {
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
  if ($null -eq $previousPreferences) {
    Remove-Item -LiteralPath $preferencesPath -Force -ErrorAction SilentlyContinue
  } else {
    Write-TextUtf8 $preferencesPath $previousPreferences
  }
}

Write-Host "Real ASR + insertion fallback smoke ($Target) passed."
