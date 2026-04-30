param(
  [string]$ExePath = "",
  [int]$TimeoutSeconds = 30,
  [int]$VirtualKey = 0xA3
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

public static class OpenLessMicPrivacyWin32 {
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

function Send-KeyEdge($Vk, $KeyUp) {
  $flags = [OpenLessMicPrivacyWin32]::KEYEVENTF_EXTENDEDKEY
  if ($KeyUp) {
    $flags = $flags -bor [OpenLessMicPrivacyWin32]::KEYEVENTF_KEYUP
  }
  $scanCode = if ($Vk -eq 0xA3 -or $Vk -eq 0xA2) { 0x1D } else { 0 }
  [OpenLessMicPrivacyWin32]::keybd_event([byte]$Vk, [byte]$scanCode, $flags, [UIntPtr]::Zero)
}

function Press-Hotkey {
  Send-KeyEdge $VirtualKey $false
}

function Release-Hotkey {
  Send-KeyEdge $VirtualKey $true
}

function Focus-Window($Process) {
  if ($null -eq $Process -or $Process.MainWindowHandle -eq 0) {
    return $false
  }
  [OpenLessMicPrivacyWin32]::ShowWindow($Process.MainWindowHandle, 9) | Out-Null
  [OpenLessMicPrivacyWin32]::SetForegroundWindow($Process.MainWindowHandle) | Out-Null
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

function Get-ConsentSnapshot($Path) {
  $exists = Test-Path $Path
  $valueExists = $false
  $value = $null
  if ($exists) {
    $props = Get-ItemProperty -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($props -and $props.PSObject.Properties["Value"]) {
      $valueExists = $true
      $value = $props.Value
    }
  }
  [pscustomobject]@{
    Path = $Path
    Exists = $exists
    ValueExists = $valueExists
    Value = $value
  }
}

function Restore-ConsentSnapshot($Snapshot) {
  if (-not $Snapshot.Exists) {
    Remove-Item -LiteralPath $Snapshot.Path -Recurse -Force -ErrorAction SilentlyContinue
    return
  }
  if (-not (Test-Path $Snapshot.Path)) {
    New-Item -ItemType Directory -Path $Snapshot.Path | Out-Null
  }
  if ($Snapshot.ValueExists) {
    Set-ItemProperty -LiteralPath $Snapshot.Path -Name Value -Value $Snapshot.Value
  } else {
    Remove-ItemProperty -LiteralPath $Snapshot.Path -Name Value -ErrorAction SilentlyContinue
  }
}

function Set-ConsentValue($Path, $Value) {
  if (-not (Test-Path $Path)) {
    New-Item -ItemType Directory -Path $Path | Out-Null
  }
  Set-ItemProperty -LiteralPath $Path -Name Value -Value $Value
}

function Get-NonPackagedConsentPath($ExecutablePath) {
  $resolved = (Resolve-Path $ExecutablePath).Path
  $encoded = $resolved -replace "\\", "#"
  Join-Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged" $encoded
}

function Invoke-HotkeyAttempt($ExpectedPattern, $UnexpectedPattern, $Label) {
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
  Remove-Item -LiteralPath $logPath -Force -ErrorAction SilentlyContinue

  $env:OPENLESS_SHOW_MAIN_ON_START = "1"
  $env:OPENLESS_ACCEPT_SYNTHETIC_HOTKEY_EVENTS = "1"
  try {
    Start-Process -FilePath $ExePath -WorkingDirectory (Split-Path $ExePath -Parent) | Out-Null
  } finally {
    Remove-Item Env:OPENLESS_SHOW_MAIN_ON_START -ErrorAction SilentlyContinue
    Remove-Item Env:OPENLESS_ACCEPT_SYNTHETIC_HOTKEY_EVENTS -ErrorAction SilentlyContinue
  }

  $notepad = $null
  try {
    if (-not (Wait-LogPattern $logPath "WH_KEYBOARD_LL installed" 20)) {
      throw "${Label}: Windows low-level keyboard hook was not installed."
    }
    $notepadStart = Get-Date
    Start-Process notepad.exe | Out-Null
    $notepad = Wait-ProcessWindow "notepad" $notepadStart 15
    if (-not (Focus-Window $notepad)) {
      throw "${Label}: Notepad window could not be focused."
    }
    $observedPress = $false
    for ($attempt = 1; $attempt -le 3 -and -not $observedPress; $attempt++) {
      Press-Hotkey
      $observedPress = Wait-LogPattern $logPath "\[hotkey\] Windows trigger pressed" 4
      if (-not $observedPress) {
        Release-Hotkey
        Start-Sleep -Milliseconds 500
        Focus-Window $notepad | Out-Null
      }
    }
    if (-not $observedPress) {
      throw "${Label}: Windows low-level hook did not observe the right Control press after retries."
    }
    Start-Sleep -Milliseconds 900
    Release-Hotkey

    if (-not (Wait-LogPattern $logPath $ExpectedPattern $TimeoutSeconds)) {
      throw "${Label}: expected log pattern not observed: $ExpectedPattern"
    }
    if ($UnexpectedPattern -and (Test-Path $logPath) -and ((Get-Content -Raw $logPath) -match $UnexpectedPattern)) {
      throw "${Label}: unexpected log pattern observed: $UnexpectedPattern"
    }
  } finally {
    Release-Hotkey
    if ($null -ne $notepad) {
      Stop-Process -Id $notepad.Id -Force -ErrorAction SilentlyContinue
    }
    Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
  }
}

$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"
$preferencesPath = Join-Path $env:APPDATA "OpenLess\preferences.json"
$previousPreferences = Set-HoldHotkeyPreference $preferencesPath

$globalMicPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone"
$desktopMicPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged"
$appMicPath = Get-NonPackagedConsentPath $ExePath
$snapshots = @(
  (Get-ConsentSnapshot $globalMicPath),
  (Get-ConsentSnapshot $desktopMicPath),
  (Get-ConsentSnapshot $appMicPath)
)

Write-Host "== Windows microphone privacy smoke =="
try {
  Set-ConsentValue $globalMicPath "Deny"
  Set-ConsentValue $desktopMicPath "Deny"
  Set-ConsentValue $appMicPath "Deny"
  Invoke-HotkeyAttempt "microphone permission gate failed|input probe failed" "\[coord\] session started" "privacy denied"
  Write-Host "[ok] Denied state blocks recording before session start."

  Set-ConsentValue $globalMicPath "Allow"
  Set-ConsentValue $desktopMicPath "Allow"
  Set-ConsentValue $appMicPath "Allow"
  Invoke-HotkeyAttempt "\[coord\] session started" "microphone permission gate failed" "privacy restored"
  Write-Host "[ok] Restored state allows recording session start."
} finally {
  foreach ($snapshot in $snapshots) {
    Restore-ConsentSnapshot $snapshot
  }
  if ($null -eq $previousPreferences) {
    Remove-Item -LiteralPath $preferencesPath -Force -ErrorAction SilentlyContinue
  } else {
    Write-TextUtf8 $preferencesPath $previousPreferences
  }
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
}

Write-Host "Windows microphone privacy smoke passed."
