param(
  [ValidateSet("Debug", "Release")]
  [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"

function Get-Regsvr32ForPlatform {
  param(
    [ValidateSet("x64", "Win32")]
    [string]$Platform
  )

  if ($Platform -eq "Win32") {
    $syswow64 = Join-Path $env:WINDIR "SysWOW64\regsvr32.exe"
    if (Test-Path $syswow64) {
      return $syswow64
    }
    return (Join-Path $env:WINDIR "System32\regsvr32.exe")
  }

  $sysnative = Join-Path $env:WINDIR "Sysnative\regsvr32.exe"
  if (Test-Path $sysnative) {
    return $sysnative
  }

  return (Join-Path $env:WINDIR "System32\regsvr32.exe")
}

function Test-IsAdministrator {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = [Security.Principal.WindowsPrincipal]::new($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\app")).Path
$registrationRoot = Join-Path $appRoot "src-tauri\target\windows-ime-register"
$registrationManifest = Join-Path $registrationRoot "active-registration.json"

function Get-ManifestDllPath {
  param(
    [ValidateSet("x64", "Win32")]
    [string]$Platform
  )

  if (-not (Test-Path $registrationManifest)) {
    return $null
  }

  try {
    $manifest = Get-Content -Raw -Path $registrationManifest | ConvertFrom-Json
    $property = $manifest.PSObject.Properties[$Platform]
    if ($null -ne $property -and -not [string]::IsNullOrWhiteSpace($property.Value)) {
      return $property.Value
    }
  } catch {
    Write-Host "[warn] Failed to read OpenLess IME registration manifest: $($_.Exception.Message)"
  }

  return $null
}

function Get-LatestStagedDllPath {
  param(
    [ValidateSet("x64", "Win32")]
    [string]$Platform
  )

  if (-not (Test-Path $registrationRoot)) {
    return $null
  }

  $folder = if ($Platform -eq "Win32") { "x86" } else { $Platform }
  $dll = Get-ChildItem -Path $registrationRoot -Filter OpenLessIme.dll -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -like "*\$folder\$Configuration\OpenLessIme.dll" } |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1

  if ($null -ne $dll) {
    return $dll.FullName
  }

  return $null
}

function Get-LegacyDllPath {
  param(
    [ValidateSet("x64", "Win32")]
    [string]$Platform
  )

  $legacyFolder = if ($Platform -eq "Win32") { "Win32" } else { $Platform }
  return Join-Path $appRoot "..\windows\ime\$legacyFolder\$Configuration\OpenLessIme.dll"
}

function Get-DllPath {
  param(
    [ValidateSet("x64", "Win32")]
    [string]$Platform
  )

  $manifestDll = Get-ManifestDllPath $Platform
  if ($null -ne $manifestDll) {
    return $manifestDll
  }

  $stagedDll = Get-LatestStagedDllPath $Platform
  if ($null -ne $stagedDll) {
    return $stagedDll
  }

  return Get-LegacyDllPath $Platform
}

if (-not (Test-IsAdministrator)) {
  throw "Unregistering the OpenLess TSF IME requires an elevated Administrator PowerShell."
}

foreach ($platform in @("x64", "Win32")) {
  $dll = Get-DllPath $platform
  if (-not (Test-Path $dll)) {
    Write-Host "[skip] OpenLessIme.dll not found ($platform): $dll"
    continue
  }

  $regsvr32 = Get-Regsvr32ForPlatform $platform
  $process = Start-Process -FilePath $regsvr32 -ArgumentList @("/u", "/s", $dll) -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    throw "$platform regsvr32 /u failed with exit code $($process.ExitCode)"
  }
  Write-Host "[ok] OpenLess TSF IME unregistered ($platform)"
}

if (Test-Path $registrationManifest) {
  Remove-Item -LiteralPath $registrationManifest -Force
}
