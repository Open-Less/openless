[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\")).Path
$sourceRoot = Join-Path $appRoot "crates/openless-core/src"

$violations = [System.Collections.Generic.List[string]]::new()
foreach ($file in Get-ChildItem -LiteralPath $sourceRoot -Filter "*.rs" -Recurse -File) {
    $text = Get-Content -Raw -LiteralPath $file.FullName
    $relative = [System.IO.Path]::GetRelativePath($appRoot, $file.FullName).Replace("\", "/")

    if ($text -match "tokio::runtime::Runtime::new\s*\(") {
        $violations.Add("${relative}: private Tokio Runtime::new is forbidden")
    }

    if ($relative -ne "crates/openless-core/src/config.rs" -and
        $text -match "tokio::runtime::Handle::(?:current|try_current)\s*\(") {
        $violations.Add("${relative}: runtime Handle lookup must stay inside the host TaskSpawner")
    }

    # Production code must submit background work to the injected TaskSpawner.
    # The test modules may use #[tokio::test]/tokio::spawn to orchestrate tests.
    $testMarker = $text.IndexOf("#[cfg(test)]", [System.StringComparison]::Ordinal)
    $production = if ($testMarker -ge 0) { $text.Substring(0, $testMarker) } else { $text }
    if ($production -match "tokio::spawn\s*\(") {
        $violations.Add("${relative}: production tokio::spawn bypasses TaskSpawner")
    }
}

if ($violations.Count -gt 0) {
    $violations | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Output "Core runtime seam gate passed (no private runtime; production tasks use the injected TaskSpawner)."
