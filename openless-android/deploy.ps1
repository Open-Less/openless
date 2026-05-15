param(
    [string]$SdkPath = "",
    [string]$AdbPath = "",
    [string]$Configuration = "debug",
    [switch]$BuildFirst,
    [switch]$LaunchAfterInstall
)

$ErrorActionPreference = "Stop"

if (-not $SdkPath) {
    if ($env:ANDROID_HOME) {
        $SdkPath = $env:ANDROID_HOME
    } elseif ($env:ANDROID_SDK_ROOT) {
        $SdkPath = $env:ANDROID_SDK_ROOT
    } elseif (Test-Path "$env:LOCALAPPDATA\Android\Sdk") {
        $SdkPath = "$env:LOCALAPPDATA\Android\Sdk"
    } else {
        throw "Android SDK not found. Pass -SdkPath or set ANDROID_HOME."
    }
}

if (-not $AdbPath) {
    $adbCandidates = @(
        (Join-Path $SdkPath "platform-tools\adb.exe"),
        "$env:LOCALAPPDATA\Android\Sdk\platform-tools\adb.exe"
    )
    foreach ($candidate in $adbCandidates) {
        if ($candidate -and (Test-Path $candidate)) {
            $AdbPath = $candidate
            break
        }
    }
}

if (-not $AdbPath -or -not (Test-Path $AdbPath)) {
    throw "adb.exe not found. Pass -AdbPath or install Android platform-tools."
}

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$ApkPath = Join-Path $Root ("build\OpenLessAndroid-" + $Configuration + ".apk")

if ($BuildFirst) {
    & (Join-Path $Root "build.ps1") -SdkPath $SdkPath -Configuration $Configuration
    if ($LASTEXITCODE -ne 0) {
        throw "build.ps1 failed with exit code $LASTEXITCODE"
    }
}

if (-not (Test-Path $ApkPath)) {
    throw "APK not found: $ApkPath"
}

$deviceLines = & $AdbPath devices | Select-Object -Skip 1 | Where-Object { $_.Trim() -ne "" }
if ($deviceLines.Count -eq 0) {
    throw "No adb device attached."
}

Write-Host "Deploying $ApkPath"
foreach ($line in $deviceLines) {
    Write-Host ("  Device: " + $line.Trim())
}

& $AdbPath install -r $ApkPath
if ($LASTEXITCODE -ne 0) {
    throw "adb install failed with exit code $LASTEXITCODE"
}

Write-Host "Install complete: com.openless.android"

if ($LaunchAfterInstall) {
    & $AdbPath shell am start -n com.openless.android/.MainActivity
    if ($LASTEXITCODE -ne 0) {
        throw "adb shell am start failed with exit code $LASTEXITCODE"
    }
    Write-Host "Launch requested: com.openless.android/.MainActivity"
}
