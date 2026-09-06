[CmdletBinding()]
param(
    [string]$BaselinePath,
    [string]$TauriLibPath,
    [string]$CoreEventsPath,
    [string]$ContractFixturePath
)

$ErrorActionPreference = "Stop"
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($BaselinePath)) {
    $BaselinePath = Join-Path $scriptRoot "../../../docs/linux-egui-command-event-baseline.json"
}
if ([string]::IsNullOrWhiteSpace($TauriLibPath)) {
    $TauriLibPath = Join-Path $scriptRoot "../src-tauri/src/lib.rs"
}
if ([string]::IsNullOrWhiteSpace($CoreEventsPath)) {
    $CoreEventsPath = Join-Path $scriptRoot "../crates/openless-core/src/events.rs"
}
if ([string]::IsNullOrWhiteSpace($ContractFixturePath)) {
    $ContractFixturePath = Join-Path $scriptRoot "../contract/backend-2.0.json"
}

if (-not (Test-Path -LiteralPath $BaselinePath)) {
    throw "baseline file not found: $BaselinePath"
}
if (-not (Test-Path -LiteralPath $TauriLibPath)) {
    throw "Tauri lib.rs not found: $TauriLibPath"
}
if (-not (Test-Path -LiteralPath $CoreEventsPath)) {
    throw "core events.rs not found: $CoreEventsPath"
}
if (-not (Test-Path -LiteralPath $ContractFixturePath)) {
    throw "contract fixture not found: $ContractFixturePath"
}

$baseline = Get-Content -LiteralPath $BaselinePath -Raw | ConvertFrom-Json
$fixture = Get-Content -LiteralPath $ContractFixturePath -Raw | ConvertFrom-Json
if ($baseline.contractVersion -ne "2.0.0" -or $fixture.contractVersion -ne $baseline.contractVersion) {
    throw "contract version must be 2.0.0 in baseline and canonical fixture"
}
$expected = @($baseline.commands | Sort-Object -Unique)
$source = Get-Content -LiteralPath $TauriLibPath -Raw
$actual = @(
    [regex]::Matches(
        $source,
        '(?m)^\s*(?:(?:\$crate::)?(?:commands|coding_agent::commands)::|\$crate::)([A-Za-z0-9_]+),'
    ) | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
)

$missing = @($expected | Where-Object { $_ -notin $actual })
$added = @($actual | Where-Object { $_ -notin $expected })
$duplicateCount = @($baseline.commands).Count - $expected.Count

if ($baseline.counts.tauriCommandsObserved -ne $expected.Count) {
    throw "baseline count mismatch: counts.tauriCommandsObserved=$($baseline.counts.tauriCommandsObserved), commands=$($expected.Count)"
}
if ($duplicateCount -ne 0) {
    throw "baseline contains duplicate command names: $duplicateCount"
}
if ($missing.Count -gt 0 -or $added.Count -gt 0) {
    if ($missing.Count -gt 0) {
        Write-Error ("commands missing from source: " + ($missing -join ", "))
    }
    if ($added.Count -gt 0) {
        Write-Error ("commands missing from baseline: " + ($added -join ", "))
    }
    exit 1
}

$legacyEvents = @($baseline.events | Sort-Object -Unique)
if ($baseline.counts.legacyEventsObserved -ne $legacyEvents.Count) {
    throw "baseline count mismatch: counts.legacyEventsObserved=$($baseline.counts.legacyEventsObserved), events=$($legacyEvents.Count)"
}
if (@($baseline.events).Count -ne $legacyEvents.Count) {
    throw "baseline contains duplicate legacy event names"
}

$ownedEvents = @(
    @($baseline.eventOwnership.coreSemantic.PSObject.Properties.Name)
    @($baseline.eventOwnership.tauriHost.PSObject.Properties.Name)
    @($baseline.eventOwnership.migrationRequired.PSObject.Properties.Name)
) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
$ownedUnique = @($ownedEvents | Sort-Object -Unique)
$unclassified = @($legacyEvents | Where-Object { $_ -notin $ownedUnique })
$unknownOwned = @($ownedUnique | Where-Object { $_ -notin $legacyEvents })
if ($ownedEvents.Count -ne $ownedUnique.Count) {
    throw "legacy event ownership contains duplicate classifications"
}
if ($unclassified.Count -gt 0 -or $unknownOwned.Count -gt 0) {
    if ($unclassified.Count -gt 0) {
        Write-Error ("legacy events without ownership: " + ($unclassified -join ", "))
    }
    if ($unknownOwned.Count -gt 0) {
        Write-Error ("owned events missing from baseline: " + ($unknownOwned -join ", "))
    }
    exit 1
}

