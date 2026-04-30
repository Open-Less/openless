param(
  [string]$MirrorRoot = "$env:TEMP\openless-windows-gnu"
)

$ErrorActionPreference = "Stop"

$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$buildRoot = $appRoot

if ($appRoot -match "\s") {
  Write-Host "[info] App path contains spaces: $appRoot"
  Write-Host "[info] Mirroring to no-space build root: $MirrorRoot"
  New-Item -ItemType Directory -Force -Path $MirrorRoot | Out-Null
  robocopy $appRoot $MirrorRoot /MIR /XD "$appRoot\node_modules" "$appRoot\dist" "$appRoot\src-tauri\target" "$MirrorRoot\node_modules" "$MirrorRoot\dist" "$MirrorRoot\src-tauri\target" | Out-Host
  if ($LASTEXITCODE -gt 7) {
    throw "robocopy failed with exit code $LASTEXITCODE"
  }
  $buildRoot = (Resolve-Path $MirrorRoot).Path
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\scoop\persist\rustup\.cargo\bin;$env:USERPROFILE\scoop\apps\rustup\current\.cargo\bin;$env:USERPROFILE\scoop\apps\mingw\current\bin;$env:PATH"
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"
$env:CARGO_BUILD_TARGET = "x86_64-pc-windows-gnu"

Push-Location $buildRoot
try {
  if (-not (Test-Path "node_modules")) {
    npm ci
  }
  npm run tauri build -- --target x86_64-pc-windows-gnu
} finally {
  Pop-Location
}

Write-Host ""
Write-Host "Windows GNU artifacts:"
Write-Host "$buildRoot\src-tauri\target\x86_64-pc-windows-gnu\release\openless.exe"
Write-Host "$buildRoot\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\msi"
Write-Host "$buildRoot\src-tauri\target\x86_64-pc-windows-gnu\release\bundle\nsis"
