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
$LangId = "0x0000ffff"
$SpeechCategoryGuid = "{B5A73CD1-8355-426B-A161-259808F26B14}"
$ImmersiveCategoryGuid = "{13A016DF-560B-46CD-947A-4C3AF1E0E35D}"
$SystrayCategoryGuid = "{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}"

# Keep this script aligned with the backend status check and the TSF IPC path
# used by OpenLessImeMessageWindow (WM_COPYDATA).
$ExpectedBackendKeys = @(
  "Software\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32",
  "Software\WOW6432Node\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32",
  "Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\LanguageProfile\0x0000ffff\{9B5F5E04-23F6-47DA-9A26-D221F6C3F02E}",
  "Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{B5A73CD1-8355-426B-A161-259808F26B14}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}",
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

function Get-DefaultRegistryValue {
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
  try {
    $value = [string]$key.GetValue("")
    if ([string]::IsNullOrWhiteSpace($value)) {
      throw "$Label default registry value is empty ($View): HKLM\$SubKey"
    }
    return $value
  } finally {
    $key.Close()
  }
}

function Assert-OpenLessImeInstalled {
  $comKey = "Software\Classes\CLSID\$TextServiceClsid\InprocServer32"
  $x64Dll = Get-DefaultRegistryValue -View Registry64 -SubKey $comKey -Label "x64 COM"
  $x86Dll = Get-DefaultRegistryValue -View Registry32 -SubKey $comKey -Label "x86 COM"

  foreach ($dll in @($x64Dll, $x86Dll)) {
    if (-not (Test-Path -LiteralPath $dll -PathType Leaf)) {
      throw "Registered IME DLL path does not exist: $dll"
    }
  }

  $installRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $x64Dll))
  $expectedX64 = Join-Path $installRoot "windows-ime\x64\OpenLessIme.dll"
  $expectedX86 = Join-Path $installRoot "windows-ime\x86\OpenLessIme.dll"
  if ($x64Dll -ne $expectedX64) {
    throw "x64 COM DLL path points outside the installed IME directory. Expected '$expectedX64', got '$x64Dll'"
  }
  if ($x86Dll -ne $expectedX86) {
    throw "x86 COM DLL path points outside the installed IME directory. Expected '$expectedX86', got '$x86Dll'"
  }
  if (-not (Test-Path -LiteralPath (Join-Path $installRoot "openless.exe") -PathType Leaf)) {
    throw "Installed OpenLess executable not found under $installRoot"
  }

  Assert-RegistryKey -View Registry64 -SubKey "Software\Microsoft\CTF\TIP\$TextServiceClsid\LanguageProfile\$LangId\$ProfileGuid" -Label "TSF language profile"
  Assert-RegistryKey -View Registry64 -SubKey "Software\Microsoft\CTF\TIP\$TextServiceClsid\Category\Category\$SpeechCategoryGuid\$TextServiceClsid" -Label "TSF speech category"
  Assert-RegistryKey -View Registry64 -SubKey "Software\Microsoft\CTF\TIP\$TextServiceClsid\Category\Category\$ImmersiveCategoryGuid\$TextServiceClsid" -Label "TSF immersive category"
  Assert-RegistryKey -View Registry64 -SubKey "Software\Microsoft\CTF\TIP\$TextServiceClsid\Category\Category\$SystrayCategoryGuid\$TextServiceClsid" -Label "TSF systray category"

  foreach ($key in $ExpectedBackendKeys) {
    Assert-RegistryKey -View Registry64 -SubKey $key -Label "backend-required"
  }

  Write-Host "[ok] Windows IME backend would report installed"
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
