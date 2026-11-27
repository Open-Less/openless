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
$stagingStamp = "{0:yyyyMMddHHmmss}-{1}" -f (Get-Date), $PID
$stagingRoot = Join-Path $registrationRoot $stagingStamp
$registrationManifest = Join-Path $registrationRoot "active-registration.json"
$registeredDlls = [ordered]@{}

function Save-RegistrationManifest {
  New-Item -ItemType Directory -Path $registrationRoot -Force | Out-Null
  $registeredDlls | ConvertTo-Json | Set-Content -Path $registrationManifest -Encoding UTF8
}

function Get-DllPath {
  param(
    [ValidateSet("x64", "Win32")]
    [string]$Platform
  )

  $folder = if ($Platform -eq "Win32") { "x86" } else { $Platform }
  return Join-Path $stagingRoot "$folder\$Configuration\OpenLessIme.dll"
}

function Get-IntermediateDirectory {
  param(
    [ValidateSet("x64", "Win32")]
    [string]$Platform
  )

  $folder = if ($Platform -eq "Win32") { "x86" } else { $Platform }
  return Join-Path $stagingRoot "obj\$folder\$Configuration"
}

if (-not (Test-IsAdministrator)) {
  throw "Registering the OpenLess TSF IME requires an elevated Administrator PowerShell."
}

foreach ($platform in @("x64", "Win32")) {
  $dll = Get-DllPath $platform
  & (Join-Path $PSScriptRoot "windows-ime-build.ps1") `
    -Configuration $Configuration `
    -Platform $platform `
    -OutputDirectory (Split-Path $dll -Parent) `
    -IntermediateDirectory (Get-IntermediateDirectory $platform)

  $regsvr32 = Get-Regsvr32ForPlatform $platform
  $process = Start-Process -FilePath $regsvr32 -ArgumentList @("/s", $dll) -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    throw "$platform regsvr32 failed with exit code $($process.ExitCode)"
  }
  $registeredDlls[$platform] = $dll
  Save-RegistrationManifest
  Write-Host "[ok] OpenLess TSF IME registered ($platform)"
}
