param(
  [string]$ArtifactsRoot = "",
  [switch]$SkipRustInstall,
  [switch]$SkipNpmCi,
  [switch]$CleanArtifacts
)

$ErrorActionPreference = "Stop"

$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$releaseRoot = Join-Path $appRoot "src-tauri\target\x86_64-pc-windows-msvc\release"
$imeBuildRoot = Join-Path $appRoot "src-tauri\target\windows-ime-msvc"
if ([string]::IsNullOrWhiteSpace($ArtifactsRoot)) {
  $ArtifactsRoot = Join-Path $appRoot ".artifacts\windows-msvc"
}

function Add-PathEntry($PathEntry) {
  if ([string]::IsNullOrWhiteSpace($PathEntry) -or -not (Test-Path $PathEntry)) {
    return
  }
  $entries = $env:PATH -split ";"
  if ($entries -notcontains $PathEntry) {
    $env:PATH = "$PathEntry;$env:PATH"
  }
}

function Test-Command($Name) {
  return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Install-RustMsvcToolchain {
  $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
  Add-PathEntry $cargoBin

  $hasRustup = Test-Command "rustup"
  $hasCargo = Test-Command "cargo"
  $hasRustc = Test-Command "rustc"
  $hasToolchain = $false
  if ($hasRustup) {
    $toolchains = & cmd.exe /d /c "rustup toolchain list 2>nul"
    $hasToolchain = $LASTEXITCODE -eq 0 -and $toolchains -match "stable-x86_64-pc-windows-msvc"
  }

  if ($hasRustup -and $hasCargo -and $hasRustc -and $hasToolchain) {
    Write-Host "[ok] Rust MSVC toolchain already installed"
    return
  }

  if ($SkipRustInstall) {
    throw "Rust MSVC toolchain is missing. Re-run without -SkipRustInstall to install it automatically."
  }

  Write-Host "[info] Installing Rust stable-x86_64-pc-windows-msvc"
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  $rustupInit = Join-Path $env:TEMP "rustup-init-x86_64-pc-windows-msvc.exe"
  Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit
  & $rustupInit -y --default-toolchain stable-x86_64-pc-windows-msvc

  Add-PathEntry $cargoBin
  & rustup toolchain install stable-x86_64-pc-windows-msvc
  & rustup default stable-x86_64-pc-windows-msvc

  if (-not (Test-Command "cargo") -or -not (Test-Command "rustc")) {
    throw "Rust installation finished, but cargo/rustc is still not available in PATH."
  }
}

function Find-VsDevCmd {
  $candidates = @()

  $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path $vswhere) {
    $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if (-not [string]::IsNullOrWhiteSpace($installPath)) {
      $candidates += (Join-Path $installPath "Common7\Tools\VsDevCmd.bat")
    }
  }

  $candidates += @(
    (Join-Path $env:ProgramFiles "Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat"),
    (Join-Path $env:ProgramFiles "Microsoft Visual Studio\2022\Professional\Common7\Tools\VsDevCmd.bat"),
    (Join-Path $env:ProgramFiles "Microsoft Visual Studio\2022\Enterprise\Common7\Tools\VsDevCmd.bat"),
    (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat")
  )

  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      return (Resolve-Path $candidate).Path
    }
  }

  throw "VsDevCmd.bat not found. Install Visual Studio 2022 Build Tools with the Desktop development with C++ workload."
}

function Find-WixTool($Name) {
  $tauriWixRoot = Join-Path $env:LOCALAPPDATA "tauri"
  if (Test-Path $tauriWixRoot) {
    $tauriWixTools = Get-ChildItem -LiteralPath $tauriWixRoot -Directory -Filter "WixTools*" -ErrorAction SilentlyContinue |
      Sort-Object @{ Expression = { if ($_.Name -match '^WixTools(\d+)$') { [int]$Matches[1] } else { -1 } }; Descending = $true }, @{ Expression = "Name"; Descending = $true }
    foreach ($toolDir in $tauriWixTools) {
      $tauriWixTool = Join-Path $toolDir.FullName $Name
      if (Test-Path $tauriWixTool) {
        return (Resolve-Path $tauriWixTool).Path
      }
    }
  }

  $cmd = Get-Command $Name -ErrorAction SilentlyContinue
  if ($cmd) {
    return $cmd.Source
  }

  throw "$Name not found. Run the Tauri MSI build once so a WiX tools directory is installed under $tauriWixRoot."
}

