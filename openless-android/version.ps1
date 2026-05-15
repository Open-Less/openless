param()

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$DesktopRoot = Join-Path $Root "..\openless-all\app"
$PackageJsonPath = Join-Path $DesktopRoot "package.json"
$TauriConfigPath = Join-Path $DesktopRoot "src-tauri\tauri.conf.json"
$CargoTomlPath = Join-Path $DesktopRoot "src-tauri\Cargo.toml"

foreach ($Path in @($PackageJsonPath, $TauriConfigPath, $CargoTomlPath)) {
    if (-not (Test-Path $Path)) {
        throw "Required desktop version source missing: $Path"
    }
}

$packageJson = Get-Content $PackageJsonPath -Raw | ConvertFrom-Json
$tauriConfig = Get-Content $TauriConfigPath -Raw | ConvertFrom-Json
$cargoToml = Get-Content $CargoTomlPath -Raw

$cargoMatch = [regex]::Match($cargoToml, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $cargoMatch.Success) {
    throw "Could not read version from Cargo.toml"
}

$packageVersion = [string]$packageJson.version
$tauriVersion = [string]$tauriConfig.version
$cargoVersion = [string]$cargoMatch.Groups[1].Value

if ($packageVersion -ne $tauriVersion -or $packageVersion -ne $cargoVersion) {
    throw "Desktop version mismatch: package.json=$packageVersion tauri.conf.json=$tauriVersion Cargo.toml=$cargoVersion"
}

$semverMatch = [regex]::Match($packageVersion, '^(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$')
if (-not $semverMatch.Success) {
    throw "Unsupported version format: $packageVersion"
}

$major = [int]$semverMatch.Groups[1].Value
$minor = [int]$semverMatch.Groups[2].Value
$patch = [int]$semverMatch.Groups[3].Value
$versionCode = $major * 10000 + $minor * 100 + $patch

[pscustomobject]@{
    VersionName = $packageVersion
    VersionCode = $versionCode
    PackageJsonPath = $PackageJsonPath
    TauriConfigPath = $TauriConfigPath
    CargoTomlPath = $CargoTomlPath
}
