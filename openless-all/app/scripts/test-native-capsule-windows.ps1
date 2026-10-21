param(
  [Parameter(Mandatory = $true)][string]$HelperPath,
  [Parameter(Mandatory = $true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$HelperPath = (Resolve-Path -LiteralPath $HelperPath).Path
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
[System.IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CapsuleWindowProbe {
  public delegate bool Callback(IntPtr hwnd, IntPtr value);
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback, IntPtr value);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr Style(IntPtr hwnd, int index);
  public static IntPtr Find(int pid, bool console) {
    var result = IntPtr.Zero;
    EnumWindows((hwnd, _) => {
      uint owner; GetWindowThreadProcessId(hwnd, out owner);
      if (owner != pid || !IsWindowVisible(hwnd)) return true;
      var text = new StringBuilder(256);
      if (console) GetClassName(hwnd, text, text.Capacity); else GetWindowText(hwnd, text, text.Capacity);
      if (text.ToString() == (console ? "ConsoleWindowClass" : "OpenLess Capsule")) { result = hwnd; return false; }
      return true;
    }, IntPtr.Zero);
    return result;
  }
}
'@

[CapsuleWindowProbe]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$area = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
$scale = [CapsuleWindowProbe]::GetDpiForSystem() / 96.0
$width = [int][Math]::Round(460 * $scale)
$height = [int][Math]::Round(158 * $scale)
$bounds = @([int]($area.Left + ($area.Width - $width) / 2), [int]($area.Bottom - $height - 12 * $scale), $width, $height)
$results = @()
foreach ($style in @('classic', 'typeless', 'siri')) {
  foreach ($ending in @('shutdown', 'eof', 'idle')) {
    $process = New-Object System.Diagnostics.Process
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $HelperPath
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process.StartInfo = $start
    $started = $false
    try {
      $timer = [System.Diagnostics.Stopwatch]::StartNew()
      if (-not $process.Start()) { throw 'Unable to start capsule helper' }
      $started = $true
      $stderr = $process.StandardError.ReadToEndAsync()
      $ready = $process.StandardOutput.ReadLineAsync()
      if (-not $ready.Wait(5000)) { throw "Ready timeout: $style/$ending" }
      $message = $ready.Result | ConvertFrom-Json
      if ($message.type -ne 'ready' -or $message.version -ne 1) { throw "Invalid handshake: $($ready.Result)" }
      $readyMs = $timer.ElapsedMilliseconds
      $frame = @{
        type = 'frame'; version = 1
        frame = @{
          session_id = "windows-smoke-$style-$ending"; sequence = 1; interactive = $true
          state = @{ phase = 'recording'; text = ''; audio_level = 0.45; translation_active = $false; style = $style }
          transcript = '实时转写 smoke test'; transcript_font_size = 14; bounds = $bounds
        }
      }
      $process.StandardInput.WriteLine(($frame | ConvertTo-Json -Depth 6 -Compress))
      $process.StandardInput.Flush()
      $visible = [System.Diagnostics.Stopwatch]::StartNew()
      $window = [IntPtr]::Zero
      while ($visible.ElapsedMilliseconds -lt 5000 -and $window -eq [IntPtr]::Zero -and -not $process.HasExited) {
        Start-Sleep -Milliseconds 10
        $window = [CapsuleWindowProbe]::Find($process.Id, $false)
      }
      if ($window -eq [IntPtr]::Zero) {
        if ($process.HasExited) { throw "Helper exited $($process.ExitCode): $($stderr.Result)" }
        throw "No visible capsule: $style/$ending"
      }
      $visibleMs = $visible.ElapsedMilliseconds
      if ([CapsuleWindowProbe]::GetForegroundWindow() -eq $window) { throw 'Capsule stole foreground focus' }
      if ([CapsuleWindowProbe]::Find($process.Id, $true) -ne [IntPtr]::Zero) { throw 'Visible helper console' }
      $extended = [CapsuleWindowProbe]::Style($window, -20).ToInt64()
      if (($extended -band 0x08000000) -eq 0 -or ($extended -band 0x80) -eq 0) { throw ('Missing no-activate/tool-window style: 0x{0:X}' -f $extended) }
      Start-Sleep -Milliseconds 250
      if ($ending -eq 'shutdown') {
        $rect = New-Object CapsuleWindowProbe+Rect
        if (-not [CapsuleWindowProbe]::GetWindowRect($window, [ref]$rect)) { throw 'Cannot inspect capsule rectangle' }
        $bitmap = New-Object System.Drawing.Bitmap(($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top))
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
          $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
          $bitmap.Save((Join-Path $OutputDirectory "$style.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
      }
      $process.Refresh()
      $privateMiB = [Math]::Round($process.PrivateMemorySize64 / 1MB, 2)
      $workingMiB = [Math]::Round($process.WorkingSet64 / 1MB, 2)
      $exitTimer = [System.Diagnostics.Stopwatch]::StartNew()
      switch ($ending) {
        'shutdown' { $process.StandardInput.WriteLine('{"type":"shutdown","version":1}'); $process.StandardInput.Flush() }
        'eof' { $process.StandardInput.Close() }
        'idle' { $frame.frame.state.phase = 'idle'; $frame.frame.sequence = 2; $process.StandardInput.WriteLine(($frame | ConvertTo-Json -Depth 6 -Compress)); $process.StandardInput.Flush() }
      }
      if (-not $process.WaitForExit(3000)) { throw "Helper remained after $ending" }
      if ($process.ExitCode -ne 0) { throw "Helper exit $($process.ExitCode): $($stderr.Result)" }
      $results += [pscustomobject]@{ style = $style; ending = $ending; readyMs = $readyMs; visibleAfterFrameMs = $visibleMs; exitMs = $exitTimer.ElapsedMilliseconds; privateMiB = $privateMiB; workingSetMiB = $workingMiB }
    } finally {
      # Only reap the helper started by this iteration; never stop a user's OpenLess instance.
      if ($started -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
      if ($started) {
        $stderr.Wait(2000) | Out-Null
        if ($stderr.IsCompleted) { $stderr.Result | Set-Content -LiteralPath (Join-Path $OutputDirectory "$style-$ending.stderr.txt") }
      }
      $process.Dispose()
    }
  }
}
$results | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutputDirectory 'results.json') -Encoding UTF8
$results | Format-Table -AutoSize
Write-Output 'PASS: 9 native capsule launches, non-activating windows and shutdown/EOF/Idle cleanup.'
