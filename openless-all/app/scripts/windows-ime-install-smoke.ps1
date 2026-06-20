param(
  [Parameter(Mandatory = $true)]
  [string]$InstallerPath,
  [Parameter(Mandatory = $true)]
  [ValidateSet("nsis", "msi")]
  [string]$InstallerKind,
  [switch]$SkipUninstall
)

$ErrorActionPreference = "Stop"

$TextServiceClsid = "{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}"
$ProfileGuid = "{9B5F5E04-23F6-47DA-9A26-D221F6C3F02E}"
$LangId = "0x00000804"
$KeyboardCategoryGuid = "{34745C63-B2F0-4784-8B67-5E12C8701A31}"
$ImmersiveCategoryGuid = "{13A016DF-560B-46CD-947A-4C3AF1E0E35D}"
$SystrayCategoryGuid = "{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}"

# Keep this script aligned with the default Windows backend: OpenLess ships the
# TSF DLLs for optional diagnostics, but installers must not register the TIP.
$ExpectedUnregisteredKeys = @(
  "Software\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32",
  "Software\WOW6432Node\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32",
  "Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\LanguageProfile\0x00000804\{9B5F5E04-23F6-47DA-9A26-D221F6C3F02E}",
  "Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{34745C63-B2F0-4784-8B67-5E12C8701A31}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}",
  "Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{13A016DF-560B-46CD-947A-4C3AF1E0E35D}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}",
  "Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}"
)

function Test-IsAdministrator {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = [Security.Principal.WindowsPrincipal]::new($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Join-ProcessArguments {
  param(
    [string[]]$ArgumentList = @()
  )

  $quoted = foreach ($argument in $ArgumentList) {
    if ($argument.Length -eq 0) {
      '""'
    } elseif ($argument -notmatch '[\s"]') {
      $argument
    } else {
      $escaped = $argument -replace '(\\*)"', '$1$1\"'
      $escaped = $escaped -replace '(\\+)$', '$1$1'
      '"' + $escaped + '"'
    }
  }
  return ($quoted -join " ")
}

function Invoke-CheckedProcess {
  param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath,
    [string[]]$ArgumentList = @(),
    [Parameter(Mandatory = $true)]
    [string]$Label
  )

  $commandLine = Join-ProcessArguments $ArgumentList
  Write-Host "[run] $Label`: $FilePath $commandLine"
  $process = Start-Process -FilePath $FilePath -ArgumentList $commandLine -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    throw "$Label failed with exit code $($process.ExitCode)"
  }
}

function Open-LocalMachineSubKey {
  param(
    [Parameter(Mandatory = $true)]
    [Microsoft.Win32.RegistryView]$View,
    [Parameter(Mandatory = $true)]
    [string]$SubKey
  )

  $baseKey = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, $View)
  try {
    return $baseKey.OpenSubKey($SubKey)
  } finally {
    $baseKey.Dispose()
  }
}

function Assert-RegistryKey {
  param(
    [Parameter(Mandatory = $true)]
    [Microsoft.Win32.RegistryView]$View,
    [Parameter(Mandatory = $true)]
    [string]$SubKey,
    [Parameter(Mandatory = $true)]
    [string]$Label
  )

  $key = Open-LocalMachineSubKey -View $View -SubKey $SubKey
  if ($null -eq $key) {
    throw "Missing $Label registry key ($View): HKLM\$SubKey"
  }
  $key.Close()
  Write-Host "[ok] $Label registry key present ($View)"
}

function Assert-RegistryKeyAbsent {
  param(
    [Parameter(Mandatory = $true)]
    [Microsoft.Win32.RegistryView]$View,
    [Parameter(Mandatory = $true)]
    [string]$SubKey,
    [Parameter(Mandatory = $true)]
    [string]$Label
  )

  $key = Open-LocalMachineSubKey -View $View -SubKey $SubKey
  if ($null -ne $key) {
    $key.Close()
    throw "Unexpected $Label registry key ($View): HKLM\$SubKey"
  }
  Write-Host "[ok] $Label registry key absent ($View)"
}

function Assert-OpenLessImeInstalled {
  $installRootCandidates = @(
    (Join-Path $env:ProgramFiles "OpenLess")
  )
  if ($env:ProgramFiles -ne ${env:ProgramFiles(x86)}) {
    $installRootCandidates += (Join-Path ${env:ProgramFiles(x86)} "OpenLess")
  }

  $installRoot = $installRootCandidates |
    Where-Object { Test-Path -LiteralPath (Join-Path $_ "openless.exe") -PathType Leaf } |
    Select-Object -First 1
  if ([string]::IsNullOrWhiteSpace($installRoot)) {
    throw "Installed OpenLess executable not found under: $($installRootCandidates -join ', ')"
  }

  $expectedX64 = Join-Path $installRoot "windows-ime\x64\OpenLessIme.dll"
  $expectedX86 = Join-Path $installRoot "windows-ime\x86\OpenLessIme.dll"
  foreach ($dll in @($expectedX64, $expectedX86)) {
    if (-not (Test-Path -LiteralPath $dll -PathType Leaf)) {
      throw "Packaged optional IME DLL path does not exist: $dll"
    }
  }

  foreach ($key in $ExpectedUnregisteredKeys) {
    Assert-RegistryKeyAbsent -View Registry64 -SubKey $key -Label "default-disabled TSF"
  }

  Write-Host "[ok] OpenLess installed with TSF registration disabled by default"
  return $installRoot
}

function Uninstall-OpenLess {
  param(
    [Parameter(Mandatory = $true)]
    [string]$InstallRoot
  )

  if ($InstallerKind -eq "nsis") {
    $uninstaller = Join-Path $InstallRoot "uninstall.exe"
    if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
      throw "NSIS uninstaller not found: $uninstaller"
    }
    Invoke-CheckedProcess -FilePath $uninstaller -ArgumentList @("/S") -Label "NSIS uninstall"
  } else {
    Invoke-CheckedProcess -FilePath "msiexec.exe" -ArgumentList @("/x", $InstallerPath, "/qn", "/norestart") -Label "MSI uninstall"
  }
}

if (-not (Test-IsAdministrator)) {
  throw "Windows IME install smoke must run from an elevated Administrator PowerShell."
}

$InstallerPath = (Resolve-Path -LiteralPath $InstallerPath).Path
if ($InstallerKind -eq "nsis") {
  Invoke-CheckedProcess -FilePath $InstallerPath -ArgumentList @("/S", "/AllUsers") -Label "NSIS install"
} else {
  Invoke-CheckedProcess -FilePath "msiexec.exe" -ArgumentList @("/i", $InstallerPath, "/qn", "/norestart") -Label "MSI install"
}

$installRoot = Assert-OpenLessImeInstalled
if (-not $SkipUninstall) {
  Uninstall-OpenLess -InstallRoot $installRoot
}
