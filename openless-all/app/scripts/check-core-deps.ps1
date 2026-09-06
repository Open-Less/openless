[CmdletBinding()]
param(
    [ValidateSet("openless-core", "openless-linux-egui")]
    [string]$Package = "openless-core"
)

$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\")).Path
$forbidden = if ($Package -eq "openless-core") {
    "tauri|wry|webkit2gtk|egui|eframe"
} else {
    "tauri|wry|webkit2gtk"
}

$tree = & cargo tree --locked --manifest-path (Join-Path $appRoot "Cargo.toml") -p $Package -e normal 2>&1
if ($LASTEXITCODE -ne 0) {
    $tree | Write-Error
    exit $LASTEXITCODE
}

$matches = $tree | Select-String -Pattern $forbidden -CaseSensitive:$false
if ($matches) {
    Write-Error "$Package has forbidden host dependencies: $($matches -join ', ')"
    exit 1
}

Write-Output "$Package dependency gate passed (no $forbidden)."