$coreSource = Get-Content -LiteralPath $CoreEventsPath -Raw
$enumMatch = [regex]::Match(
    $coreSource,
    '(?s)pub enum BackendEventKind\s*\{(?<body>.*?)\n\}'
)
if (-not $enumMatch.Success) {
    throw "BackendEventKind enum not found in $CoreEventsPath"
}
$coreActual = @(
    [regex]::Matches($enumMatch.Groups['body'].Value, '(?m)^\s*([A-Z][A-Za-z0-9]+)(?:\(|,)') |
        ForEach-Object {
            ([regex]::Replace($_.Groups[1].Value, '(?<!^)([A-Z])', '_$1')).ToLowerInvariant()
        } |
        Sort-Object -Unique
)
$coreExpected = @($baseline.coreEventKinds | Sort-Object -Unique)
$coreMissing = @($coreExpected | Where-Object { $_ -notin $coreActual })
$coreAdded = @($coreActual | Where-Object { $_ -notin $coreExpected })
if ($baseline.counts.coreEventKindsDefined -ne $coreExpected.Count) {
    throw "baseline count mismatch: counts.coreEventKindsDefined=$($baseline.counts.coreEventKindsDefined), coreEventKinds=$($coreExpected.Count)"
}
if (@($baseline.coreEventKinds).Count -ne $coreExpected.Count) {
    throw "baseline contains duplicate core event kinds"
}
if ($coreMissing.Count -gt 0 -or $coreAdded.Count -gt 0) {
    if ($coreMissing.Count -gt 0) {
        Write-Error ("core event kinds missing from source: " + ($coreMissing -join ", "))
    }
    if ($coreAdded.Count -gt 0) {
        Write-Error ("core event kinds missing from baseline: " + ($coreAdded -join ", "))
    }
    exit 1
}

$fixtureKinds = @($fixture.backendEvent.kinds | Sort-Object -Unique)
$fixtureKindDiff = @(Compare-Object $coreExpected $fixtureKinds)
if (@($fixture.backendEvent.kinds).Count -ne $fixtureKinds.Count -or $fixtureKindDiff.Count -ne 0) {
    throw "canonical fixture backend event kinds do not match BackendEventKind"
}
$fixtureSampleKinds = @($fixture.backendEvent.samples.PSObject.Properties.Name | Sort-Object -Unique)
if ((Compare-Object $coreExpected $fixtureSampleKinds).Count -ne 0) {
    throw "canonical fixture must contain one JSON sample for every BackendEventKind"
}

$startupFields = @($fixture.startupSnapshot.fields | Sort-Object -Unique)
if ((Compare-Object @("backend", "contractVersion") $startupFields).Count -ne 0) {
    throw "canonical StartupSnapshot fields must be backend and contractVersion"
}
if ($fixture.startupSnapshot.sample.contractVersion -ne $baseline.contractVersion) {
    throw "canonical StartupSnapshot sample has the wrong contract version"
}

$fieldGroups = @(
    @($fixture.startupSnapshot.fields),
    @($fixture.backendEvent.fields),
    @($fixture.androidJni.fields),
    @($fixture.linuxFacade.startupFields),
    @($fixture.linuxFacade.eventFields)
)
foreach ($field in ($fieldGroups | ForEach-Object { $_ })) {
    if ($field -notmatch '^[a-z][A-Za-z0-9]*$') {
        throw "canonical field is not camelCase: $field"
    }
}
function Assert-CamelCaseObjectFields {
    param(
        [AllowNull()]$Value,
        [string]$Path = "root"
    )
    if ($null -eq $Value) {
        return
    }
    if ($Value -is [System.Management.Automation.PSCustomObject]) {
        foreach ($property in $Value.PSObject.Properties) {
            if ($Path -ne "root.backendEvent.samples" -and $property.Name -notmatch '^[a-z][A-Za-z0-9]*$') {
                throw "canonical field is not camelCase: $Path.$($property.Name)"
            }
            Assert-CamelCaseObjectFields -Value $property.Value -Path "$Path.$($property.Name)"
        }
    } elseif ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) {
        foreach ($item in $Value) {
            Assert-CamelCaseObjectFields -Value $item -Path $Path
        }
    }
}
Assert-CamelCaseObjectFields -Value $fixture
if ($fixture.androidJni.sample.contractVersion -ne $baseline.contractVersion) {
    throw "canonical Android JNI sample has the wrong contract version"
}
if ((Compare-Object @("copiedFallback", "inserted", "notRequested", "pasteSent") @($fixture.enums.insertStatus | Sort-Object)).Count -ne 0) {
    throw "canonical insertStatus enum values changed"
}
if ((Compare-Object @("bad_pin", "locked", "ok") @($fixture.enums.remoteAuthResult | Sort-Object)).Count -ne 0) {
    throw "canonical remoteAuthResult enum values changed"
}

Write-Output "command/event baseline passed ($($expected.Count) commands; $(@($baseline.events).Count) legacy events; $(@($baseline.coreEventKinds).Count) core event kinds; contract $($fixture.contractVersion))."
