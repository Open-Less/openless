param(
  [string]$AppRoot = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($AppRoot)) {
  $AppRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

$lockPath = Join-Path $AppRoot "src-tauri/Cargo.lock"
$lock = Get-Content -LiteralPath $lockPath -Raw
$match = [regex]::Match(
  $lock,
  '(?ms)\[\[package\]\]\r?\nname = "sherpa-onnx-sys"\r?\nversion = "([^"]+)"'
)
if (-not $match.Success) {
  throw "sherpa-onnx-sys version not found in $lockPath"
}

$version = $match.Groups[1].Value
if ($version -notmatch '^\d+\.\d+\.\d+$') {
  throw "Invalid sherpa-onnx-sys version in ${lockPath}: $version"
}
$archiveStem = "sherpa-onnx-v$version-win-x64-static-MT-Release-lib"
$archiveName = "$archiveStem.tar.bz2"
$cacheRoot = Join-Path $AppRoot "src-tauri/target/sherpa-onnx-prebuilt"
$extractedRoot = Join-Path $cacheRoot $archiveStem
$cachePrefix = [IO.Path]::GetFullPath($cacheRoot) + [IO.Path]::DirectorySeparatorChar
if (-not [IO.Path]::GetFullPath($extractedRoot).StartsWith($cachePrefix, [StringComparison]::OrdinalIgnoreCase)) {
  throw "Refusing to use Sherpa path outside the cache root: $extractedRoot"
}
$library = Get-ChildItem -LiteralPath $extractedRoot -Recurse -Filter "sherpa-onnx-c-api.lib" -File -ErrorAction SilentlyContinue |
  Select-Object -First 1

if ($null -eq $library) {
  New-Item -ItemType Directory -Force -Path $cacheRoot | Out-Null
  $archivePath = Join-Path $cacheRoot $archiveName
  if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
    $partialPath = "$archivePath.partial"
    Invoke-WebRequest -Uri "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$version/$archiveName" -OutFile $partialPath
    Move-Item -LiteralPath $partialPath -Destination $archivePath -Force
  }

  $stagingRoot = Join-Path $cacheRoot ".extract-$([guid]::NewGuid().ToString('N'))"
  New-Item -ItemType Directory -Path $stagingRoot | Out-Null
  try {
    & tar.exe -xjf $archivePath -C $stagingRoot
    if ($LASTEXITCODE -ne 0) {
      Remove-Item -LiteralPath $archivePath -Force -ErrorAction SilentlyContinue
      throw "Failed to extract $archivePath (tar exit $LASTEXITCODE)"
    }
    $staged = Join-Path $stagingRoot $archiveStem
    $stagedLibrary = Get-ChildItem -LiteralPath $staged -Recurse -Filter "sherpa-onnx-c-api.lib" -File -ErrorAction SilentlyContinue |
      Select-Object -First 1
    if ($null -eq $stagedLibrary) {
      throw "$archiveName does not contain sherpa-onnx-c-api.lib"
    }
    if (Test-Path -LiteralPath $extractedRoot) {
      Remove-Item -LiteralPath $extractedRoot -Recurse -Force
    }
    Move-Item -LiteralPath $staged -Destination $extractedRoot
  } finally {
    Remove-Item -LiteralPath $stagingRoot -Recurse -Force -ErrorAction SilentlyContinue
  }

  $library = Get-ChildItem -LiteralPath $extractedRoot -Recurse -Filter "sherpa-onnx-c-api.lib" -File |
    Select-Object -First 1
}

$libDir = $library.Directory.FullName
$env:SHERPA_ONNX_LIB_DIR = $libDir
if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) {
  Add-Content -LiteralPath $env:GITHUB_ENV -Value "SHERPA_ONNX_LIB_DIR=$libDir"
}
Write-Host "[ok] SHERPA_ONNX_LIB_DIR -> $libDir"