function Get-PackageVersion {
  $packageJson = Get-Content -LiteralPath (Join-Path $appRoot "package.json") -Raw | ConvertFrom-Json
  return $packageJson.version
}

function Get-MsiName {
  return "OpenLess_$(Get-PackageVersion)_x64_en-US.msi"
}

function Get-MsiPath {
  return Join-Path $releaseRoot "bundle\msi\$(Get-MsiName)"
}

function Test-WebView2Runtime {
  $paths = @(
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F1E7FBD4-9C4C-41A4-AB01-7C0F7A947F1A}",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F1E7FBD4-9C4C-41A4-AB01-7C0F7A947F1A}"
  )
  foreach ($path in $paths) {
    if (Test-Path $path) {
      Write-Host "[ok] WebView2 Runtime registry key found"
      return
    }
  }
  Write-Warning "WebView2 Runtime registry key not found. Install Evergreen runtime if the app window is blank."
}

function Invoke-MsvcBuild {
  param(
    [string]$VsDevCmd,
    [string]$CargoBin
  )

  if ([string]::IsNullOrWhiteSpace($env:OPENLESS_IME_DLL_X64) -or -not (Test-Path $env:OPENLESS_IME_DLL_X64)) {
    throw "OPENLESS_IME_DLL_X64 must point to the built x64 OpenLessIme.dll before the MSI build."
  }
  if ([string]::IsNullOrWhiteSpace($env:OPENLESS_IME_DLL_X86) -or -not (Test-Path $env:OPENLESS_IME_DLL_X86)) {
    throw "OPENLESS_IME_DLL_X86 must point to the built x86 OpenLessIme.dll before the MSI build."
  }

  $msiPath = Get-MsiPath
  Remove-Item -LiteralPath $msiPath -Force -ErrorAction SilentlyContinue

  # cmd expands %PATH% before executing `call VsDevCmd`, which would discard
  # the MSVC paths and select Git/MSYS `link.exe`. Delayed expansion keeps the
  # environment produced by VsDevCmd intact for Cargo and Tauri.
  $buildCommand = "call `"$VsDevCmd`" -arch=x64 -host_arch=x64 && set `"PATH=$CargoBin;!PATH!`" && set `"OPENLESS_IME_DLL_X64=$env:OPENLESS_IME_DLL_X64`" && set `"OPENLESS_IME_DLL_X86=$env:OPENLESS_IME_DLL_X86`" && npm.cmd run tauri build -- --target x86_64-pc-windows-msvc --bundles msi"
  & cmd.exe /V:ON /d /c $buildCommand
  if ($LASTEXITCODE -ne 0) {
    Write-Warning "Tauri Windows MSI build returned exit code $LASTEXITCODE. Trying to finish MSI linking from generated WiX objects."
    Repair-TauriMsiBundle
  }
}

function Repair-TauriMsiBundle {
  $wixRoot = Join-Path $releaseRoot "wix\x64"
  $mainObject = Join-Path $wixRoot "main.wixobj"
  $imeObject = Join-Path $wixRoot "openless-ime.wixobj"
  $locale = Join-Path $wixRoot "locale.wxl"
  $msiPath = Get-MsiPath

  foreach ($requiredPath in @($mainObject, $imeObject, $locale, $env:OPENLESS_IME_DLL_X64, $env:OPENLESS_IME_DLL_X86)) {
    if ([string]::IsNullOrWhiteSpace($requiredPath) -or -not (Test-Path $requiredPath)) {
      throw "Cannot repair Tauri MSI bundle because a required file is missing: $requiredPath"
    }
  }

  $bundleDir = Split-Path -Parent $msiPath
  New-Item -ItemType Directory -Force -Path $bundleDir | Out-Null
  Remove-Item -LiteralPath $msiPath -Force -ErrorAction SilentlyContinue

  $light = Find-WixTool "light.exe"
  # -sice:ICE80：x86 IME DLL 与 x64 一起装进 INSTALLDIR\windows-ime\，
  # 32-bit Component 落在 64-bit Directory 下是 ICE80 的告警，但本场景路径
  # 绝对指向、不依赖 SysWOW64 重定向，是 Microsoft 文档允许的合法用法。
  # 与 .github/workflows/release-tauri.yml 的 light 调用保持一致。
  & $light -nologo -sice:ICE80 -ext WixUIExtension -ext WixUtilExtension -loc $locale -out $msiPath $mainObject $imeObject
  if ($LASTEXITCODE -ne 0) {
    throw "WiX light.exe failed with exit code $LASTEXITCODE."
  }
  if (-not (Test-Path $msiPath)) {
    throw "WiX light.exe finished but MSI was not produced: $msiPath"
  }

  Write-Host "[ok] MSI linked from generated WiX objects -> $msiPath"
}

function Invoke-OpenLessImeBuild {
  $buildScript = Join-Path $PSScriptRoot "windows-ime-build.ps1"

  if (-not (Test-Path $buildScript)) {
    throw "OpenLess IME build script not found: $buildScript"
  }

  $targets = @(
    @{ Platform = "x64"; Folder = "x64"; EnvName = "OPENLESS_IME_DLL_X64" },
    @{ Platform = "Win32"; Folder = "x86"; EnvName = "OPENLESS_IME_DLL_X86" }
  )
  foreach ($target in $targets) {
    $imeOutDir = Join-Path $imeBuildRoot "$($target.Folder)\Release"
    $imeIntDir = Join-Path $imeBuildRoot "obj\$($target.Folder)\Release"
    & $buildScript -Configuration Release -Platform $target.Platform -OutputDirectory $imeOutDir -IntermediateDirectory $imeIntDir
    if ($LASTEXITCODE -ne 0) {
      throw "OpenLessIme $($target.Platform) build failed with exit code $LASTEXITCODE."
    }

    $imeDll = Join-Path $imeOutDir "OpenLessIme.dll"
    if (-not (Test-Path $imeDll)) {
      throw "OpenLessIme.dll was not produced: $imeDll"
    }

    Set-Item -Path "Env:$($target.EnvName)" -Value (Resolve-Path $imeDll).Path
    Write-Host "[ok] $($target.EnvName) -> $((Get-Item -Path "Env:$($target.EnvName)").Value)"
  }
}

function Reset-ArtifactsRoot {
  if (-not $CleanArtifacts) {
    New-Item -ItemType Directory -Force -Path $ArtifactsRoot | Out-Null
    return
  }

  $resolvedAppRoot = (Resolve-Path $appRoot).Path
  if (Test-Path $ArtifactsRoot) {
    $resolvedArtifactsRoot = (Resolve-Path $ArtifactsRoot).Path
    if (-not $resolvedArtifactsRoot.StartsWith($resolvedAppRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
      throw "-CleanArtifacts refuses to delete output outside the app root: $resolvedArtifactsRoot"
    }
    Remove-Item -LiteralPath $resolvedArtifactsRoot -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $ArtifactsRoot | Out-Null
}

function Copy-WindowsArtifacts {
  $version = Get-PackageVersion
  $msiName = Get-MsiName
  $msiPath = Get-MsiPath
  $exePath = Join-Path $releaseRoot "openless.exe"
  $webView2Loader = Get-ChildItem -Path (Join-Path $releaseRoot "build") -Recurse -Filter "WebView2Loader.dll" -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "\\out\\x64\\WebView2Loader\.dll$" } |
    Select-Object -First 1

  if (-not (Test-Path $msiPath)) {
    throw "MSI not found: $msiPath"
  }
  if (-not (Test-Path $exePath)) {
    throw "Release exe not found: $exePath"
  }
  if ($null -eq $webView2Loader) {
    throw "WebView2Loader.dll x64 not found under $releaseRoot\build"
  }
  if ([string]::IsNullOrWhiteSpace($env:OPENLESS_IME_DLL_X64) -or -not (Test-Path $env:OPENLESS_IME_DLL_X64)) {
    throw "x64 OpenLessIme.dll not found for portable package: $env:OPENLESS_IME_DLL_X64"
  }
  if ([string]::IsNullOrWhiteSpace($env:OPENLESS_IME_DLL_X86) -or -not (Test-Path $env:OPENLESS_IME_DLL_X86)) {
    throw "x86 OpenLessIme.dll not found for portable package: $env:OPENLESS_IME_DLL_X86"
  }

  Reset-ArtifactsRoot
  Copy-Item -LiteralPath $msiPath -Destination (Join-Path $ArtifactsRoot $msiName) -Force

  $portableName = "OpenLess_${version}_x64_portable"
  $portableRoot = Join-Path $ArtifactsRoot $portableName
  $portableImeRoot = Join-Path $portableRoot "windows-ime"
  $portableImeX64Root = Join-Path $portableImeRoot "x64"
  $portableImeX86Root = Join-Path $portableImeRoot "x86"
  New-Item -ItemType Directory -Force -Path $portableRoot | Out-Null
  New-Item -ItemType Directory -Force -Path $portableImeX64Root | Out-Null
  New-Item -ItemType Directory -Force -Path $portableImeX86Root | Out-Null
  Copy-Item -LiteralPath $exePath -Destination (Join-Path $portableRoot "openless.exe") -Force
  Copy-Item -LiteralPath $webView2Loader.FullName -Destination (Join-Path $portableRoot "WebView2Loader.dll") -Force
  Copy-Item -LiteralPath $env:OPENLESS_IME_DLL_X64 -Destination (Join-Path $portableImeX64Root "OpenLessIme.dll") -Force
  Copy-Item -LiteralPath $env:OPENLESS_IME_DLL_X86 -Destination (Join-Path $portableImeX86Root "OpenLessIme.dll") -Force

  $zipPath = Join-Path $ArtifactsRoot "$portableName.zip"
  Remove-Item -LiteralPath $zipPath -Force -ErrorAction SilentlyContinue
  Compress-Archive -LiteralPath $portableRoot -DestinationPath $zipPath -CompressionLevel Optimal

  Write-Host ""
  Write-Host "Windows artifacts:"
  Get-ChildItem -File -LiteralPath $ArtifactsRoot | Select-Object Name,Length,LastWriteTime | Format-Table -AutoSize

  Write-Host "SHA256:"
  Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $ArtifactsRoot $msiName), $zipPath | Select-Object Path,Hash | Format-List
}

Push-Location $appRoot
try {
  Write-Host "[info] App root: $appRoot"
  Install-RustMsvcToolchain
  Test-WebView2Runtime

  $vsDevCmd = Find-VsDevCmd
  Write-Host "[ok] VsDevCmd.bat -> $vsDevCmd"

  if (-not (Test-Command "node") -or -not (Test-Command "npm.cmd")) {
    throw "Node.js/npm.cmd not found. Install Node.js before packaging."
  }

  if ($SkipNpmCi) {
    if (-not (Test-Path (Join-Path $appRoot "node_modules"))) {
      throw "-SkipNpmCi was set, but node_modules does not exist."
    }
    Write-Host "[info] Skipping npm.cmd ci"
  } else {
    npm.cmd ci
  }

  $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
  Invoke-OpenLessImeBuild
  . (Join-Path $PSScriptRoot "prepare-windows-sherpa.ps1") -AppRoot $appRoot
  Invoke-MsvcBuild -VsDevCmd $vsDevCmd -CargoBin $cargoBin
  Copy-WindowsArtifacts
} finally {
  Pop-Location
}
