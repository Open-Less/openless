param(
  [string]$ExePath = "",
  [string]$Phrase = "OpenLess Windows regression suite phrase. OpenLess Windows regression suite phrase.",
  [ValidateSet("notepad", "browser")]
  [string[]]$Targets = @("notepad", "browser"),
  [switch]$Build,
  [switch]$SkipRuntime,
  [switch]$SkipHotkey,
  [switch]$SkipRealAsr,
  [switch]$SkipPrivacy,
  [switch]$DebugHotkeyEvents
)

$ErrorActionPreference = "Stop"

$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $ExePath = Join-Path $appRoot ".artifacts\windows-gnu\dev\openless.exe"
}

if (-not $env:SystemDrive) {
  $env:SystemDrive = "C:"
}
if (-not $env:ProgramData) {
  $env:ProgramData = Join-Path $env:SystemDrive "ProgramData"
}

function Invoke-Step($Name, [scriptblock]$Block) {
  Write-Host ""
  Write-Host "== $Name =="
  $start = Get-Date
  try {
    & $Block
    $elapsed = [int]((Get-Date) - $start).TotalSeconds
    Write-Host "[ok] $Name (${elapsed}s)"
  } catch {
    $elapsed = [int]((Get-Date) - $start).TotalSeconds
    Write-Host "[fail] $Name (${elapsed}s)"
    throw
  }
}

function Invoke-Script($Path, [hashtable]$Parameters = @{}) {
  $resolved = (Resolve-Path $Path).Path
  & $resolved @Parameters
}

function Test-PowerShellSyntax($Path) {
  $tokens = $null
  $errors = $null
  [System.Management.Automation.Language.Parser]::ParseFile(
    (Resolve-Path $Path).Path,
    [ref]$tokens,
    [ref]$errors
  ) | Out-Null
  if ($errors.Count) {
    throw ($errors | Out-String)
  }
}

$scriptsToParse = @(
  "windows-runtime-smoke.ps1",
  "windows-hotkey-os-hook-smoke.ps1",
  "windows-real-asr-insertion-smoke.ps1",
  "windows-microphone-privacy-smoke.ps1"
)

Write-Host "OpenLess Windows smoke suite"
Write-Host "appRoot=$appRoot"
Write-Host "exe=$ExePath"

try {
  Invoke-Step "PowerShell syntax" {
    foreach ($script in $scriptsToParse) {
      Test-PowerShellSyntax (Join-Path $PSScriptRoot $script)
    }
  }

  if ($Build) {
    Invoke-Step "Windows GNU build" {
      Invoke-Script (Join-Path $PSScriptRoot "windows-build-gnu.ps1")
    }
  }

  if (-not (Test-Path $ExePath)) {
    throw "OpenLess executable not found: $ExePath. Run with -Build or run scripts/windows-build-gnu.ps1 first."
  }

  if (-not $SkipRuntime) {
    Invoke-Step "Runtime smoke" {
      Invoke-Script (Join-Path $PSScriptRoot "windows-runtime-smoke.ps1") @{
        ExePath = $ExePath
        RequireCredentials = $true
      }
    }
  }

  if (-not $SkipHotkey) {
    Invoke-Step "OS hotkey smoke" {
      Invoke-Script (Join-Path $PSScriptRoot "windows-hotkey-os-hook-smoke.ps1") @{
        ExePath = $ExePath
      }
    }
  }

  if (-not $SkipRealAsr) {
    foreach ($target in $Targets) {
      Invoke-Step "Real ASR insertion fallback: $target" {
        $parameters = @{
          ExePath = $ExePath
          Target = $target
          Phrase = $Phrase
        }
        if ($DebugHotkeyEvents) {
          $parameters.DebugHotkeyEvents = $true
        }
        Invoke-Script (Join-Path $PSScriptRoot "windows-real-asr-insertion-smoke.ps1") $parameters
      }
    }
  }

  if (-not $SkipPrivacy) {
    Invoke-Step "Microphone privacy deny/restore" {
      Invoke-Script (Join-Path $PSScriptRoot "windows-microphone-privacy-smoke.ps1") @{
        ExePath = $ExePath
      }
    }
  }
} finally {
  Get-Process openless -ErrorAction SilentlyContinue | Stop-Process -Force
}

Write-Host ""
Write-Host "Windows smoke suite passed."
