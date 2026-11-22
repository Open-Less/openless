param([Parameter(Mandatory=$true)][string]$InstallerPath,
      [Parameter(Mandatory=$true)][string]$EvidenceDirectory)
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true') { throw 'CI only; do not install on a user host.' }
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
New-Item -ItemType Directory -Force $EvidenceDirectory | Out-Null
$expectedVersion = '2.0.3-Beta.2+build.noask.20261123'
$result = @{ status='incomplete'; runtime_sha='not_probed' }
$timeline = [System.Collections.Generic.List[string]]::new()
$started = Get-Date
$app = $null
$installRoot = $null
$stdout = $null
$stderr = $null
function Record([string]$Text) {
  $line = '{0:N2}s {1}' -f ((Get-Date)-$started).TotalSeconds, $Text
  $timeline.Add($line)
  Write-Host $line
}
function Get-Tree {
  $condition = [System.Windows.Automation.PropertyCondition]::new(
    [System.Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$app.Id)
  $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
    [System.Windows.Automation.TreeScope]::Children, $condition)
  $nodes = @()
  foreach ($window in $windows) {
    $nodes += $window
    $nodes += @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                               [System.Windows.Automation.Condition]::TrueCondition))
  }
  return $nodes
}
function Dump([string]$Name, $Nodes) {
  @($Nodes | ForEach-Object {
    '{0} enabled={1} offscreen={2} {3}' -f $_.Current.ControlType.ProgrammaticName,
      $_.Current.IsEnabled, $_.Current.IsOffscreen, $_.Current.Name
  }) | Set-Content (Join-Path $EvidenceDirectory "$Name-uia.txt")
}
function Capture([string]$Name) {
  $window = @(Get-Tree | Where-Object {
    $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window -and
    $_.Current.BoundingRectangle.Width -ge 700 -and $_.Current.BoundingRectangle.Height -ge 480
  } | Select-Object -First 1)
  if ($window.Count -ne 1) { throw 'Main application window not available for capture.' }
  $rect = $window[0].Current.BoundingRectangle
  $bitmap = [System.Drawing.Bitmap]::new([int]$rect.Width, [int]$rect.Height)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $graphics.CopyFromScreen([int]$rect.X, [int]$rect.Y, 0, 0, $bitmap.Size)
    $bitmap.Save((Join-Path $EvidenceDirectory "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    Record "screenshot=$Name"
  } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
function Find-Button($Nodes, [string[]]$Labels) {
  return $Nodes | Where-Object {
    $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and
    $_.Current.Name -in $Labels -and -not $_.Current.IsOffscreen
  } | Select-Object -First 1
}
function Wait-Content([string]$Name, [scriptblock]$Predicate) {
  $deadline = (Get-Date).AddSeconds(40)
  while ((Get-Date) -lt $deadline) {
    if ($app.HasExited) { throw "App exited with $($app.ExitCode)" }
    $nodes = @(Get-Tree)
    if (& $Predicate $nodes) { Record "ready=$Name"; Dump $Name $nodes; return $nodes }
    Start-Sleep -Milliseconds 500
  }
  throw "Content/control not ready: $Name"
}
function Press($Node, [string]$Name) {
  if ($null -eq $Node) { throw "Button missing: $Name" }
  if (-not $Node.Current.IsEnabled) { throw "Button disabled: $Name" }
  $pattern = $Node.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
  $pattern.Invoke()
  Record "InvokePattern=$Name"
}
try {
  # Reuse the release's existing install/registration verifier; no custom install procedure.
  & (Join-Path $PSScriptRoot 'windows-ime-install-smoke.ps1') -InstallerPath $InstallerPath -InstallerKind nsis -SkipUninstall
  $dll = (Get-ItemProperty 'Registry::HKEY_LOCAL_MACHINE\Software\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32').'(default)'
  $installRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $dll))
  $exe = Join-Path $installRoot 'openless.exe'
  $result.installed_executable = $exe
  $result.file_product_version = (Get-Item $exe).VersionInfo.ProductVersion
  $info = [System.Diagnostics.ProcessStartInfo]::new($exe)
  $info.WorkingDirectory = $installRoot
  $info.UseShellExecute = $false
  $info.RedirectStandardOutput = $true
  $info.RedirectStandardError = $true
  $originalEnvironment = @{}
  foreach ($key in @('SystemRoot','SystemDrive','WINDIR','PATH','TEMP','TMP','USERPROFILE',
      'APPDATA','LOCALAPPDATA','PROGRAMDATA','PROGRAMFILES','PROGRAMFILES(X86)',
      'COMMONPROGRAMFILES','COMMONPROGRAMFILES(X86)','COMSPEC','HOMEDRIVE','HOMEPATH','USERNAME')) {
    $value = [Environment]::GetEnvironmentVariable($key)
    if ($null -ne $value) { $originalEnvironment[$key] = $value }
  }
  $info.EnvironmentVariables.Clear()
  foreach ($key in $originalEnvironment.Keys) { $info.EnvironmentVariables[$key] = $originalEnvironment[$key] }
  $app = [System.Diagnostics.Process]::Start($info)
  $stdout = $app.StandardOutput.ReadToEndAsync()
  $stderr = $app.StandardError.ReadToEndAsync()
  Record "spawned pid=$($app.Id)"
  $nodes = Wait-Content 'initialized-onboarding' { param($n) $null -ne (Find-Button $n @('Explore first','先去看看')) }
  Capture 'initialized-onboarding'
  $result.content_initialized = 'onboarding_controls_accessible'
  Press (Find-Button $nodes @('Explore first','先去看看')) 'Explore first'
  $nodes = Wait-Content 'main-overview' { param($n) $null -ne (Find-Button $n @('Settings','设置')) }
  Capture 'main-overview'
  Press (Find-Button $nodes @('Tools','工具')) 'Tools'
  $nodes = Wait-Content 'ask-visible' { param($n) $null -ne (Find-Button $n @('Ask','随便问')) }
  $ask = Find-Button $nodes @('Ask','随便问')
  if ($ask.Current.IsEnabled) { throw 'Ask is enabled in noask build.' }
  $result.ask = 'UIA_IsEnabled=false'
  Capture 'main-ask-disabled'
  Press (Find-Button $nodes @('Settings','设置')) 'Settings'
  $nodes = Wait-Content 'settings' { param($n) $null -ne (Find-Button $n @('About & updates','关于与更新')) }
  Press (Find-Button $nodes @('About & updates','关于与更新')) 'About & updates'
  $nodes = Wait-Content 'about-version' { param($n) @($n | Where-Object { $_.Current.Name.Contains($expectedVersion) }).Count -gt 0 }
  Capture 'about-version'
  $result.ui_version = $expectedVersion
  $result.navigation = 'onboarding_to_main_to_settings_about'
  $result.status = 'PASS'
} catch {
  $result.status = 'BLOCKED'
  $result.reason = $_.Exception.Message
  Write-Host "Acceptance incomplete: $($result.reason)"
} finally {
  if ($null -ne $app) {
    $result.process_before_cleanup = if ($app.HasExited) { 'exited' } else { 'running' }
    try { Dump 'final' @(Get-Tree); Capture 'final' } catch { Record "Final capture unavailable: $($_.Exception.Message)" }
    if (-not $app.HasExited) { $app.Kill(); $app.WaitForExit(5000) | Out-Null }
    if ($null -ne $stdout -and $stdout.Wait(5000)) { $stdout.Result | Set-Content (Join-Path $EvidenceDirectory 'stdout.log') }
    if ($null -ne $stderr -and $stderr.Wait(5000)) { $stderr.Result | Set-Content (Join-Path $EvidenceDirectory 'stderr.log') }
  }
  if ($null -ne $installRoot) {
    $uninstaller = Join-Path $installRoot 'uninstall.exe'
    $uninstall = Start-Process $uninstaller -ArgumentList '/S' -Wait -PassThru
    $result.uninstall_exit = $uninstall.ExitCode
    if ($uninstall.ExitCode -ne 0) { $result.status = 'BLOCKED' }
  }
  $timeline | Set-Content (Join-Path $EvidenceDirectory 'timeline.txt')
  $result | ConvertTo-Json | Set-Content (Join-Path $EvidenceDirectory 'result.json')
  $result | Format-List | Out-String | Write-Host
}
if ($result.status -ne 'PASS') { exit 1 }
