param(
  [string]$RepoRoot = (Get-Location).Path,
  [string]$ExePath = "",
  [string]$OutRoot = "",
  [int]$WaitSeconds = 12,
  [int]$ExpectedWidth = 0,
  [int]$ExpectedHeight = 0,
  [int]$SizeTolerance = 2,
  [string]$ExpectedMode = "recording-pill",
  [switch]$TranslationActive,
  [switch]$KeepOpen,
  [switch]$AllowScreenCaptureMiss,
  [switch]$StrictVisualPill
)

$ErrorActionPreference = "Stop"

$dpiBootstrapSource = @'
using System;
using System.Runtime.InteropServices;

public static class OpenLessCapsuleHitlDpi {
  [DllImport("user32.dll")]
  public static extern bool SetProcessDpiAwarenessContext(IntPtr dpiContext);
}
'@

Add-Type -TypeDefinition $dpiBootstrapSource
try {
  [void][OpenLessCapsuleHitlDpi]::SetProcessDpiAwarenessContext([IntPtr]::new(-4))
} catch {
  try {
    [void][OpenLessCapsuleHitlDpi]::SetProcessDpiAwarenessContext([IntPtr]::new(-3))
  } catch {
  }
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$nativeSource = @'
using System;
using System.Text;
using System.Runtime.InteropServices;

public static class OpenLessCapsuleHitlNative {
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

  [StructLayout(LayoutKind.Sequential)]
  public struct POINT { public int X; public int Y; }

  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT point);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr hwnd, uint gaFlags);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdcBlt, uint nFlags);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hWnd);
}
'@

Add-Type -TypeDefinition $nativeSource

function Get-WindowTextValue([IntPtr]$Hwnd) {
  $sb = [Text.StringBuilder]::new(512)
  [void][OpenLessCapsuleHitlNative]::GetWindowText($Hwnd, $sb, $sb.Capacity)
  $sb.ToString()
}

function Get-ClassNameValue([IntPtr]$Hwnd) {
  $sb = [Text.StringBuilder]::new(256)
  [void][OpenLessCapsuleHitlNative]::GetClassName($Hwnd, $sb, $sb.Capacity)
  $sb.ToString()
}

function Get-VisibleWindows {
  $rows = New-Object System.Collections.Generic.List[object]
  $callback = [OpenLessCapsuleHitlNative+EnumWindowsProc]{
    param([IntPtr]$h, [IntPtr]$l)
    if ([OpenLessCapsuleHitlNative]::IsWindowVisible($h)) {
      $rect = [OpenLessCapsuleHitlNative+RECT]::new()
      [void][OpenLessCapsuleHitlNative]::GetWindowRect($h, [ref]$rect)
      $rows.Add([pscustomobject]@{
        hwnd = $h
        title = Get-WindowTextValue $h
        className = Get-ClassNameValue $h
        rect = $rect
      })
    }
    return $true
  }
  [void][OpenLessCapsuleHitlNative]::EnumWindows($callback, [IntPtr]::Zero)
  $rows
}

function Hide-OpenLessMainWindow {
  $callback = [OpenLessCapsuleHitlNative+EnumWindowsProc]{
    param([IntPtr]$h, [IntPtr]$l)
    $title = Get-WindowTextValue $h
    $class = Get-ClassNameValue $h
    if ([OpenLessCapsuleHitlNative]::IsWindowVisible($h) -and $title -eq "OpenLess" -and $class -eq "Tauri Window") {
      [void][OpenLessCapsuleHitlNative]::ShowWindow($h, 0)
    }
    return $true
  }
  [void][OpenLessCapsuleHitlNative]::EnumWindows($callback, [IntPtr]::Zero)
}

function Stop-OpenLess {
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Milliseconds 700
}

function Invoke-ToggleDictation([string]$Path) {
  Start-Process -FilePath $Path -ArgumentList "--toggle-dictation" -WorkingDirectory (Split-Path $Path) | Out-Null
}

