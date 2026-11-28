param(
  [string]$ExePath = "",
  [int]$TimeoutSeconds = 20,
  [int]$VirtualKey = 0xA3,
  [int]$Iterations = 20
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\app")).Path
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

  [DllImport("user32.dll", SetLastError = true)]
  private static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

  [StructLayout(LayoutKind.Sequential)]
  private struct MOUSEINPUT {
    public int dx;
    public int dy;
    public uint mouseData;
    public uint dwFlags;
    public uint time;
    public UIntPtr dwExtraInfo;
  }

  [StructLayout(LayoutKind.Sequential)]
  private struct KEYBDINPUT {
    public ushort wVk;
    public ushort wScan;
    public uint dwFlags;
    public uint time;
    public UIntPtr dwExtraInfo;
  }

  [StructLayout(LayoutKind.Explicit)]
  private struct INPUT_UNION {
    [FieldOffset(0)] public MOUSEINPUT mi;
    [FieldOffset(0)] public KEYBDINPUT ki;
  }

  [StructLayout(LayoutKind.Sequential)]
  private struct INPUT {
    public uint type;
    public INPUT_UNION U;
  }

  public static uint SendInputKey(byte bVk, bool keyUp) {
    var extended = bVk == 0xA3 || bVk == 0xA5 || bVk == 0x5B || bVk == 0x5C;
    var input = new INPUT {
      type = 1,
      U = new INPUT_UNION {
        ki = new KEYBDINPUT {
          wVk = bVk,
          wScan = 0,
          dwFlags = (uint)((keyUp ? KEYEVENTF_KEYUP : 0) | (extended ? KEYEVENTF_EXTENDEDKEY : 0)),
          time = 0,
          dwExtraInfo = UIntPtr.Zero
        }
      }
    };
    return SendInput(1, new[] { input }, Marshal.SizeOf(typeof(INPUT)));
  }

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

function Send-KeyEdge($Vk, $KeyUp, [ValidateSet("keybd_event", "SendInput")] [string]$Method) {
  if ($Method -eq "SendInput") {
    if ([OpenLessInput]::SendInputKey([byte]$Vk, [bool]$KeyUp) -ne 1) {
      throw "SendInput failed for vk=$Vk keyUp=$KeyUp (Win32=$([Runtime.InteropServices.Marshal]::GetLastWin32Error()))."
    }
    return
  }

  $flags = 0
  if (Test-KeyExtended $Vk) {
    $flags = $flags -bor [OpenLessInput]::KEYEVENTF_EXTENDEDKEY
  }
  if ($KeyUp) {
    $flags = $flags -bor [OpenLessInput]::KEYEVENTF_KEYUP
  }
  [OpenLessInput]::keybd_event(
    [byte]$Vk,
    [byte](Get-KeyScanCode $Vk),
    $flags,
    [UIntPtr]::Zero
  )
}

function Release-AllModifiers() {
  foreach ($vk in @(0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C)) {
    # keybd_event 的 key-up 可清理由两种注入 API 设置的系统修饰键状态；
    # 重复 key-up 是幂等的，适合在失败路径兜底。
    Send-KeyEdge $vk $true "keybd_event"
  }
}

function Get-LogCount($Path, $Pattern) {
  if (-not (Test-Path $Path)) {
    return 0
  }
  return ([regex]::Matches((Get-Content -Raw $Path), $Pattern)).Count
}

function Wait-LogCount($Path, $Pattern, $Minimum, $TimeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if ((Get-LogCount $Path $Pattern) -ge $Minimum) {
      return $true
    }
    Start-Sleep -Milliseconds 250
  }
  return $false
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
try {
  Start-Process -FilePath $ExePath -WorkingDirectory (Split-Path $ExePath -Parent) | Out-Null
} finally {
  Remove-Item Env:OPENLESS_SHOW_MAIN_ON_START -ErrorAction SilentlyContinue
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

  $methods = @("keybd_event", "SendInput")
  foreach ($method in $methods) {
    $pressedBefore = Get-LogCount $logPath "\[hotkey\] Windows trigger pressed"
    $releasedBefore = Get-LogCount $logPath "\[hotkey\] Windows trigger released"
    Write-Host "Testing $method with $Iterations complete down/up cycles..."

    for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
      Send-KeyEdge $VirtualKey $false $method
      Start-Sleep -Milliseconds 35
      Send-KeyEdge $VirtualKey $true $method
      Start-Sleep -Milliseconds 35
    }

    if (-not (Wait-LogCount $logPath "\[hotkey\] Windows trigger pressed" ($pressedBefore + $Iterations) $TimeoutSeconds)) {
      throw "$method did not produce $Iterations Windows trigger pressed events."
    }
    if (-not (Wait-LogCount $logPath "\[hotkey\] Windows trigger released" ($releasedBefore + $Iterations) $TimeoutSeconds)) {
      throw "$method did not produce $Iterations Windows trigger released events."
    }
    Write-Host "[ok] $method produced $Iterations complete hotkey cycles."
  }

  if (-not (Wait-LogPattern $logPath "\[coord\] hotkey pressed" $TimeoutSeconds)) {
    throw "Coordinator did not observe OS hook hotkey press."
  }
  Write-Host "[ok] Windows low-level hook accepted keybd_event and SendInput for vk=$VirtualKey."
} finally {
  Release-AllModifiers
  if ($null -ne $notepad) {
    Stop-Process -Id $notepad.Id -Force -ErrorAction SilentlyContinue
  }
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
}

Write-Host "Windows OS hotkey hook smoke passed."
