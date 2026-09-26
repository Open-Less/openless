[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$appRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\")).Path
$sourceRoot = Join-Path $appRoot "crates/openless-core/src"

$files = @(Get-ChildItem -LiteralPath $sourceRoot -Filter "*.rs" -Recurse -File)
$textByPath = @{}
foreach ($file in $files) {
    $textByPath[$file.FullName] = Get-Content -Raw -LiteralPath $file.FullName
}

function Get-CoreRelativePath([string] $fullName) {
    return [System.IO.Path]::GetRelativePath($appRoot, $fullName).Replace("\", "/")
}

# A module file is test-only when some module file declares it with `#[cfg(test)]`,
# or when another test-only module declares it (a `#[path]` include, for example).
# Test-only files are absent from a production build, so the TaskSpawner rule
# below does not apply to them.
$testOnly = [System.Collections.Generic.HashSet[string]]::new()
$changed = $true
while ($changed) {
    $changed = $false
    foreach ($file in $files) {
        $relative = Get-CoreRelativePath $file.FullName
        if ($testOnly.Contains($relative)) { continue }
        $stem = [System.IO.Path]::GetFileNameWithoutExtension($file.FullName)
        $declarations = @(Get-ChildItem -LiteralPath $file.DirectoryName -Filter "*.rs" -File)
        $parentDir = Split-Path -Parent $file.DirectoryName
        if ($parentDir) {
            $declarations += @(Get-ChildItem -LiteralPath $parentDir -Filter "*.rs" -File -ErrorAction SilentlyContinue)
        }
        foreach ($declaration in $declarations) {
            if ($declaration.FullName -eq $file.FullName) { continue }
            $text = $textByPath[$declaration.FullName]
            $module = "\bmod\s+$([regex]::Escape($stem))\s*;"
            if ($text -match "(?s)#\[cfg\(test\)\][^;]{0,200}?$module") {
                $testOnly.Add($relative) | Out-Null
                $changed = $true
                break
            }
            if ($text -match $module -and $testOnly.Contains((Get-CoreRelativePath $declaration.FullName))) {
                $testOnly.Add($relative) | Out-Null
                $changed = $true
                break
            }
        }
    }
}

$violations = [System.Collections.Generic.List[string]]::new()
foreach ($file in $files) {
    $text = $textByPath[$file.FullName]
    $relative = Get-CoreRelativePath $file.FullName

    if ($text -match "tokio::runtime::Runtime::new\s*\(") {
        $violations.Add("${relative}: private Tokio Runtime::new is forbidden")
    }

    if ($relative -ne "crates/openless-core/src/config.rs" -and
        $text -match "tokio::runtime::Handle::(?:current|try_current)\s*\(") {
        $violations.Add("${relative}: runtime Handle lookup must stay inside the host TaskSpawner")
    }

    # Production code must submit background work to the injected TaskSpawner.
    # The test modules may use #[tokio::test]/tokio::spawn to orchestrate tests.
    if ($testOnly.Contains($relative)) { continue }
    $testMarker = $text.IndexOf("#[cfg(test)]", [System.StringComparison]::Ordinal)
    $production = if ($testMarker -ge 0) { $text.Substring(0, $testMarker) } else { $text }
    if ($production -match "tokio::spawn\s*\(") {
        $violations.Add("${relative}: production tokio::spawn bypasses TaskSpawner")
    }
}

if ($violations.Count -gt 0) {
    foreach ($violation in $violations) {
        Write-Host "::error::$violation"
    }
    Write-Host "Core runtime seam gate failed: $($violations.Count) violation(s)."
    exit 1
}

Write-Output "Core runtime seam gate passed (no private runtime; production tasks use the injected TaskSpawner)."
