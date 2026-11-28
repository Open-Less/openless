param(
  [ValidateSet("notepad", "powershell", "cmd", "wt")]
  [string]$Target = "powershell",
  [int]$RestoreDelayMs = 150,
  [int]$PasteSettleDelayMs = 900
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class ClipboardRestoreSmokeWin32 {
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

function Send-KeyEdge($Vk, $KeyUp, $Extended = $false) {
  $flags = 0
  if ($Extended) {
    $flags = $flags -bor [ClipboardRestoreSmokeWin32]::KEYEVENTF_EXTENDEDKEY
  }
  if ($KeyUp) {
    $flags = $flags -bor [ClipboardRestoreSmokeWin32]::KEYEVENTF_KEYUP
  }
  $scanCode = if ($Vk -eq 0xA2 -or $Vk -eq 0xA3) { 0x1D } else { 0 }
  [ClipboardRestoreSmokeWin32]::keybd_event([byte]$Vk, [byte]$scanCode, $flags, [UIntPtr]::Zero)
}

function Send-CtrlChord($Vk) {
  Send-KeyEdge 0xA2 $false $false
  Start-Sleep -Milliseconds 70
  Send-KeyEdge $Vk $false $false
  Start-Sleep -Milliseconds 70
  Send-KeyEdge $Vk $true $false
  Start-Sleep -Milliseconds 70
  Send-KeyEdge 0xA2 $true $false
}

function Send-EnterKey {
  Send-KeyEdge 0x0D $false $false
  Start-Sleep -Milliseconds 70
  Send-KeyEdge 0x0D $true $false
}

function Wait-ProcessWindow($ProcessName, $After, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    $candidate = Get-Process $ProcessName -ErrorAction SilentlyContinue |
      Where-Object { $_.StartTime -ge $After -and $_.MainWindowHandle -ne 0 } |
      Sort-Object StartTime -Descending |
      Select-Object -First 1
    if ($null -ne $candidate) {
      return $candidate
    }
    Start-Sleep -Milliseconds 300
  }
  return $null
}

function Focus-Window($Process) {
  if ($null -eq $Process -or $Process.MainWindowHandle -eq 0) {
    throw "Target window is unavailable."
  }
  [ClipboardRestoreSmokeWin32]::ShowWindow($Process.MainWindowHandle, 9) | Out-Null
  [ClipboardRestoreSmokeWin32]::SetForegroundWindow($Process.MainWindowHandle) | Out-Null
  Start-Sleep -Milliseconds 500
}

function Start-TargetWindow($TargetName) {
  $startedAt = Get-Date
  switch ($TargetName) {
    "notepad" {
      Start-Process notepad.exe | Out-Null
      return Wait-ProcessWindow "notepad" $startedAt 15
    }
    "powershell" {
      Start-Process powershell.exe -ArgumentList "-NoLogo" | Out-Null
      return Wait-ProcessWindow "powershell" $startedAt 15
    }
    "cmd" {
      Start-Process cmd.exe | Out-Null
      return Wait-ProcessWindow "cmd" $startedAt 15
    }
    "wt" {
      $wt = Get-Command wt.exe -ErrorAction SilentlyContinue
      if ($null -eq $wt) {
        throw "wt.exe was not found."
      }
      Start-Process $wt.Source -ArgumentList "new-tab", "powershell.exe", "-NoLogo" | Out-Null
      return Wait-ProcessWindow "WindowsTerminal" $startedAt 20
    }
  }
}

function Wait-FileText($Path, $Expected, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if (Test-Path $Path) {
      $content = Get-Content -Raw -Encoding UTF8 $Path
      if ($content.Contains($Expected)) {
        return $true
      }
    }
    Start-Sleep -Milliseconds 200
  }
  return $false
}

function Restore-ClipboardValue($Value) {
  if ($null -eq $Value) {
    cmd /c "echo off | clip" | Out-Null
    return
  }
  Set-Clipboard -Value $Value
}

$marker = "OPENLESS_CLIPBOARD_RESTORE_OK"
$outputPath = Join-Path $env:TEMP "openless-clipboard-restore-$Target.txt"
Remove-Item -LiteralPath $outputPath -Force -ErrorAction SilentlyContinue
$previousClipboard = Get-Clipboard -Raw -ErrorAction SilentlyContinue

switch ($Target) {
  "notepad" {
    $payload = $marker
  }
  "cmd" {
    $payload = "echo $marker > `"$outputPath`""
  }
  default {
    $payload = "Set-Content -Path `"$outputPath`" -Value `"$marker`""
  }
}

$targetProcess = $null
try {
  $targetProcess = Start-TargetWindow $Target
  if ($null -eq $targetProcess) {
    throw "Failed to start target window: $Target"
  }
  Focus-Window $targetProcess
  Set-Clipboard -Value $payload
  Start-Sleep -Milliseconds 150

  Send-CtrlChord 0x56
  Start-Sleep -Milliseconds $RestoreDelayMs
  Restore-ClipboardValue $previousClipboard

  if ($Target -eq "notepad") {
    Start-Sleep -Milliseconds $PasteSettleDelayMs
    Send-CtrlChord 0x41
    Start-Sleep -Milliseconds 120
    Send-CtrlChord 0x43
    Start-Sleep -Milliseconds 200
    $result = Get-Clipboard -Raw -ErrorAction SilentlyContinue
    if (-not $result.Contains($marker)) {
      throw "Notepad readback did not contain the pasted marker."
    }
  } else {
    Start-Sleep -Milliseconds 120
    Send-EnterKey
    if (-not (Wait-FileText $outputPath $marker 6)) {
      throw "Terminal target did not execute the pasted command before clipboard restore."
    }
  }

  Write-Host "[ok] target=$Target restoreDelayMs=$RestoreDelayMs"
} finally {
  Restore-ClipboardValue $previousClipboard
  if ($null -ne $targetProcess) {
    Stop-Process -Id $targetProcess.Id -Force -ErrorAction SilentlyContinue
  }
}
