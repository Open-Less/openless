param(
  [int]$DurationSeconds = 20
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class OpenLessCapsuleWatch {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern IntPtr FindWindowW(string lpClassName, string lpWindowName);

  [DllImport("user32.dll")]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool IsWindowVisible(IntPtr hWnd);
}
"@

function Get-CapsuleState {
  $hwnd = [OpenLessCapsuleWatch]::FindWindowW($null, "OpenLess Capsule")
  if ($hwnd -eq [IntPtr]::Zero) {
    return "missing"
  }
  if ([OpenLessCapsuleWatch]::IsWindowVisible($hwnd)) {
    return "visible"
  }
  return "hidden"
}

Write-Host "== Windows capsule watch =="
Write-Host "Watch duration: $DurationSeconds seconds"
Write-Host "Please trigger dictation start/stop now."

$deadline = (Get-Date).AddSeconds($DurationSeconds)
$last = ""
while ((Get-Date) -lt $deadline) {
  $state = Get-CapsuleState
  if ($state -ne $last) {
    Write-Host ("[{0:HH:mm:ss.fff}] capsule={1}" -f (Get-Date), $state)
    $last = $state
  }
  Start-Sleep -Milliseconds 100
}

Write-Host "== Final capsule state: $last =="
