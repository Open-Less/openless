param(
  [string]$ExePath = "",
  [int]$TimeoutSeconds = 15
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
  $ExePath = Join-Path $appRoot "src-tauri\target\debug\openless.exe"
}

if (-not (Test-Path $ExePath)) {
  throw "OpenLess executable not found: $ExePath"
}

$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"
Remove-Item -LiteralPath $logPath -Force -ErrorAction SilentlyContinue
Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class OpenLessCapsuleProbe {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern IntPtr FindWindowW(string lpClassName, string lpWindowName);

  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool IsWindowVisible(IntPtr hWnd);

  [DllImport("user32.dll")]
  public static extern void keybd_event(byte bVk, byte bScan, int dwFlags, UIntPtr dwExtraInfo);

  public const int KEYEVENTF_EXTENDEDKEY = 0x0001;
  public const int KEYEVENTF_KEYUP = 0x0002;
}
"@

function Wait-LogPattern($Pattern, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if ((Test-Path $logPath) -and ((Get-Content -Raw $logPath) -match $Pattern)) {
      return $true
    }
    Start-Sleep -Milliseconds 200
  }
  return $false
}

function Send-KeyEdge([byte]$Vk, [bool]$KeyUp) {
  $flags = [OpenLessCapsuleProbe]::KEYEVENTF_EXTENDEDKEY
  if ($KeyUp) {
    $flags = $flags -bor [OpenLessCapsuleProbe]::KEYEVENTF_KEYUP
  }
  [OpenLessCapsuleProbe]::keybd_event($Vk, 0x1D, $flags, [UIntPtr]::Zero)
}

function Get-CapsuleWindowState() {
  $hwnd = [OpenLessCapsuleProbe]::FindWindowW($null, "OpenLess Capsule")
  if ($hwnd -eq [IntPtr]::Zero) {
    return [pscustomobject]@{
      Exists = $false
      Visible = $false
      Handle = "0x0"
    }
  }

  return [pscustomobject]@{
    Exists = $true
    Visible = [OpenLessCapsuleProbe]::IsWindowVisible($hwnd)
    Handle = ('0x{0:X}' -f $hwnd.ToInt64())
  }
}

Write-Host "== Windows capsule lifecycle smoke =="
$env:OPENLESS_ACCEPT_SYNTHETIC_HOTKEY_EVENTS = "1"
$env:OPENLESS_HOTKEY_INJECTION_DRY_RUN = "1"
$process = Start-Process -FilePath $ExePath -WorkingDirectory (Split-Path $ExePath -Parent) -PassThru
try {
  if (-not (Wait-LogPattern "hotkey listener installed" $TimeoutSeconds)) {
    throw "Hotkey listener did not install within $TimeoutSeconds seconds."
  }

  Start-Sleep -Milliseconds 500
  $before = Get-CapsuleWindowState

  Send-KeyEdge 0xA3 $false
  Start-Sleep -Milliseconds 120
  Send-KeyEdge 0xA3 $true

  $startedDryRun = Wait-LogPattern "session started \(hotkey-injection dry-run\)" 5
  Start-Sleep -Milliseconds 400
  $afterStart = Get-CapsuleWindowState

  Send-KeyEdge 0xA3 $false
  Start-Sleep -Milliseconds 120
  Send-KeyEdge 0xA3 $true
  Start-Sleep -Seconds 3
  $afterStop = Get-CapsuleWindowState

  [pscustomobject]@{
    StartedDryRun = $startedDryRun
    Before = "$($before.Handle) visible=$($before.Visible)"
    AfterStart = "$($afterStart.Handle) visible=$($afterStart.Visible)"
    AfterStop = "$($afterStop.Handle) visible=$($afterStop.Visible)"
  } | Format-List

  if (-not $startedDryRun) {
    throw "Dry-run session did not start; cannot verify capsule lifecycle."
  }

  if (-not $afterStart.Visible) {
    throw "Capsule did not become visible during synthetic recording start."
  }

  if ($afterStop.Visible) {
    throw "Capsule is still visible after synthetic stop."
  }

  Write-Host "[ok] Capsule window is not visible after synthetic stop."
}
finally {
  Remove-Item Env:OPENLESS_ACCEPT_SYNTHETIC_HOTKEY_EVENTS -ErrorAction SilentlyContinue
  Remove-Item Env:OPENLESS_HOTKEY_INJECTION_DRY_RUN -ErrorAction SilentlyContinue
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
}
