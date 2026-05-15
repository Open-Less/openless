param(
    [string]$SdkPath = "",
    [string]$Configuration = "debug",
    [string]$KeystorePath = "",
    [string]$KeystoreAlias = "",
    [string]$StorePass = "",
    [string]$KeyPass = ""
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
$AndroidJar = Join-Path $SdkPath "platforms\android-34\android.jar"
$Aapt2 = Join-Path $BuildTools "aapt2.exe"
$D8 = Join-Path $BuildTools "d8.bat"
$ZipAlign = Join-Path $BuildTools "zipalign.exe"
$ApkSigner = Join-Path $BuildTools "apksigner.bat"

foreach ($Path in @($AndroidJar, $Aapt2, $D8, $ZipAlign, $ApkSigner)) {
    if (-not (Test-Path $Path)) {
        throw "Required Android tool missing: $Path"
    }
}

function Invoke-Checked {
    param([scriptblock]$Command)
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code $LASTEXITCODE"
    }
}

$OutDir = Join-Path $Root "build"
$Build = Join-Path $OutDir ("work-" + [System.DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
$Gen = Join-Path $Build "gen"
$Classes = Join-Path $Build "classes"
$Dex = Join-Path $Build "dex"
$Compiled = Join-Path $Build "compiled.zip"
$Unsigned = Join-Path $Build "unsigned.apk"
$Aligned = Join-Path $Build "aligned.apk"
$Signed = Join-Path $Build "signed.apk"
$FinalApk = Join-Path $OutDir "OpenLessAndroid-$Configuration.apk"
$Keystore = Join-Path $OutDir "debug.keystore"
$UseCustomKeystore = $Configuration -ieq "release" -and $KeystorePath -and $KeystoreAlias -and $StorePass -and $KeyPass

New-Item -ItemType Directory -Force -Path $OutDir, $Gen, $Classes, $Dex | Out-Null

Invoke-Checked { & $Aapt2 compile --dir (Join-Path $Root "res") -o $Compiled }
Invoke-Checked { & $Aapt2 link `
    -o $Unsigned `
    -I $AndroidJar `
    --manifest (Join-Path $Root "AndroidManifest.xml") `
    -R $Compiled `
    --java $Gen `
    --version-code $VersionInfo.VersionCode `
    --version-name $VersionInfo.VersionName `
    --auto-add-overlay }

$Sources = Get-ChildItem -Path (Join-Path $Root "src") -Recurse -Filter *.java
$Generated = Get-ChildItem -Path $Gen -Recurse -Filter *.java
$SourceList = Join-Path $Build "sources.txt"
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllLines($SourceList, (($Sources + $Generated) | ForEach-Object { $_.FullName }), $Utf8NoBom)

Invoke-Checked { javac -encoding UTF-8 -source 8 -target 8 -classpath $AndroidJar -d $Classes "@$SourceList" }
Invoke-Checked { & $D8 --classpath $AndroidJar --output $Dex (Get-ChildItem -Path $Classes -Recurse -Filter *.class | ForEach-Object { $_.FullName }) }
Copy-Item $Unsigned $FinalApk
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::Open($FinalApk, "Update")
try {
    $existing = $zip.GetEntry("classes.dex")
    if ($existing) { $existing.Delete() }
    [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, (Join-Path $Dex "classes.dex"), "classes.dex") | Out-Null
} finally {
    $zip.Dispose()
}

Invoke-Checked { & $ZipAlign -f 4 $FinalApk $Aligned }

if ($UseCustomKeystore) {
    Invoke-Checked { & $ApkSigner sign `
        --ks $KeystorePath `
        --ks-key-alias $KeystoreAlias `
        --ks-pass ("pass:" + $StorePass) `
        --key-pass ("pass:" + $KeyPass) `
        --out $Signed `
        $Aligned }
} else {
    if (-not (Test-Path $Keystore)) {
        keytool -genkeypair `
            -keystore $Keystore `
            -storepass android `
            -keypass android `
            -alias androiddebugkey `
            -keyalg RSA `
            -keysize 2048 `
            -validity 10000 `
            -dname "CN=Android Debug,O=Android,C=US" | Out-Null
    }

    Invoke-Checked { & $ApkSigner sign `
        --ks $Keystore `
        --ks-pass pass:android `
        --key-pass pass:android `
        --out $Signed `
        $Aligned }
}

Invoke-Checked { & $ApkSigner verify $Signed }
Copy-Item -LiteralPath $Signed -Destination $FinalApk -Force
Write-Host "Built $FinalApk"
Write-Host ("Version " + $VersionInfo.VersionName + " (" + $VersionInfo.VersionCode + ")")
