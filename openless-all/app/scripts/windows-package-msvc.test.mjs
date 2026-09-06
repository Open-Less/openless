import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const appRoot = join(scriptsDir, "..");
const repoRoot = join(appRoot, "..", "..");
const scriptPath = join(scriptsDir, "windows-package-msvc.ps1");
const launcherPath = join(scriptsDir, "windows-package-msvc.cmd");
const ciWorkflowPath = join(repoRoot, ".github", "workflows", "release-tauri.yml");
const imeBuildPath = join(scriptsDir, "windows-ime-build.ps1");
const imeInstallSmokePath = join(scriptsDir, "windows-ime-install-smoke.ps1");
const imeRegisterPath = join(scriptsDir, "windows-ime-register.ps1");
const imeUnregisterPath = join(scriptsDir, "windows-ime-unregister.ps1");
const imeSolutionPath = join(appRoot, "windows-ime", "OpenLessIme.sln");
const imeProjectPath = join(appRoot, "windows-ime", "OpenLessIme.vcxproj");
const imeEditSessionPath = join(appRoot, "windows-ime", "src", "edit_session.cpp");
const imeTextServicePath = join(appRoot, "windows-ime", "src", "text_service.cpp");
const tauriConfigPath = join(appRoot, "src-tauri", "tauri.conf.json");
const nsisHookPath = join(appRoot, "src-tauri", "nsis", "openless-ime-hooks.nsh");
const wixFragmentPath = join(appRoot, "src-tauri", "wix", "openless-ime.wxs");

const script = readFileSync(scriptPath, "utf8");
const launcher = readFileSync(launcherPath, "utf8");
const ciWorkflow = readFileSync(ciWorkflowPath, "utf8");
const imeBuild = readFileSync(imeBuildPath, "utf8");
const imeInstallSmoke = readFileSync(imeInstallSmokePath, "utf8");
const imeRegister = readFileSync(imeRegisterPath, "utf8");
const imeUnregister = readFileSync(imeUnregisterPath, "utf8");
const imeSolution = readFileSync(imeSolutionPath, "utf8");
const imeProject = readFileSync(imeProjectPath, "utf8");
const imeEditSession = readFileSync(imeEditSessionPath, "utf8");
const imeTextService = readFileSync(imeTextServicePath, "utf8");
const tauriConfig = JSON.parse(readFileSync(tauriConfigPath, "utf8"));
const nsisHook = readFileSync(nsisHookPath, "utf8");
const wixFragment = readFileSync(wixFragmentPath, "utf8");

const requiredFragments = [
  "Install-RustMsvcToolchain",
  "https://win.rustup.rs/x86_64",
  "stable-x86_64-pc-windows-msvc",
  "Find-VsDevCmd",
  "VsDevCmd.bat",
  "npm.cmd ci",
  "windows-ime-build.ps1",
  "OPENLESS_IME_DLL_X64",
  "OPENLESS_IME_DLL_X86",
  "OpenLessIme.dll",
  "tauri build -- --target x86_64-pc-windows-msvc --bundles msi",
  "Repair-TauriMsiBundle",
  "light.exe",
  "main.wixobj",
  "openless-ime.wixobj",
  "locale.wxl",
  "WebView2Loader.dll",
  "Compress-Archive",
  "Get-FileHash -Algorithm SHA256",
];

