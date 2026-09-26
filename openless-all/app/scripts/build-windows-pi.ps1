param(
  [ValidateSet('nsis', 'msi', 'all')]
  [string]$Bundle = 'nsis'
)

$ErrorActionPreference = 'Stop'
$appDirectory = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) {
  throw '请先安装 Visual Studio C++ Build Tools 与 Windows SDK。'
}
$vsInstallation = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsInstallation) { throw '找不到 MSVC C++ 编译工具链。' }
Import-Module (Join-Path $vsInstallation 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vsInstallation -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'

Push-Location $appDirectory
try {
  if (-not (Test-Path -LiteralPath 'node_modules')) {
    npm ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci 失败。' }
  }
  foreach ($imeTarget in @(
    @{ Platform = 'x64'; Folder = 'x64'; Variable = 'OPENLESS_IME_DLL_X64' },
    @{ Platform = 'Win32'; Folder = 'x86'; Variable = 'OPENLESS_IME_DLL_X86' }
  )) {
    $imeOutput = Join-Path $appDirectory "src-tauri\target\windows-ime-msvc\$($imeTarget.Folder)\Release"
    $imeIntermediate = Join-Path $appDirectory "src-tauri\target\windows-ime-msvc\obj\$($imeTarget.Folder)\Release"
    & (Join-Path $PSScriptRoot 'windows-ime-build.ps1') -Configuration Release -Platform $imeTarget.Platform -OutputDirectory $imeOutput -IntermediateDirectory $imeIntermediate
    if ($LASTEXITCODE -ne 0) { throw "IME $($imeTarget.Platform) 构建失败。" }
    $imeDll = (Resolve-Path -LiteralPath (Join-Path $imeOutput 'OpenLessIme.dll')).Path
    Set-Item -LiteralPath "Env:$($imeTarget.Variable)" -Value $imeDll
  }
  # Tauri beforeBuildCommand prepares the bundled PI and builds the frontend.
  npm run tauri -- build --target x86_64-pc-windows-msvc --bundles $Bundle
  if ($LASTEXITCODE -ne 0) { throw 'OpenLess Windows 安装包构建失败。' }
  Write-Host "安装包目录：$(Join-Path $appDirectory 'src-tauri\target\x86_64-pc-windows-msvc\release\bundle')"
} finally {
  Pop-Location
}
