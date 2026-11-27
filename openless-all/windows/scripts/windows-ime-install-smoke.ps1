param(
  [Parameter(Mandatory = $true)]
  [string]$InstallerPath,
  [Parameter(Mandatory = $true)]
  [ValidateSet("nsis", "msi")]
  [string]$InstallerKind,
  [switch]$SkipUninstall,
  [string]$InstallerLogPath
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
    } elseif ($argument -notmatch '"') {
      '"' + $argument + '"'
    } else {
      $argument
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
    [string]$Label,
    [int[]]$SuccessExitCodes = @(0)
  )

  $commandLine = Join-ProcessArguments $ArgumentList
  Write-Host "[run] $Label`: $FilePath $commandLine"
  $process = Start-Process -FilePath $FilePath -ArgumentList $commandLine -Wait -PassThru
  if ($process.ExitCode -notin $SuccessExitCodes) {
    throw "$Label failed with exit code $($process.ExitCode)"
  }
  if ($process.ExitCode -eq 3010) {
    Write-Host "[ok] $Label succeeded; Windows requested a reboot (3010). Continuing payload/registry verification without rebooting."
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
  $expectedX64Directory = Join-Path $installRoot "windows-ime\x64"
  $expectedX86Directory = Join-Path $installRoot "windows-ime\x86"
  foreach ($registered in @(
      @{ Path = $x64Dll; Directory = $expectedX64Directory; Label = "x64" },
      @{ Path = $x86Dll; Directory = $expectedX86Directory; Label = "x86" }
    )) {
    if ((Split-Path -Parent $registered.Path) -ne $registered.Directory -or
        (Split-Path -Leaf $registered.Path) -notmatch '^OpenLessIme-.+\.dll$') {
      throw "$($registered.Label) COM DLL path is not a versioned installed IME DLL: $($registered.Path)"
    }
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
    Invoke-CheckedProcess -FilePath "msiexec.exe" -ArgumentList @("/x", $InstallerPath, "/qn", "/norestart") -Label "MSI uninstall" -SuccessExitCodes @(0, 3010)
  }
}

if (-not (Test-IsAdministrator)) {
  throw "Windows IME install smoke must run from an elevated Administrator PowerShell."
}

$InstallerPath = (Resolve-Path -LiteralPath $InstallerPath).Path
if ($InstallerKind -eq "nsis") {
  $nsisArguments = @("/S", "/AllUsers")
  $logDirectory = $null
  if (-not [string]::IsNullOrWhiteSpace($InstallerLogPath)) {
    $logDirectory = Split-Path -Parent $InstallerLogPath
    if (-not [string]::IsNullOrWhiteSpace($logDirectory)) {
      New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
    }
    $nsisArguments += "/LOG=`"$InstallerLogPath`""
  }
  try {
    Invoke-CheckedProcess -FilePath $InstallerPath -ArgumentList $nsisArguments -Label "NSIS install"
  } catch {
    if (-not [string]::IsNullOrWhiteSpace($InstallerLogPath)) {
      $logPresent = Test-Path -LiteralPath $InstallerLogPath -PathType Leaf
      Write-Host "[diagnostic] NSIS log path present: $logPresent"
      if ($logPresent) {
        Write-Host "[diagnostic] NSIS installer log (last 200 lines):"
        Get-Content -LiteralPath $InstallerLogPath -Tail 200 | ForEach-Object { Write-Host $_ }
      } else {
        $logDirectories = @(
          $logDirectory,
          $env:RUNNER_TEMP,
          $env:TEMP,
          $env:TMP,
          (Split-Path -Parent $InstallerPath)
        ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Unique
        foreach ($directory in $logDirectories) {
          if (-not (Test-Path -LiteralPath $directory -PathType Container)) {
            continue
          }
          $candidates = @(
            Get-ChildItem -LiteralPath $directory -Filter '*.log' -File -ErrorAction SilentlyContinue |
              Sort-Object LastWriteTime -Descending |
              Select-Object -First 20
          )
          Write-Host "[diagnostic] NSIS log candidates in ${directory}: $($candidates.Count)"
          foreach ($candidate in $candidates) {
            Write-Host "[diagnostic] candidate=$($candidate.Name) bytes=$($candidate.Length)"
          }
          $markers = @(
            Get-ChildItem -LiteralPath $directory -Filter 'openless-ime-install-diagnostic.ndjson' -File -ErrorAction SilentlyContinue |
              Sort-Object LastWriteTime -Descending |
              Select-Object -First 1
          )
          foreach ($marker in $markers) {
            Write-Host "[diagnostic] NSIS stage markers (last 200 lines):"
            Get-Content -LiteralPath $marker.FullName -Tail 200 | ForEach-Object { Write-Host $_ }
          }
        }
      }
    }
    throw
  }
} else {
  Invoke-CheckedProcess -FilePath "msiexec.exe" -ArgumentList @("/i", $InstallerPath, "/qn", "/norestart") -Label "MSI install" -SuccessExitCodes @(0, 3010)
}

$installRoot = Assert-OpenLessImeInstalled
if (-not $SkipUninstall) {
  Uninstall-OpenLess -InstallRoot $installRoot
}
