[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\")).Path
$coreRoot = Join-Path $appRoot "crates/openless-core"
$sourceRoot = Join-Path $coreRoot "src"
$testsRoot = Join-Path $coreRoot "tests"
$forbiddenPattern = 'data_dir:\s*"data"\.into\(\)'

$matches = & rg -n --glob "*.rs" $forbiddenPattern $sourceRoot $testsRoot 2>&1
if ($LASTEXITCODE -gt 1) {
    $matches | Write-Error
    exit $LASTEXITCODE
}
if ($LASTEXITCODE -eq 0) {
    $matches | ForEach-Object { Write-Error "Core test uses the shared crate-local data directory: $_" }
    exit 1
}

$crateDataDir = Join-Path $coreRoot "data"
if (Test-Path -LiteralPath $crateDataDir) {
    Write-Error "Core tests left a runtime data directory in the source tree: $crateDataDir"
    exit 1
}

Write-Output "Core test isolation gate passed (no shared crate-local data directory or source-tree residue)."
exit 0