for (const fragment of requiredFragments) {
  assert.match(script, new RegExp(fragment.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")), `missing ${fragment}`);
}

assert.match(script, /\[switch\]\$SkipRustInstall/, "script should support opting out of Rust installation");
assert.match(script, /\[switch\]\$SkipNpmCi/, "script should support reusing existing node_modules");
assert.match(script, /\[switch\]\$CleanArtifacts/, "script should support cleaning the output directory");
assert.doesNotMatch(script, /WixTools314/, "MSVC packaging must not hard-code a single Tauri WiX tools version");
assert.doesNotMatch(ciWorkflow, /WixTools314/, "CI MSI repair must not hard-code a single Tauri WiX tools version");
assert.match(script, /cmd\.exe \/V:ON \/d \/c/, "MSVC packaging must preserve the VsDevCmd environment with delayed expansion");
assert.match(script, /PATH=\$CargoBin;!PATH!/, "MSVC packaging must expand PATH after VsDevCmd runs");
assert.doesNotMatch(script, /PATH=\$CargoBin;%PATH%/, "MSVC packaging must not eagerly expand PATH before VsDevCmd");
assert.match(script, /-Filter "WixTools\*"/, "MSVC packaging should discover Tauri WiX tools by WixTools* glob");
assert.match(ciWorkflow, /WixTools\*\\light\.exe/, "CI MSI repair should discover Tauri WiX tools by WixTools* glob");

assert.match(imeBuild, /\[string\]\$OutputDirectory/, "IME build should support a package-specific output directory");
assert.match(imeBuild, /\[string\]\$IntermediateDirectory/, "IME build should support a package-specific intermediate directory");
assert.match(imeBuild, /\[ValidateSet\("x64", "Win32"\)\]/, "IME build should support x64 and Win32 platforms");
assert.match(imeBuild, /\/p:Platform=\$Platform/, "IME build should pass Platform to MSBuild");
assert.match(imeBuild, /\$defaultOutputDirectory = Join-Path \$appRoot "windows-ime\\\$defaultPlatformFolder\\\$Configuration"/, "IME build should force stable default OutDir per platform");
assert.match(imeBuild, /\/p:OutDir=/, "IME build should pass OutDir to MSBuild");
assert.match(imeBuild, /\/p:IntDir=/, "IME build should pass IntDir to MSBuild");
assert.match(imeRegister, /windows-ime-build\.ps1/, "IME register should build before registering");
assert.doesNotMatch(imeRegister, /if \(-not \(Test-Path \$dll\)\)/, "IME register must rebuild stale DLLs, not only missing DLLs");
assert.match(imeRegister, /windows-ime-register/, "IME register should use a side-by-side staging output to avoid locked registered DLLs");
assert.match(imeRegister, /Get-Date/, "IME register should create a fresh staging output for each registration run");
assert.match(imeRegister, /\$PID/, "IME register should include the process id in the staging output to avoid path reuse");
assert.match(imeRegister, /-OutputDirectory/, "IME register should pass a staging output directory to the build script");
assert.match(imeRegister, /-IntermediateDirectory/, "IME register should pass a staging intermediate directory to the build script");
assert.match(imeRegister, /active-registration\.json/, "IME register should persist the staged DLL paths it registered");
assert.match(imeUnregister, /active-registration\.json/, "IME unregister should read the registered staged DLL manifest");
assert.match(imeUnregister, /windows-ime-register/, "IME unregister should target the same staging root used by register");
assert.match(imeUnregister, /ConvertFrom-Json/, "IME unregister should parse persisted registered DLL paths");
assert.doesNotMatch(imeUnregister, /windows-ime\\\$folder\\\$Configuration\\OpenLessIme\.dll/, "IME unregister must not only derive legacy build-output DLL paths");

assert.deepEqual(tauriConfig.bundle.windows.wix.fragmentPaths, ["wix/openless-ime.wxs"]);
assert.deepEqual(tauriConfig.bundle.windows.wix.componentRefs, [
  "OpenLessImeDllX64Component",
  "OpenLessImeDllX86Component",
]);
assert.equal(tauriConfig.bundle.windows.nsis.installMode, "perMachine", "NSIS must force a machine-wide install because TSF registration is machine-wide");
assert.equal(tauriConfig.bundle.windows.nsis.installerHooks, "nsis/openless-ime-hooks.nsh", "NSIS must install and register the TSF DLLs");

assert.match(imeSolution, /Release\|Win32/, "IME solution should include a Win32 Release configuration");
assert.match(imeProject, /Release\|Win32/, "IME project should include a Win32 Release configuration");
assert.match(imeProject, /<AdditionalOptions>\/utf-8 %\(AdditionalOptions\)<\/AdditionalOptions>/, "IME project must decode UTF-8 source independently of the runner code page");
assert.match(imeTextService, /TF_E_SYNCHRONOUS/, "IME should detect hosts like Word that reject synchronous edit sessions");
assert.match(imeTextService, /TF_ES_ASYNC \| TF_ES_READWRITE/, "IME should retry Word-hosted commits with an async edit session");
assert.match(imeTextService, /WaitForSingleObject/, "IME pipe submit should wait for async edit-session completion");
assert.match(imeEditSession, /SetEvent/, "IME edit session should signal async completion back to the pipe submitter");
assert.match(imeEditSession, /Collapse\(edit_cookie, TF_ANCHOR_END\)/, "IME should collapse the committed range to its end after insertion");
assert.match(imeEditSession, /SetSelection\(edit_cookie, 1, &selection\)/, "IME should move the caret to the end of inserted text");
assert.match(imeEditSession, /TF_AE_END/, "IME should make the end of the committed text the active selection end");

assert.match(wixFragment, /DirectoryRef Id="INSTALLDIR"/, "WiX fragment should install into the app directory");
assert.match(wixFragment, /Component Id="OpenLessImeDllX64Component"/, "WiX fragment should define the x64 TSF DLL component");
assert.match(wixFragment, /Component Id="OpenLessImeDllX86Component"/, "WiX fragment should define the x86 TSF DLL component");
assert.match(wixFragment, /Source="src-tauri\\target\\windows-ime-msvc\\x64\\Release\\OpenLessIme\.dll"/, "WiX fragment should consume the package-built x64 IME DLL");
assert.match(wixFragment, /Source="src-tauri\\target\\windows-ime-msvc\\x86\\Release\\OpenLessIme\.dll"/, "WiX fragment should consume the package-built x86 IME DLL");
assert.match(wixFragment, /regsvr32\.exe/, "MSI should register and unregister the TSF DLL");
assert.match(wixFragment, /\[System64Folder\]regsvr32\.exe/, "MSI should register the x64 IME with 64-bit regsvr32");
assert.match(wixFragment, /\[WindowsFolder\]SysWOW64\\regsvr32\.exe/, "MSI should register the x86 IME with 32-bit regsvr32");
assert.match(wixFragment, /RegisterOpenLessImeX64/, "MSI should register x64 OpenLess IME during install");
assert.match(wixFragment, /RegisterOpenLessImeX86/, "MSI should register x86 OpenLess IME during install");
assert.match(wixFragment, /UnregisterOpenLessImeX64/, "MSI should unregister x64 OpenLess IME during uninstall");
assert.match(wixFragment, /UnregisterOpenLessImeX86/, "MSI should unregister x86 OpenLess IME during uninstall");

assert.match(nsisHook, /NSIS_HOOK_PREINSTALL/, "NSIS should copy IME DLLs before install completes");
assert.match(nsisHook, /NSIS_HOOK_POSTINSTALL/, "NSIS should register IME DLLs after files are installed");
assert.match(nsisHook, /NSIS_HOOK_PREUNINSTALL/, "NSIS should unregister IME DLLs before uninstall removes them");
assert.match(nsisHook, /OPENLESS_IME_STAGE_AND_REPLACE "x64" "OPENLESS_IME_DLL_X64"/, "NSIS should consume the CI-built x64 IME DLL");
assert.match(nsisHook, /OPENLESS_IME_STAGE_AND_REPLACE "x86" "OPENLESS_IME_DLL_X86"/, "NSIS should consume the CI-built x86 IME DLL");
assert.match(nsisHook, /SetOutPath "\$INSTDIR\\windows-ime\\\$\{PLATFORM_DIR\}"/, "NSIS should install the IME DLL beside the app by platform");
assert.match(nsisHook, /File \/oname=OpenLessIme\.dll\.new "\$%\$\{ENV_VAR\}%"/, "NSIS should embed OpenLessIme.dll in the installer");
assert.match(nsisHook, /Sysnative\\regsvr32\.exe/, "NSIS should use 64-bit regsvr32 for the x64 IME");
assert.match(nsisHook, /SysWOW64\\regsvr32\.exe/, "NSIS should use 32-bit regsvr32 for the x86 IME");
assert.match(nsisHook, /System32\\regsvr32\.exe[\s\S]*windows-ime\\x86\\OpenLessIme\.dll/, "NSIS should use System32 regsvr32 for the x86 IME on 32-bit Windows");
assert.match(nsisHook, /Abort/, "NSIS install should fail if TSF registration fails");
assert.match(nsisHook, /OPENLESS_IME_ABORT_IF_FAILED \$0 "x64 registration"/, "NSIS install should fail if x64 TSF registration fails");
assert.match(nsisHook, /OPENLESS_IME_ABORT_IF_FAILED \$0 "x86 registration"/, "NSIS install should fail if x86 TSF registration fails");
assert.match(nsisHook, /OPENLESS_IME_REGISTER_X86[\s\S]*\$\{If\} \$0 != 0[\s\S]*StrCpy \$1 \$0[\s\S]*OPENLESS_IME_UNREGISTER_X64[\s\S]*StrCpy \$0 \$1[\s\S]*OPENLESS_IME_ABORT_IF_FAILED \$0 "x86 registration"/, "NSIS install should roll back x64 registration before aborting on x86 registration failure");
assert.doesNotMatch(nsisHook, /OPENLESS_IME_ABORT_IF_FAILED \$0 "x64 unregistration"/, "NSIS uninstall should not fail if x64 TSF unregistration fails");
assert.doesNotMatch(nsisHook, /OPENLESS_IME_ABORT_IF_FAILED \$0 "x86 unregistration"/, "NSIS uninstall should not fail if x86 TSF unregistration fails");
assert.match(nsisHook, /OpenLess x64 TSF IME unregister exit code \$0/, "NSIS uninstall should log x64 TSF unregistration failures");
assert.match(nsisHook, /OpenLess x86 TSF IME unregister exit code \$0/, "NSIS uninstall should log x86 TSF unregistration failures");

assert.match(imeInstallSmoke, /\[ValidateSet\("nsis", "msi"\)\]/, "install smoke should support both Windows installers");
assert.match(imeInstallSmoke, /Join-ProcessArguments/, "install smoke should quote process arguments before Start-Process");
assert.match(imeInstallSmoke, /\$commandLine = Join-ProcessArguments \$ArgumentList/, "install smoke should build a single quoted command line");
assert.match(imeInstallSmoke, /Start-Process -FilePath \$FilePath -ArgumentList \$commandLine/, "install smoke should pass a single quoted command line to Start-Process");
assert.match(imeInstallSmoke, /OpenLessImeSubmit/, "install smoke should preserve TSF backend context");
assert.match(imeInstallSmoke, /Software\\Classes\\CLSID\\\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D\}\\InprocServer32/, "install smoke should check x64 COM registration");
assert.match(imeInstallSmoke, /Software\\WOW6432Node\\Classes\\CLSID\\\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D\}\\InprocServer32/, "install smoke should check x86 COM registration");
assert.match(imeInstallSmoke, /LanguageProfile\\0x00000804\\\{9B5F5E04-23F6-47DA-9A26-D221F6C3F02E\}/, "install smoke should check the TSF language profile");
assert.match(imeInstallSmoke, /Category\\Category\\\{34745C63-B2F0-4784-8B67-5E12C8701A31\}/, "install smoke should check the keyboard TSF category");
assert.match(imeInstallSmoke, /foreach \(\$key in \$ExpectedBackendKeys\) \{[\s\S]*Assert-RegistryKey -View Registry64 -SubKey \$key[\s\S]*\}/, "install smoke should assert every backend-required registry key exists");
assert.doesNotMatch(imeInstallSmoke, /foreach \(\$key in \$ExpectedBackendKeys\) \{[\s\S]*Write-Host "\[trace\] backend-required key: HKLM\\\$key"[\s\S]*\}/, "install smoke must not only trace backend-required registry keys");
assert.match(ciWorkflow, /windows-ime-install-smoke\.ps1[\s\S]*-InstallerKind nsis/, "CI should install and verify the NSIS artifact");
assert.match(ciWorkflow, /windows-ime-install-smoke\.ps1[\s\S]*-InstallerKind msi/, "CI should install and verify the MSI artifact");
assert.match(ciWorkflow, /InstallerKind nsis[\s\S]*\$LASTEXITCODE -ne 0[\s\S]*NSIS installer smoke failed/, "CI should fail immediately when the NSIS smoke run fails");
assert.match(ciWorkflow, /InstallerKind msi[\s\S]*\$LASTEXITCODE -ne 0[\s\S]*MSI installer smoke failed/, "CI should fail when the MSI smoke run fails");

assert.match(launcher, /powershell\.exe/, "launcher should call powershell.exe");
assert.match(launcher, /-ExecutionPolicy Bypass/, "launcher should bypass execution policy for this process");
assert.match(launcher, /windows-package-msvc\.ps1/, "launcher should invoke the packaging script");
assert.match(launcher, /%SUPPLIED_ARGS%/, "launcher should forward user arguments");