function Hit-Point($Name, [int]$X, [int]$Y, [IntPtr]$CapsuleHwnd) {
  $pt = [OpenLessCapsuleHitlNative+POINT]::new()
  $pt.X = $X
  $pt.Y = $Y
  $hwnd = [OpenLessCapsuleHitlNative]::WindowFromPoint($pt)
  $root = [OpenLessCapsuleHitlNative]::GetAncestor($hwnd, 2)
  [pscustomobject]@{
    name = $Name
    point = [pscustomobject]@{ x = $X; y = $Y }
    hwnd = ("0x{0:X}" -f $hwnd.ToInt64())
    title = Get-WindowTextValue $hwnd
    className = Get-ClassNameValue $hwnd
    rootHwnd = ("0x{0:X}" -f $root.ToInt64())
    rootTitle = Get-WindowTextValue $root
    rootClassName = Get-ClassNameValue $root
    isCapsuleRoot = ($root -eq $CapsuleHwnd)
  }
}

function Save-PrintWindowCapture([IntPtr]$Hwnd, [int]$Width, [int]$Height, [string]$Path) {
  $bmp = [Drawing.Bitmap]::new([Math]::Max(1, $Width), [Math]::Max(1, $Height))
  $graphics = [Drawing.Graphics]::FromImage($bmp)
  $hdc = $graphics.GetHdc()
  $ok = $false
  try {
    $ok = [OpenLessCapsuleHitlNative]::PrintWindow($Hwnd, $hdc, 2)
  } finally {
    $graphics.ReleaseHdc($hdc)
    $graphics.Dispose()
  }
  $bmp.Save($Path, [Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  return $ok
}

function Color-Object($Color) {
  [pscustomobject]@{ r = $Color.R; g = $Color.G; b = $Color.B; a = $Color.A }
}

function Is-Green($Color) {
  $Color.R -eq 37 -and $Color.G -eq 200 -and $Color.B -eq 81
}

function Test-ExpectedDimension([int]$Actual, [int]$Expected, [int]$Tolerance) {
  if ($Expected -le 0) { return $true }
  [Math]::Abs($Actual - $Expected) -le $Tolerance
}

function Convert-LogicalToPhysicalDimension([int]$Logical, [double]$DpiScale) {
  if ($Logical -le 0) { return 0 }
  [int][Math]::Round($Logical * $DpiScale)
}

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
  throw "windows-capsule-hitl only runs on Windows."
}

if (-not $ExePath) {
  $defaultExe = "D:\cargo-targets\github.com_appergb_openless\debug\openless.exe"
  if (Test-Path $defaultExe) {
    $ExePath = $defaultExe
  } else {
    throw "Pass -ExePath or build the debug binary at $defaultExe."
  }
}

if (-not (Test-Path $ExePath)) {
  throw "OpenLess exe not found: $ExePath"
}

if (-not $OutRoot) {
  $OutRoot = Join-Path $RepoRoot ".artifacts\windows-capsule-hitl"
}
New-Item -ItemType Directory -Force -Path $OutRoot | Out-Null
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$outDir = Join-Path $OutRoot "capsule-hitl-$stamp"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$form = $null
$screenBmp = $null
$screenGraphics = $null
$cropBmp = $null
$cropGraphics = $null
$oldDryRun = $env:OPENLESS_HOTKEY_INJECTION_DRY_RUN
$oldShowMain = $env:OPENLESS_SHOW_MAIN_ON_START
$oldTranslationFixture = $env:OPENLESS_CAPSULE_HITL_TRANSLATION

try {
  $preExisting = Get-Process openless -ErrorAction SilentlyContinue | Select-Object Id, Path, StartTime
  Stop-OpenLess

  $env:OPENLESS_HOTKEY_INJECTION_DRY_RUN = "1"
  $env:OPENLESS_SHOW_MAIN_ON_START = "0"
  if ($TranslationActive) {
    $env:OPENLESS_CAPSULE_HITL_TRANSLATION = "1"
    if ($ExpectedMode -eq "recording-pill") {
      $ExpectedMode = "translation-pill"
    }
  } else {
    Remove-Item Env:\OPENLESS_CAPSULE_HITL_TRANSLATION -ErrorAction SilentlyContinue
  }

  Start-Process -FilePath $ExePath -WorkingDirectory (Split-Path $ExePath) | Out-Null
  Start-Sleep -Seconds 4
  Hide-OpenLessMainWindow

  $form = [Windows.Forms.Form]::new()
  $form.Text = "OpenLess Hit Test Backdrop"
  $form.FormBorderStyle = [Windows.Forms.FormBorderStyle]::None
  $form.StartPosition = [Windows.Forms.FormStartPosition]::Manual
  $form.Bounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
  $form.BackColor = [Drawing.Color]::FromArgb(37, 200, 81)
  $form.ShowInTaskbar = $false
  $form.TopMost = $false
  $form.Show()
  $form.Activate()
  [Windows.Forms.Application]::DoEvents()
  Start-Sleep -Milliseconds 400

  $triggerAttempts = 1
  $lastTriggerAt = Get-Date
  Invoke-ToggleDictation $ExePath

  $capsuleRow = $null
  $deadline = (Get-Date).AddSeconds($WaitSeconds)
  while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 150
    [Windows.Forms.Application]::DoEvents()
    Hide-OpenLessMainWindow
    $capsuleRow = Get-VisibleWindows | Where-Object {
      $_.title -eq "OpenLess Capsule" -and $_.className -eq "Tauri Window"
    } | Select-Object -First 1
    if ($null -ne $capsuleRow) { break }
    if ($triggerAttempts -lt 3 -and ((Get-Date) - $lastTriggerAt).TotalSeconds -ge 3) {
      $triggerAttempts += 1
      $lastTriggerAt = Get-Date
      Invoke-ToggleDictation $ExePath
    }
  }

  if ($null -eq $capsuleRow) {
    throw "OpenLess Capsule window was not visible within $WaitSeconds seconds."
  }

  $capsule = [IntPtr]$capsuleRow.hwnd
  $rect = [OpenLessCapsuleHitlNative+RECT]::new()
  [void][OpenLessCapsuleHitlNative]::GetWindowRect($capsule, [ref]$rect)
  $width = $rect.Right - $rect.Left
  $height = $rect.Bottom - $rect.Top
  $dpi = [OpenLessCapsuleHitlNative]::GetDpiForWindow($capsule)
  if ($dpi -le 0) { $dpi = 96 }
  $dpiScale = [double]$dpi / 96.0
  $expectedPhysicalWidth = Convert-LogicalToPhysicalDimension $ExpectedWidth $dpiScale
  $expectedPhysicalHeight = Convert-LogicalToPhysicalDimension $ExpectedHeight $dpiScale

  $hits = [ordered]@{
    transparentTopLeft = Hit-Point "transparentTopLeft" ($rect.Left + 5) ($rect.Top + 5) $capsule
    transparentLeftMiddle = Hit-Point "transparentLeftMiddle" ($rect.Left + 5) ($rect.Top + [int]($height / 2)) $capsule
    transparentBottomRight = Hit-Point "transparentBottomRight" ($rect.Right - 5) ($rect.Bottom - 5) $capsule
    pillCenter = Hit-Point "pillCenter" ($rect.Left + [int]($width / 2)) ($rect.Top + [int]($height / 2)) $capsule
  }
  if ($TranslationActive) {
    $badgeCenterY = $rect.Top + [int][Math]::Round(34 * $dpiScale)
    $hits.badgeCenter = Hit-Point "badgeCenter" ($rect.Left + [int]($width / 2)) $badgeCenterY $capsule
  }

  $screenBounds = [Windows.Forms.Screen]::PrimaryScreen.Bounds
  $screenBmp = [Drawing.Bitmap]::new($screenBounds.Width, $screenBounds.Height)
  $screenGraphics = [Drawing.Graphics]::FromImage($screenBmp)
  $screenGraphics.CopyFromScreen($screenBounds.Location, [Drawing.Point]::Empty, $screenBounds.Size)
  $fullPath = Join-Path $outDir "screen-green-bg.png"
  $screenBmp.Save($fullPath, [Drawing.Imaging.ImageFormat]::Png)

  $cropBmp = [Drawing.Bitmap]::new([Math]::Max(1, $width), [Math]::Max(1, $height))
  $cropGraphics = [Drawing.Graphics]::FromImage($cropBmp)
  $cropGraphics.DrawImage(
    $screenBmp,
    0,
    0,
    [Drawing.Rectangle]::new($rect.Left, $rect.Top, [Math]::Max(1, $width), [Math]::Max(1, $height)),
    [Drawing.GraphicsUnit]::Pixel
  )
  $cropPath = Join-Path $outDir "capsule-host-crop.png"
  $cropBmp.Save($cropPath, [Drawing.Imaging.ImageFormat]::Png)

  $printWindowPath = Join-Path $outDir "capsule-printwindow.png"
  $printWindowOk = Save-PrintWindowCapture $capsule $width $height $printWindowPath
  $printWindowSamples = [ordered]@{}
  $printWindowPillCaptured = $false
  $printBmp = $null
  try {
    $printBmp = [Drawing.Bitmap][Drawing.Image]::FromFile($printWindowPath)
    $printWindowSamples.center = $printBmp.GetPixel([int]($printBmp.Width / 2), [int]($printBmp.Height / 2))
    $printWindowSamples.topLeft = $printBmp.GetPixel(5, 5)
    $printCenter = $printWindowSamples.center
    $printWindowPillCaptured = -not (
      ($printCenter.R -eq 0 -and $printCenter.G -eq 0 -and $printCenter.B -eq 0) -or
      (Is-Green $printCenter)
    )
  } finally {
    if ($printBmp) { $printBmp.Dispose() }
  }

  $samples = [ordered]@{
    topLeft = $screenBmp.GetPixel($rect.Left + 5, $rect.Top + 5)
    leftMiddle = $screenBmp.GetPixel($rect.Left + 5, $rect.Top + [int]($height / 2))
    bottomRight = $screenBmp.GetPixel($rect.Right - 5, $rect.Bottom - 5)
    pillCenter = $screenBmp.GetPixel($rect.Left + [int]($width / 2), $rect.Top + [int]($height / 2))
  }
  if ($TranslationActive) {
    $samples.badgeCenter = $screenBmp.GetPixel($hits.badgeCenter.point.x, $hits.badgeCenter.point.y)
  }

  $checks = [ordered]@{
    currentExeWasActive = ((Get-Process openless -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $ExePath }) -ne $null)
    transparentTopLeftPassesThrough = -not $hits.transparentTopLeft.isCapsuleRoot
    transparentLeftMiddlePassesThrough = -not $hits.transparentLeftMiddle.isCapsuleRoot
    transparentBottomRightPassesThrough = -not $hits.transparentBottomRight.isCapsuleRoot
    pillCenterHitsCapsule = $hits.pillCenter.isCapsuleRoot
    badgeCenterHitsCapsule = if ($TranslationActive) { $hits.badgeCenter.isCapsuleRoot } else { $true }
    transparentTopLeftPixelIsGreen = Is-Green $samples.topLeft
    transparentLeftMiddlePixelIsGreen = Is-Green $samples.leftMiddle
    transparentBottomRightPixelIsGreen = Is-Green $samples.bottomRight
    screenPillCaptured = -not (Is-Green $samples.pillCenter)
    screenBadgeCaptured = if ($TranslationActive) { -not (Is-Green $samples.badgeCenter) } else { $true }
    printWindowPillCaptured = $printWindowPillCaptured
    visualPillCaptured = (-not (Is-Green $samples.pillCenter)) -or $printWindowPillCaptured
    printWindowReturnedTrue = [bool]$printWindowOk
    capsuleWidthMatchesExpected = Test-ExpectedDimension $width $expectedPhysicalWidth $SizeTolerance
    capsuleHeightMatchesExpected = Test-ExpectedDimension $height $expectedPhysicalHeight $SizeTolerance
  }
  $checks.capsuleSizeMatchesExpected = $checks.capsuleWidthMatchesExpected -and $checks.capsuleHeightMatchesExpected
  $checks.inputPassed = $checks.currentExeWasActive `
    -and $checks.transparentTopLeftPassesThrough `
    -and $checks.transparentLeftMiddlePassesThrough `
    -and $checks.transparentBottomRightPassesThrough `
    -and $checks.pillCenterHitsCapsule `
    -and $checks.badgeCenterHitsCapsule
  $checks.transparentPixelsPassed = $checks.transparentTopLeftPixelIsGreen `
    -and $checks.transparentLeftMiddlePixelIsGreen `
    -and $checks.transparentBottomRightPixelIsGreen
  $screenCaptureGatePassed = ($checks.screenPillCaptured -and $checks.screenBadgeCaptured) -or $AllowScreenCaptureMiss
  $checks.allPassed = $checks.inputPassed `
    -and $checks.transparentPixelsPassed `
    -and $checks.capsuleSizeMatchesExpected `
    -and $screenCaptureGatePassed `
    -and ((-not $StrictVisualPill) -or $checks.visualPillCaptured)

  $warnings = @()
  if (-not $checks.visualPillCaptured) {
    $warnings += "Screen capture did not include the visible layered/WebView capsule surface at pillCenter; use hit-test plus crop/PrintWindow artifacts for review, and rerun with a different desktop layer if a PR requires visible-pill screenshot evidence."
  }

  $contractGates = @(
    [pscustomobject]@{ id = "process.currentExe"; expected = "openless.exe process path equals ExePath"; observed = $checks.currentExeWasActive; pass = $checks.currentExeWasActive },
    [pscustomobject]@{ id = "input.transparentTopLeft"; expected = "WindowFromPoint root is not capsule"; observed = $hits.transparentTopLeft.rootTitle; pass = $checks.transparentTopLeftPassesThrough },
    [pscustomobject]@{ id = "input.transparentLeftMiddle"; expected = "WindowFromPoint root is not capsule"; observed = $hits.transparentLeftMiddle.rootTitle; pass = $checks.transparentLeftMiddlePassesThrough },
    [pscustomobject]@{ id = "input.transparentBottomRight"; expected = "WindowFromPoint root is not capsule"; observed = $hits.transparentBottomRight.rootTitle; pass = $checks.transparentBottomRightPassesThrough },
    [pscustomobject]@{ id = "input.pillCenter"; expected = "WindowFromPoint root is OpenLess Capsule"; observed = $hits.pillCenter.rootTitle; pass = $checks.pillCenterHitsCapsule },
    [pscustomobject]@{ id = "pixels.transparentTopLeft"; expected = "green backdrop #25C851"; observed = Color-Object $samples.topLeft; pass = $checks.transparentTopLeftPixelIsGreen },
    [pscustomobject]@{ id = "pixels.transparentLeftMiddle"; expected = "green backdrop #25C851"; observed = Color-Object $samples.leftMiddle; pass = $checks.transparentLeftMiddlePixelIsGreen },
    [pscustomobject]@{ id = "pixels.transparentBottomRight"; expected = "green backdrop #25C851"; observed = Color-Object $samples.bottomRight; pass = $checks.transparentBottomRightPixelIsGreen },
    [pscustomobject]@{ id = "capture.screenPill"; expected = "screen crop captures a non-green visible pill center"; observed = Color-Object $samples.pillCenter; pass = $checks.screenPillCaptured }
  )
  if ($TranslationActive) {
    $contractGates += [pscustomobject]@{ id = "input.badgeCenter"; expected = "WindowFromPoint root is OpenLess Capsule"; observed = $hits.badgeCenter.rootTitle; pass = $checks.badgeCenterHitsCapsule }
    $contractGates += [pscustomobject]@{ id = "capture.screenBadge"; expected = "screen crop captures a non-green visible translation badge center"; observed = Color-Object $samples.badgeCenter; pass = $checks.screenBadgeCaptured }
  }
  if ($ExpectedWidth -gt 0) {
    $contractGates += [pscustomobject]@{ id = "shape.width"; expected = "$ExpectedWidth logical px -> $expectedPhysicalWidth physical px +/- $SizeTolerance"; observed = $width; pass = $checks.capsuleWidthMatchesExpected }
  }
  if ($ExpectedHeight -gt 0) {
    $contractGates += [pscustomobject]@{ id = "shape.height"; expected = "$ExpectedHeight logical px -> $expectedPhysicalHeight physical px +/- $SizeTolerance"; observed = $height; pass = $checks.capsuleHeightMatchesExpected }
  }
  if ($StrictVisualPill) {
    $contractGates += [pscustomobject]@{ id = "capture.visiblePill"; expected = "screen or PrintWindow captures a non-green pill center"; observed = $checks.visualPillCaptured; pass = $checks.visualPillCaptured }
  }

  $result = [ordered]@{
    timestamp = (Get-Date).ToString("o")
    repoRoot = $RepoRoot
    exe = $ExePath
    dryRun = $true
    expected = [ordered]@{
      mode = $ExpectedMode
      greenBackdrop = "#25C851"
      capsuleWidth = if ($ExpectedWidth -gt 0) { $ExpectedWidth } else { $null }
      capsuleHeight = if ($ExpectedHeight -gt 0) { $ExpectedHeight } else { $null }
      physicalCapsuleWidth = if ($expectedPhysicalWidth -gt 0) { $expectedPhysicalWidth } else { $null }
      physicalCapsuleHeight = if ($expectedPhysicalHeight -gt 0) { $expectedPhysicalHeight } else { $null }
      sizeTolerance = $SizeTolerance
      strictVisualPill = [bool]$StrictVisualPill
      allowScreenCaptureMiss = [bool]$AllowScreenCaptureMiss
      translationActive = [bool]$TranslationActive
    }
    triggerAttempts = $triggerAttempts
    preExistingOpenLess = $preExisting
    runningAfterStart = Get-Process openless -ErrorAction SilentlyContinue | Select-Object Id, Path, StartTime
    capsuleHwnd = ("0x{0:X}" -f $capsule.ToInt64())
    capsuleTitle = Get-WindowTextValue $capsule
    capsuleClassName = Get-ClassNameValue $capsule
    dpi = $dpi
    dpiScale = $dpiScale
    rect = [pscustomobject]@{ left = $rect.Left; top = $rect.Top; right = $rect.Right; bottom = $rect.Bottom; width = $width; height = $height }
    expectedContract = "transparent host margins pass through; visible pill center hits capsule; transparent pixels show green backdrop"
    hits = $hits
    pixelSamples = [ordered]@{
      topLeft = Color-Object $samples.topLeft
      leftMiddle = Color-Object $samples.leftMiddle
      bottomRight = Color-Object $samples.bottomRight
      pillCenter = Color-Object $samples.pillCenter
      badgeCenter = if ($TranslationActive) { Color-Object $samples.badgeCenter } else { $null }
      printWindowCenter = if ($printWindowSamples.center) { Color-Object $printWindowSamples.center } else { $null }
      printWindowTopLeft = if ($printWindowSamples.topLeft) { Color-Object $printWindowSamples.topLeft } else { $null }
    }
    checks = $checks
    contractGates = $contractGates
    warnings = $warnings
    artifacts = [pscustomobject]@{
      outputDir = $outDir
      fullScreenshot = $fullPath
      cropScreenshot = $cropPath
      printWindowScreenshot = $printWindowPath
    }
  }

  $jsonPath = Join-Path $outDir "hit-test.json"
  $result | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 $jsonPath
  Get-Content $jsonPath

  if (-not $checks.allPassed) {
    exit 2
  }
} catch {
  $jsonPath = Join-Path $outDir "hit-test.json"
  $visibleOpenLessWindows = @()
  try {
    $visibleOpenLessWindows = Get-VisibleWindows | Where-Object {
      $_.title -like "OpenLess*" -or $_.className -eq "Tauri Window"
    } | ForEach-Object {
      [pscustomobject]@{
        hwnd = ("0x{0:X}" -f ([IntPtr]$_.hwnd).ToInt64())
        title = $_.title
        className = $_.className
        rect = [pscustomobject]@{
          left = $_.rect.Left
          top = $_.rect.Top
          right = $_.rect.Right
          bottom = $_.rect.Bottom
          width = $_.rect.Right - $_.rect.Left
          height = $_.rect.Bottom - $_.rect.Top
        }
      }
    }
  } catch {
    $visibleOpenLessWindows = @()
  }

  $failure = [ordered]@{
    timestamp = (Get-Date).ToString("o")
    repoRoot = $RepoRoot
    exe = $ExePath
    dryRun = ($env:OPENLESS_HOTKEY_INJECTION_DRY_RUN -eq "1")
    expected = [ordered]@{
      mode = $ExpectedMode
      greenBackdrop = "#25C851"
      capsuleWidth = if ($ExpectedWidth -gt 0) { $ExpectedWidth } else { $null }
      capsuleHeight = if ($ExpectedHeight -gt 0) { $ExpectedHeight } else { $null }
      physicalCapsuleWidth = $null
      physicalCapsuleHeight = $null
      sizeTolerance = $SizeTolerance
      strictVisualPill = [bool]$StrictVisualPill
      allowScreenCaptureMiss = [bool]$AllowScreenCaptureMiss
      translationActive = [bool]$TranslationActive
    }
    error = $_.Exception.Message
    runningOpenLess = Get-Process openless -ErrorAction SilentlyContinue | Select-Object Id, Path, StartTime
    visibleOpenLessWindows = $visibleOpenLessWindows
    checks = [ordered]@{
      inputPassed = $false
      transparentPixelsPassed = $false
      capsuleSizeMatchesExpected = $false
      allPassed = $false
    }
    contractGates = @(
      [pscustomobject]@{ id = "pipeline.completed"; expected = "runner captures capsule state and writes evidence"; observed = $_.Exception.Message; pass = $false }
    )
    artifacts = [pscustomobject]@{
      outputDir = $outDir
      fullScreenshot = $null
      cropScreenshot = $null
      printWindowScreenshot = $null
    }
  }
  $failure | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 $jsonPath
  Get-Content $jsonPath
  exit 2
} finally {
  if (-not $KeepOpen) {
    Stop-OpenLess
  }
  if ($form) {
    $form.Close()
    $form.Dispose()
  }
  if ($screenGraphics) { $screenGraphics.Dispose() }
  if ($screenBmp) { $screenBmp.Dispose() }
  if ($cropGraphics) { $cropGraphics.Dispose() }
  if ($cropBmp) { $cropBmp.Dispose() }

  if ($null -eq $oldDryRun) { Remove-Item Env:\OPENLESS_HOTKEY_INJECTION_DRY_RUN -ErrorAction SilentlyContinue } else { $env:OPENLESS_HOTKEY_INJECTION_DRY_RUN = $oldDryRun }
  if ($null -eq $oldShowMain) { Remove-Item Env:\OPENLESS_SHOW_MAIN_ON_START -ErrorAction SilentlyContinue } else { $env:OPENLESS_SHOW_MAIN_ON_START = $oldShowMain }
  if ($null -eq $oldTranslationFixture) { Remove-Item Env:\OPENLESS_CAPSULE_HITL_TRANSLATION -ErrorAction SilentlyContinue } else { $env:OPENLESS_CAPSULE_HITL_TRANSLATION = $oldTranslationFixture }
}
