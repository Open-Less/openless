param(
  [string]$MirrorRoot = "$env:TEMP\openless-windows-gnu",
  [string]$ArtifactsRoot = "",
  [switch]$KeepMirror
)

$ErrorActionPreference = "Stop"

$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\app")).Path
$allRoot = (Resolve-Path (Join-Path $appRoot "..")).Path
$buildRoot = $appRoot
$usedMirror = $false
if ([string]::IsNullOrWhiteSpace($ArtifactsRoot)) {
  $ArtifactsRoot = Join-Path $appRoot ".artifacts\windows-gnu"
}

if ($appRoot -match "\s") {
  Write-Host "[info] App path contains spaces: $appRoot"
  Write-Host "[info] Mirroring to no-space scratch build root: $MirrorRoot"
  New-Item -ItemType Directory -Force -Path $MirrorRoot | Out-Null
  # Copy all tracked inputs with their all/ relative paths, including external
  # Cargo members linux/egui and harmony/rust and the Windows IME and scripts.
  # Git's exclude rules keep private files and build/dependency outputs out.
  Push-Location $allRoot
  try {
    $inputs = @(& git -c core.quotePath=false ls-files --cached --others --exclude-standard -- .)
    if ($LASTEXITCODE -ne 0) { throw "Cannot enumerate platform mirror inputs" }
    $inputs = @($inputs | Where-Object {
      if ([string]::IsNullOrWhiteSpace($_)) { return $false }
      $src = Join-Path $allRoot $_
      Test-Path -LiteralPath $src -PathType Leaf
    } | Sort-Object -Unique)
    $inputSet = [Collections.Generic.HashSet[string]]::new([string[]]$inputs, [StringComparer]::OrdinalIgnoreCase)
    $mirrorBase = [IO.Path]::GetFullPath($MirrorRoot).TrimEnd('\', '/')
    $mirrorPrefix = $mirrorBase + [IO.Path]::DirectorySeparatorChar
    $manifestPath = Join-Path $MirrorRoot ".openless-mirror-manifest.txt"
    function Resolve-MirrorInput([string]$relative) {
      $destination = [IO.Path]::GetFullPath((Join-Path $mirrorBase $relative))
      if (-not $destination.StartsWith($mirrorPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Mirror input escapes mirror root: $relative"
      }
      # Never treat reusable dependencies or build outputs as source inputs,
      # even if an interrupted/old manifest happens to contain one.
      $normalized = $relative -replace '\\', '/'
      if ($normalized -match '(^|/)node_modules(/|$)|^(?:app/)?(?:target|dist|\.artifacts)(?:/|$)|^app/src-tauri/(?:backend-tests/)?target(?:/|$)|^(?:ios/CoreHost|linux/egui|harmony/rust)/target(?:/|$)') {
        throw "Mirror input points to a protected cache: $relative"
      }
      # Refuse junctions/symlinks before copying or removing through them.
      $ancestor = $destination
      while ($ancestor -ne $mirrorBase) {
        if (Test-Path -LiteralPath $ancestor) {
          if ((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Mirror input traverses a reparse point: $relative"
          }
        }
        $ancestor = Split-Path $ancestor -Parent
      }
      return $destination
    }
    if (Test-Path -LiteralPath $manifestPath -PathType Leaf) {
      foreach ($previous in [IO.File]::ReadAllLines($manifestPath, [Text.Encoding]::UTF8)) {
        if ([string]::IsNullOrWhiteSpace($previous) -or $inputSet.Contains($previous)) { continue }
        $stale = Resolve-MirrorInput $previous
        if (Test-Path -LiteralPath $stale -PathType Leaf) {
          Remove-Item -LiteralPath $stale -Force
        }
        $parent = Split-Path $stale -Parent
        while ($parent -ne $mirrorBase -and (Test-Path -LiteralPath $parent -PathType Container)) {
          if (@(Get-ChildItem -LiteralPath $parent -Force).Count -ne 0) { break }
          Remove-Item -LiteralPath $parent -Force
          $parent = Split-Path $parent -Parent
        }
      }
    }
    foreach ($relative in $inputs) {
      $null = Resolve-MirrorInput $relative
      $destination = Join-Path $MirrorRoot $relative
      New-Item -ItemType Directory -Force -Path (Split-Path $destination -Parent) | Out-Null
      Copy-Item -LiteralPath (Join-Path $allRoot $relative) -Destination $destination -Force
    }
    [IO.File]::WriteAllLines($manifestPath, [string[]]$inputs, [Text.UTF8Encoding]::new($false))
  } finally {
    Pop-Location
  }
  $buildRoot = (Resolve-Path (Join-Path $MirrorRoot "app")).Path
  $usedMirror = $true
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\scoop\persist\rustup\.cargo\bin;$env:USERPROFILE\scoop\apps\rustup\current\.cargo\bin;$env:USERPROFILE\scoop\apps\mingw\current\bin;$env:PATH"
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"
$env:CARGO_BUILD_TARGET = "x86_64-pc-windows-gnu"

function Resolve-WebView2Loader {
  $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
  $registrySrc = Join-Path $cargoHome "registry\src"
  $loader = Get-ChildItem -Path $registrySrc -Recurse -Filter WebView2Loader.dll -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match "\\x64\\WebView2Loader\.dll$" } |
    Select-Object -First 1
  if ($null -eq $loader) {
    throw "WebView2Loader.dll x64 not found under $registrySrc"
  }
  return $loader.FullName
}

Push-Location $buildRoot
try {
  if (-not (Test-Path "node_modules")) {
    npm ci
  }
  npm run tauri build -- --target x86_64-pc-windows-gnu --no-bundle
  $releaseRoot = Join-Path $buildRoot "src-tauri\target\x86_64-pc-windows-gnu\release"
  $artifactDevRoot = Join-Path $ArtifactsRoot "dev"
  New-Item -ItemType Directory -Force -Path $artifactDevRoot | Out-Null
  Copy-Item -LiteralPath (Join-Path $releaseRoot "openless.exe") -Destination (Join-Path $artifactDevRoot "openless.exe") -Force
  Copy-Item -LiteralPath (Resolve-WebView2Loader) -Destination (Join-Path $artifactDevRoot "WebView2Loader.dll") -Force

  # Build OpenLessIme.dll (x64 + x86) 并导出绝对路径 env var——
  # nsis/openless-ime-hooks.nsh 的 PREINSTALL 用 File "$%OPENLESS_IME_DLL_X64%" / _X86
  # 在 makensis 编译时把 dll 嵌入 NSIS 包；wix/openless-ime.wxs 也用同一对 env 解析
  # candle/light 的绝对路径（CI release-tauri.yml 同款做法）。
  foreach ($t in @(
    @{ Platform = 'x64';   Folder = 'x64'; EnvName = 'OPENLESS_IME_DLL_X64' },
    @{ Platform = 'Win32'; Folder = 'x86'; EnvName = 'OPENLESS_IME_DLL_X86' }
  )) {
    $out = Join-Path $buildRoot "src-tauri\target\windows-ime-msvc\$($t.Folder)\Release"
    $obj = Join-Path $buildRoot "src-tauri\target\windows-ime-msvc\obj\$($t.Folder)\Release"
    & ./../windows/scripts/windows-ime-build.ps1 -Configuration Release -Platform $t.Platform -OutputDirectory $out -IntermediateDirectory $obj
    if ($LASTEXITCODE -ne 0) {
      throw "OpenLessIme $($t.Platform) build failed with exit $LASTEXITCODE"
    }
    $dll = (Resolve-Path (Join-Path $out 'OpenLessIme.dll')).Path
    if (-not (Test-Path $dll)) {
      throw "OpenLessIme.dll not produced at $dll"
    }
    Set-Item -Path "Env:$($t.EnvName)" -Value $dll
    Write-Host "[ok] built $dll (exported $($t.EnvName))"
  }

  npm run tauri build -- --target x86_64-pc-windows-gnu --bundles msi nsis
} finally {
  Pop-Location
}

$releaseRoot = Join-Path $buildRoot "src-tauri\target\x86_64-pc-windows-gnu\release"
$artifactReleaseRoot = Join-Path $ArtifactsRoot "release"
New-Item -ItemType Directory -Force -Path $artifactReleaseRoot | Out-Null
Remove-Item -LiteralPath (Join-Path $artifactReleaseRoot "openless.exe") -Force -ErrorAction SilentlyContinue

if (Test-Path (Join-Path $releaseRoot "bundle")) {
  Copy-Item -LiteralPath (Join-Path $releaseRoot "bundle") -Destination $artifactReleaseRoot -Recurse -Force
}

if ($usedMirror -and (-not $KeepMirror)) {
  $resolvedMirror = (Resolve-Path $MirrorRoot).Path
  $resolvedTemp = (Resolve-Path $env:TEMP).Path
  if ($resolvedMirror.StartsWith($resolvedTemp, [System.StringComparison]::OrdinalIgnoreCase) -and
      ((Split-Path $resolvedMirror -Leaf) -eq "openless-windows-gnu")) {
    Write-Host "[info] Removing scratch build root: $resolvedMirror"
    Remove-Item -LiteralPath $resolvedMirror -Recurse -Force
  } else {
    Write-Warning "Refusing to remove unexpected mirror path: $resolvedMirror"
  }
}

Write-Host ""
Write-Host "Windows GNU artifacts:"
Write-Host "$ArtifactsRoot\dev\openless.exe"
Write-Host "$artifactReleaseRoot\bundle\msi"
Write-Host "$artifactReleaseRoot\bundle\nsis"
