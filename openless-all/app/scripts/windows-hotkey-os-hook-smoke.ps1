param(
  [string]$ExePath = "",
  [int]$TimeoutSeconds = 20,
  [int]$VirtualKey = 0xA3
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $ExePath = Join-Path $appRoot "src-tauri\target\x86_64-pc-windows-gnu\release\openless.exe"
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

public static class OpenLessInput {
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

function Wait-LogPattern($Path, $Pattern, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if (Test-Path $Path) {
      $text = Get-Content -Raw $Path
      if ($text -match $Pattern) {
        return $true
      }
    }
    Start-Sleep -Milliseconds 250
  }
  return $false
}

function Send-KeyEdge($Vk, $KeyUp) {
  $flags = [OpenLessInput]::KEYEVENTF_EXTENDEDKEY
  if ($KeyUp) {
    $flags = $flags -bor [OpenLessInput]::KEYEVENTF_KEYUP
  }
  [OpenLessInput]::keybd_event([byte]$Vk, 0x1D, $flags, [UIntPtr]::Zero)
}

function Focus-Window($Process) {
  if ($null -eq $Process -or $Process.MainWindowHandle -eq 0) {
    return $false
  }
  [OpenLessInput]::ShowWindow($Process.MainWindowHandle, 9) | Out-Null
  [OpenLessInput]::SetForegroundWindow($Process.MainWindowHandle) | Out-Null
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

$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"
Remove-Item -LiteralPath $logPath -Force -ErrorAction SilentlyContinue
Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force

Write-Host "== Windows OS hotkey hook smoke =="
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
  if (-not (Wait-LogPattern $logPath "hotkey listener installed|Windows low-level keyboard hook" $TimeoutSeconds)) {
    throw "Windows low-level keyboard hook was not installed within $TimeoutSeconds seconds."
  }

  $notepadStart = Get-Date
  Start-Process notepad.exe | Out-Null
  $notepad = Wait-ProcessWindow "notepad" $notepadStart 15
  if (-not (Focus-Window $notepad)) {
    throw "Notepad window could not be focused."
  }

  $observedPress = $false
  for ($attempt = 1; $attempt -le 3 -and -not $observedPress; $attempt++) {
    Send-KeyEdge $VirtualKey $false
    $observedPress = Wait-LogPattern $logPath "\[hotkey\] Windows trigger pressed" 4
    Start-Sleep -Milliseconds 400
    Send-KeyEdge $VirtualKey $true
    if (-not $observedPress) {
      Start-Sleep -Milliseconds 500
      Focus-Window $notepad | Out-Null
    }
  }

  if (-not $observedPress) {
    throw "Windows hook did not observe synthetic vk=$VirtualKey press."
  }
  if (-not (Wait-LogPattern $logPath "\[coord\] hotkey pressed" $TimeoutSeconds)) {
    throw "Coordinator did not observe OS hook hotkey press."
  }
  Write-Host "[ok] Windows low-level hook observed vk=$VirtualKey and reached Coordinator."
} finally {
  if ($null -ne $notepad) {
    Stop-Process -Id $notepad.Id -Force -ErrorAction SilentlyContinue
  }
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
}

Write-Host "Windows OS hotkey hook smoke passed."
