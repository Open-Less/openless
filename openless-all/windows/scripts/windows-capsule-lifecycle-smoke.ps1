param(
  [string]$ExePath = "",
  [int]$TimeoutSeconds = 15
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\app")).Path
  $ExePath = Join-Path $appRoot "src-tauri\target\debug\openless.exe"
}

if (-not (Test-Path $ExePath)) {
  throw "OpenLess executable not found: $ExePath"
}

$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"
$existingOpenLess = @(Get-Process openless -ErrorAction SilentlyContinue)
foreach ($existingProcess in $existingOpenLess) {
  Stop-Process -Id $existingProcess.Id -Force -ErrorAction SilentlyContinue
}
if ($existingOpenLess.Count -gt 0) {
  Start-Sleep -Milliseconds 300
}
Remove-Item -LiteralPath $logPath -Force -ErrorAction SilentlyContinue

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class OpenLessCapsuleProbe {
  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool IsWindowVisible(IntPtr hWnd);

  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

  [DllImport("user32.dll")]
  private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  private static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int maxCount);

  private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

  public static IntPtr FindVisibleCapsuleWindowForProcess(int processId) {
    var result = IntPtr.Zero;
    EnumWindows((hWnd, _) => {
      if (!IsWindowVisible(hWnd)) {
        return true;
      }
      uint ownerPid;
      GetWindowThreadProcessId(hWnd, out ownerPid);
      if (ownerPid != (uint)processId) {
        return true;
      }
      var title = new StringBuilder(256);
      GetWindowText(hWnd, title, title.Capacity);
      if (title.ToString() == "OpenLess Capsule") {
        result = hWnd;
        return false;
      }
      return true;
    }, IntPtr.Zero);
    return result;
  }

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

function Get-LogCount($Pattern) {
  if (-not (Test-Path $logPath)) {
    return 0
  }
  return ([regex]::Matches((Get-Content -Raw $logPath), $Pattern)).Count
}

function Get-KeyScanCode($Vk) {
  switch ([int]$Vk) {
    0xA0 { return 0x2A }
    0xA1 { return 0x36 }
    0xA2 { return 0x1D }
    0xA3 { return 0x1D }
    0xA4 { return 0x38 }
    0xA5 { return 0x38 }
    0x5B { return 0x5B }
    0x5C { return 0x5C }
    default { return 0 }
  }
}

function Test-KeyExtended($Vk) {
  return @(
    0xA3, 0xA5, 0x5B, 0x5C
  ) -contains [int]$Vk
}

function Send-KeyEdge([byte]$Vk, [bool]$KeyUp) {
  $flags = 0
  if (Test-KeyExtended $Vk) {
    $flags = $flags -bor [OpenLessCapsuleProbe]::KEYEVENTF_EXTENDEDKEY
  }
  if ($KeyUp) {
    $flags = $flags -bor [OpenLessCapsuleProbe]::KEYEVENTF_KEYUP
  }
  [OpenLessCapsuleProbe]::keybd_event(
    $Vk,
    [byte](Get-KeyScanCode $Vk),
    $flags,
    [UIntPtr]::Zero
  )
}

function Release-AllModifiers() {
  foreach ($vk in @(0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C)) {
    Send-KeyEdge $vk $true
  }
}

function Get-CapsuleWindowState($ProcessId) {
  $hwnd = [OpenLessCapsuleProbe]::FindVisibleCapsuleWindowForProcess($ProcessId)
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
$env:OPENLESS_HOTKEY_INJECTION_DRY_RUN = "1"
$process = Start-Process -FilePath $ExePath -WorkingDirectory (Split-Path $ExePath -Parent) -PassThru
try {
  if (-not (Wait-LogPattern "hotkey listener installed" $TimeoutSeconds)) {
    throw "Hotkey listener did not install within $TimeoutSeconds seconds."
  }

  Start-Sleep -Milliseconds 500
  $before = Get-CapsuleWindowState $process.Id

  Send-KeyEdge 0xA3 $false
  Start-Sleep -Milliseconds 120
  Send-KeyEdge 0xA3 $true

  $startedDryRun = Wait-LogPattern "session started \(hotkey-injection dry-run\)" $TimeoutSeconds
  Start-Sleep -Milliseconds 400
  $afterStart = Get-CapsuleWindowState $process.Id

  Send-KeyEdge 0xA3 $false
  Start-Sleep -Milliseconds 120
  Send-KeyEdge 0xA3 $true
  Start-Sleep -Seconds 3
  $afterStop = Get-CapsuleWindowState $process.Id

  # Auto/hold semantics depend on the user's persisted mode. If the first short
  # cycle was interpreted as a long hold, it already stopped the first session;
  # the second cycle may therefore have started a new one. Close only that
  # observed extra dry-run session, while still failing on a single-session hide
  # regression instead of masking it with another key press.
  if ($afterStop.Visible -and (Get-LogCount "session started \(hotkey-injection dry-run\)") -gt 1) {
    Send-KeyEdge 0xA3 $false
    Start-Sleep -Milliseconds 120
    Send-KeyEdge 0xA3 $true
    Start-Sleep -Seconds 3
    $afterStop = Get-CapsuleWindowState $process.Id
  }

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
  Release-AllModifiers
  Remove-Item Env:OPENLESS_HOTKEY_INJECTION_DRY_RUN -ErrorAction SilentlyContinue
  if ($null -ne $process) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
  }
}
