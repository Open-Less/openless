# android-ui-verify.ps1 — Trigger CI Android debug APK build (optional), install via adb,
# capture logcat, and save UI screenshots. Artifacts stay local; do not commit outputs.
param(
    [string]$Repo = "HKLHaoBin/openless",
    [string]$Branch = "feat/mobile-portrait-ui",
    [string]$Workflow = "Android APK (debug)",
    [string]$ArtifactName = "openless-android-debug-arm64-v8a",
    [switch]$SkipBuild,
    [int]$StartupWaitSeconds = 8,
    [string]$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\\..\\..")).Path
)

$ErrorActionPreference = "Stop"
Set-Location $ProjectRoot

function Require-Command($name) {
    if (-not (Get-Command $name -ErrorAction SilentlyContinue)) {
        throw "Required command not found: $name"
    }
}

Require-Command adb
if (-not $SkipBuild) {
    Require-Command gh
}

$devices = adb devices | Select-String "device$"
if (-not $devices) {
    throw "No adb device in 'device' state. Connect phone and enable USB debugging."
}

$ciDir = Join-Path $ProjectRoot "ci-artifacts"
$shotDir = Join-Path $ProjectRoot "android-screenshots"
New-Item -ItemType Directory -Force -Path $ciDir, $shotDir | Out-Null

if (-not $SkipBuild) {
    Write-Host "Triggering workflow '$Workflow' on $Repo ref $Branch ..."
    gh workflow run $Workflow --repo $Repo --ref $Branch
    Start-Sleep -Seconds 5
    $runLine = gh run list --repo $Repo --workflow $Workflow --limit 1 --json databaseId,status --jq '.[0]'
    $run = $runLine | ConvertFrom-Json
    if (-not $run.databaseId) {
        throw "Could not resolve latest workflow run id."
    }
    $runId = $run.databaseId
    Write-Host "Watching run $runId ..."
    gh run watch $runId --repo $Repo --exit-status
    if (Test-Path $ciDir) {
        Get-ChildItem $ciDir -File | Remove-Item -Force
    }
    gh run download $runId --repo $Repo --name $ArtifactName --dir $ciDir
}

$apk = Get-ChildItem -Path $ciDir -Filter "OpenLess-android-debug-*.apk" -File | Select-Object -First 1
if (-not $apk) {
    throw "No APK found in $ciDir. Run without -SkipBuild or place an APK there."
}

Write-Host "Installing $($apk.FullName) ..."
adb logcat -c
adb shell pm uninstall --user 0 com.openless.app 2>$null
if ($LASTEXITCODE -ne 0) {
    adb uninstall com.openless.app 2>$null
}
adb install $apk.FullName

Write-Host "Starting MainActivity ..."
adb shell am start -n com.openless.app/.MainActivity
Start-Sleep -Seconds $StartupWaitSeconds

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$crashOnly = Join-Path $ProjectRoot "android-crash-only.log"
$fullLog = Join-Path $ProjectRoot "android-crash.log"
adb logcat -b crash -d -v time | Set-Content -Path $crashOnly -Encoding utf8
adb logcat -d -v time | Set-Content -Path $fullLog -Encoding utf8
adb shell dumpsys package com.openless.app | Set-Content -Path (Join-Path $ProjectRoot "openless-package.txt") -Encoding utf8

function Capture-Screen([string]$label) {
    $out = Join-Path $shotDir "openless-screenshot-$label-$stamp.png"
    $tmp = Join-Path $env:TEMP "openless-screencap-$label.png"
    adb shell screencap -p /sdcard/openless-ui-$label.png
    adb pull /sdcard/openless-ui-$label.png $tmp | Out-Null
    if (Test-Path $tmp) {
        Move-Item -Force $tmp $out
        Write-Host "Screenshot: $out"
    }
}

Capture-Screen "overview"
Write-Host "Capture additional screens manually or extend script with adb input tap coordinates."
Write-Host "Logs: $fullLog"
Write-Host "Filter: Select-String -Path '$fullLog' -Pattern 'FATAL EXCEPTION','panic','UnsatisfiedLinkError'"
