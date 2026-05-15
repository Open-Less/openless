param(
    [string]$SdkPath = "",
    [string]$AdbPath = "",
    [string]$Configuration = "debug",
    [switch]$BuildFirst,
    [switch]$CheckDevice
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

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$VersionInfo = & (Join-Path $Root "version.ps1")
$BuildTools = Join-Path $SdkPath "build-tools\34.0.0"
$Aapt = Join-Path $BuildTools "aapt.exe"
$ApkSigner = Join-Path $BuildTools "apksigner.bat"
$ApkPath = Join-Path $Root ("build\OpenLessAndroid-" + $Configuration + ".apk")

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

foreach ($Path in @($Aapt, $ApkSigner)) {
    if (-not (Test-Path $Path)) {
        throw "Required Android tool missing: $Path"
    }
}

if ($BuildFirst) {
    & (Join-Path $Root "build.ps1") -SdkPath $SdkPath -Configuration $Configuration
    if ($LASTEXITCODE -ne 0) {
        throw "build.ps1 failed with exit code $LASTEXITCODE"
    }
}

if (-not (Test-Path $ApkPath)) {
    throw "APK not found: $ApkPath"
}

& $ApkSigner verify $ApkPath
if ($LASTEXITCODE -ne 0) {
    throw "apksigner verify failed with exit code $LASTEXITCODE"
}

$badging = & $Aapt dump badging $ApkPath
if ($LASTEXITCODE -ne 0) {
    throw "aapt dump badging failed with exit code $LASTEXITCODE"
}

function Assert-BadgingContains {
    param(
        [string]$Needle,
        [string]$Message
    )
    if (-not ($badging -match [regex]::Escape($Needle))) {
        throw $Message
    }
}

Assert-BadgingContains "package: name='com.openless.android'" "Unexpected package name."
Assert-BadgingContains ("versionCode='" + $VersionInfo.VersionCode + "'") "Unexpected versionCode."
Assert-BadgingContains ("versionName='" + $VersionInfo.VersionName + "'") "Unexpected versionName."
Assert-BadgingContains "launchable-activity: name='com.openless.android.MainActivity'" "MainActivity is not launchable."
Assert-BadgingContains "uses-permission: name='android.permission.RECORD_AUDIO'" "Missing RECORD_AUDIO permission."
Assert-BadgingContains "uses-permission: name='android.permission.SYSTEM_ALERT_WINDOW'" "Missing SYSTEM_ALERT_WINDOW permission."
Assert-BadgingContains "uses-permission: name='android.permission.FOREGROUND_SERVICE_MICROPHONE'" "Missing FOREGROUND_SERVICE_MICROPHONE permission."
Assert-BadgingContains "uses-permission: name='android.permission.POST_NOTIFICATIONS'" "Missing POST_NOTIFICATIONS permission."
Assert-BadgingContains "provides-component:'ime'" "IME component is missing."

Write-Host "verify.ps1 passed:"
Write-Host "  APK: $ApkPath"
Write-Host "  Package: com.openless.android"
Write-Host ("  Version: " + $VersionInfo.VersionCode + " / " + $VersionInfo.VersionName)
Write-Host "  Launchable activity: com.openless.android.MainActivity"
Write-Host "  IME component: present"
if ($AdbPath -and (Test-Path $AdbPath)) {
    Write-Host "  ADB: $AdbPath"
    if ($CheckDevice) {
        $deviceLines = & $AdbPath devices | Select-Object -Skip 1 | Where-Object { $_.Trim() -ne "" }
        if ($deviceLines.Count -eq 0) {
            Write-Host "  Device: none attached"
        } else {
            Write-Host "  Device:"
            foreach ($line in $deviceLines) {
                Write-Host ("    " + $line.Trim())
            }
        }
    }
}
