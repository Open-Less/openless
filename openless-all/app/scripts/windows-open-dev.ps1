param(
  [string]$ExePath = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $ExePath = Join-Path $appRoot ".artifacts\windows-gnu\dev\openless.exe"
}

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class OpenLessWindow {
  [DllImport("user32.dll")]
  public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

  [DllImport("user32.dll")]
  public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@

function Show-OpenLessWindow($Process) {
  if ($null -eq $Process -or $Process.MainWindowHandle -eq 0) {
    return $false
  }

  # 9 = SW_RESTORE. This restores minimized windows and leaves normal windows visible.
  [OpenLessWindow]::ShowWindow($Process.MainWindowHandle, 9) | Out-Null
  [OpenLessWindow]::SetForegroundWindow($Process.MainWindowHandle) | Out-Null
  return $true
}

$running = Get-Process openless -ErrorAction SilentlyContinue |
  Where-Object { $_.MainWindowHandle -ne 0 } |
  Select-Object -First 1

if (Show-OpenLessWindow $running) {
  Write-Host "OpenLess is already running; brought window to foreground. pid=$($running.Id)"
  exit 0
}

if (-not (Test-Path $ExePath)) {
  throw "OpenLess executable not found: $ExePath. Run scripts/windows-build-gnu.ps1 first."
}

if (-not (Test-Path (Join-Path (Split-Path $ExePath -Parent) "WebView2Loader.dll"))) {
  throw "WebView2Loader.dll not found beside $ExePath. Run scripts/windows-build-gnu.ps1 again."
}

if (-not $env:SystemDrive) {
  $env:SystemDrive = "C:"
}
if (-not $env:ProgramData) {
  $env:ProgramData = Join-Path $env:SystemDrive "ProgramData"
}
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\scoop\persist\rustup\.cargo\bin;$env:USERPROFILE\scoop\apps\rustup\current\.cargo\bin;$env:USERPROFILE\scoop\apps\mingw\current\bin;$env:PATH"
$env:OPENLESS_SHOW_MAIN_ON_START = "1"
try {
  $process = Start-Process -FilePath $ExePath -WorkingDirectory (Split-Path $ExePath -Parent) -PassThru
} finally {
  Remove-Item Env:OPENLESS_SHOW_MAIN_ON_START -ErrorAction SilentlyContinue
}
$deadline = (Get-Date).AddSeconds(10)

while ((Get-Date) -lt $deadline) {
  Start-Sleep -Milliseconds 250
  $current = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
  if (Show-OpenLessWindow $current) {
    Write-Host "OpenLess started and brought to foreground. pid=$($current.Id)"
    exit 0
  }
}

throw "OpenLess started but no main window was visible within 10 seconds. pid=$($process.Id)"
