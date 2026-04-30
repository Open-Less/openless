param(
  [string]$MirrorRoot = "$env:TEMP\openless-windows-gnu",
  [string]$ArtifactsRoot = "",
  [switch]$KeepMirror
)

$ErrorActionPreference = "Stop"

$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$buildRoot = $appRoot
$usedMirror = $false
if ([string]::IsNullOrWhiteSpace($ArtifactsRoot)) {
  $ArtifactsRoot = Join-Path $appRoot ".artifacts\windows-gnu"
}

if ($appRoot -match "\s") {
  Write-Host "[info] App path contains spaces: $appRoot"
  Write-Host "[info] Mirroring to no-space scratch build root: $MirrorRoot"
  New-Item -ItemType Directory -Force -Path $MirrorRoot | Out-Null
  robocopy $appRoot $MirrorRoot /MIR /XD "$appRoot\.artifacts" "$appRoot\node_modules" "$appRoot\dist" "$appRoot\src-tauri\target" "$MirrorRoot\.artifacts" "$MirrorRoot\node_modules" "$MirrorRoot\dist" "$MirrorRoot\src-tauri\target" | Out-Host
  if ($LASTEXITCODE -gt 7) {
    throw "robocopy failed with exit code $LASTEXITCODE"
  }
  $buildRoot = (Resolve-Path $MirrorRoot).Path
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
