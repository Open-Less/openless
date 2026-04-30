param(
  [ValidateSet("all", "msvc", "gnu")]
  [string]$Toolchain = "all"
)

$ErrorActionPreference = "Stop"

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\scoop\persist\rustup\.cargo\bin;$env:USERPROFILE\scoop\apps\rustup\current\.cargo\bin;$env:USERPROFILE\scoop\apps\mingw\current\bin;$env:PATH"

function Test-Command($Name) {
  $cmd = Get-Command $Name -ErrorAction SilentlyContinue
  if ($cmd) {
    Write-Host "[ok] $Name -> $($cmd.Source)"
    return $true
  }
  Write-Host "[missing] $Name"
  return $false
}

function Find-Kernel32Lib {
  $kitsRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\Lib"
  if (-not (Test-Path $kitsRoot)) {
    return $null
  }
  Get-ChildItem -LiteralPath $kitsRoot -Directory |
    Sort-Object Name -Descending |
    ForEach-Object {
      $candidate = Join-Path $_.FullName "um\x64\kernel32.lib"
      if (Test-Path $candidate) {
        return $candidate
      }
    }
}

function Test-WebView2Runtime {
  $paths = @(
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F1E7FBD4-9C4C-41A4-AB01-7C0F7A947F1A}",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F1E7FBD4-9C4C-41A4-AB01-7C0F7A947F1A}"
  )
  foreach ($path in $paths) {
    if (Test-Path $path) {
      Write-Host "[ok] WebView2 Runtime registry key found"
      return $true
    }
  }
  Write-Host "[warn] WebView2 Runtime registry key not found; install Evergreen runtime if the app window is blank."
  return $false
}

$failed = $false

Write-Host "== Common prerequisites =="
foreach ($name in @("node", "npm", "rustc", "cargo", "rustup")) {
  if (-not (Test-Command $name)) {
    $failed = $true
  }
}
Test-WebView2Runtime | Out-Null

if ($Toolchain -eq "all" -or $Toolchain -eq "msvc") {
  Write-Host ""
  Write-Host "== MSVC route =="
  if (-not (Test-Command "link.exe")) {
    Write-Host "[hint] Run from a Developer PowerShell, or call vcvars64.bat first."
    $failed = $true
  }
  $kernel32 = Find-Kernel32Lib
  if ($kernel32) {
    Write-Host "[ok] kernel32.lib -> $kernel32"
  } else {
    Write-Host "[missing] kernel32.lib"
    Write-Host "[hint] Install Visual Studio Build Tools workload 'Desktop development with C++' and a Windows 10/11 SDK."
    $failed = $true
  }
}

if ($Toolchain -eq "all" -or $Toolchain -eq "gnu") {
  Write-Host ""
  Write-Host "== GNU/MinGW route =="
  foreach ($name in @("gcc", "dlltool")) {
    if (-not (Test-Command $name)) {
      $failed = $true
    }
  }
  if (Get-Command rustup -ErrorAction SilentlyContinue) {
    $toolchains = & rustup toolchain list 2>$null
    if ($toolchains -match "x86_64-pc-windows-gnu") {
      Write-Host "[ok] Rust GNU toolchain installed"
    } else {
      Write-Host "[missing] stable-x86_64-pc-windows-gnu"
      Write-Host "[hint] rustup toolchain install stable-x86_64-pc-windows-gnu"
      $failed = $true
    }
  } else {
    Write-Host "[missing] rustup"
    $failed = $true
  }
  if ((Get-Location).Path -match "\s") {
    Write-Host "[warn] Current path contains spaces. Use scripts/windows-build-gnu.ps1 or build from a no-space path."
  }
}

if ($failed) {
  exit 1
}

Write-Host ""
Write-Host "Preflight passed."
