import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const appTsx = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
const onboardingTsx = await readFile(
  new URL("../src/components/Onboarding.tsx", import.meta.url),
  "utf8",
);
const floatingShellTsx = await readFile(
  new URL("../src/components/FloatingShell.tsx", import.meta.url),
  "utf8",
);
const settingsModalTsx = await readFile(
  new URL("../src/components/SettingsModal.tsx", import.meta.url),
  "utf8",
);
const settingsTsx = await readFile(new URL("../src/pages/Settings.tsx", import.meta.url), "utf8");
const frontendTypesTs = await readFile(new URL("../src/lib/types.ts", import.meta.url), "utf8");
const rustTypesRs = await readFile(new URL("../src-tauri/src/types.rs", import.meta.url), "utf8");

function assertIncludes(source, expected, message) {
  assert.ok(source.includes(expected), message);
}

assertIncludes(
  onboardingTsx,
  "export const ONBOARDING_COMPLETE_KEY = 'openless:onboarding-complete:v3';",
  "new onboarding flow must not be skipped by the old v2 completion marker",
);

assertIncludes(
  onboardingTsx,
  "export const REQUIRED_ONBOARDING_VERSION = 3;",
  "startup must compare against a persisted onboarding version",
);

assert.match(
  appTsx,
  /async function resolveStartupGate\(\): Promise<Gate> \{\s*const prefs = await getSettings\(\)\.catch\(\(\) => null\);\s*if \(!onboardingMarkedComplete\(prefs\)\) \{\s*return 'onboarding';\s*\}/s,
  "startup must read persisted preferences before provider readiness can bypass onboarding",
);

assert.match(
  appTsx,
  /function onboardingMarkedComplete\(prefs: Pick<UserPreferences, 'onboardingVersion'> \| null\) \{\s*if \(prefs\) \{\s*return prefs\.onboardingVersion >= REQUIRED_ONBOARDING_VERSION;\s*\}/s,
  "startup must require the current onboarding version in persisted preferences",
);

assertIncludes(
  appTsx,
  "if (prefs.startMinimized && gate !== 'onboarding') return;",
  "startMinimized must not hide a required onboarding window",
);

assertIncludes(
  appTsx,
  "initialSettings={postOnboardingSettingsSection !== undefined}",
  "main shell must open settings after onboarding when requested",
);

assertIncludes(
  appTsx,
  "initialSettingsSection={postOnboardingSettingsSection}",
  "main shell must receive the requested settings section after onboarding",
);

assertIncludes(
  appTsx,
  "window.sessionStorage.setItem(PROVIDER_SETUP_PROMPT_DEFERRED_KEY, '1');",
  "settings jump from onboarding must not be covered by the provider setup prompt",
);

assert.doesNotMatch(
  appTsx,
  /asrConfigured\s*&&\s*credentials\.value\.llmConfigured/,
  "startup gate must not treat provider readiness as a replacement for onboarding",
);

assertIncludes(
  onboardingTsx,
  "onboardingVersion: REQUIRED_ONBOARDING_VERSION,",
  "completing onboarding must persist the current onboarding version",
);

assert.match(
  onboardingTsx,
  /const asrReady = Boolean\(credentials\?\.volcengineConfigured \|\| asrSaveState === 'saved'\);/,
  "onboarding ASR step must only consider Volcengine online ASR ready",
);

assert.match(
  onboardingTsx,
  /const LOCAL_ASR_PROVIDER_IDS = new Set\(\['local-qwen3', 'foundry-local-whisper'\]\);/,
  "onboarding must recognize local ASR providers",
);

assert.match(
  onboardingTsx,
  /setActiveAsrProvider\(VOLCENGINE_PROVIDER_ID\)/,
  "onboarding must switch local ASR back to the online default",
);

assertIncludes(
  onboardingTsx,
  "Resource ID: volc.seedasr.sauc.duration",
  "onboarding must visibly remind users which Volcengine Resource ID is expected",
);

assertIncludes(
  onboardingTsx,
  "https://console.volcengine.com/auth/login/",
  "Volcengine guide must link directly to console login",
);

assertIncludes(
  onboardingTsx,
  "https://console.volcengine.com/speech/app?opt=create",
  "Volcengine guide must link directly to legacy app creation",
);

assertIncludes(
  onboardingTsx,
  "https://console.volcengine.com/speech/service/10038?AppID=&opt=create",
  "Volcengine guide must link directly to the Doubao streaming ASR 2.0 management page",
);

assertIncludes(
  onboardingTsx,
  "onboarding.hig.asr.volcGuide",
  "ASR onboarding must expose a compact Volcengine setup guide",
);

assertIncludes(
  onboardingTsx,
  "到底部复制 AppID 和 Access Token",
  "Volcengine guide must tell users where to find AppID and Access Token",
);

assertIncludes(
  onboardingTsx,
  "openSettingsSection?: 'providers' | 'advanced';",
  "onboarding must be able to request a settings jump after completion",
);

assertIncludes(
  onboardingTsx,
  "onboarding.hig.asr.otherOnline",
  "onboarding ASR step must offer a route for other online ASR",
);

assertIncludes(
  onboardingTsx,
  "onboarding.hig.asr.localAi",
  "onboarding ASR step must offer a separate route for local AI",
);

assert.doesNotMatch(
  onboardingTsx,
  /activeSlide === 'asr' && !asrReady[\s\S]{0,260}onboarding\.hig\.asr\.(otherOnline|localAi)/,
  "other ASR and local AI routes must remain visible even when Volcengine credentials are already ready",
);

assertIncludes(
  onboardingTsx,
  "complete({ openSettingsSection: 'providers' })",
  "other online ASR route must jump directly to provider settings",
);

assertIncludes(
  onboardingTsx,
  "complete({ openSettingsSection: 'advanced' })",
  "local AI route must jump directly to advanced settings",
);

assertIncludes(
  frontendTypesTs,
  "onboardingVersion: number;",
  "frontend preferences must include onboardingVersion",
);

assertIncludes(
  rustTypesRs,
  "pub onboarding_version: u32,",
  "persisted Rust preferences must include onboarding_version",
);

assertIncludes(
  rustTypesRs,
  "onboarding_version: 0,",
  "new profiles must default to incomplete onboarding",
);

assertIncludes(
  floatingShellTsx,
  "onStartOnboarding?: () => void;",
  "FloatingShell must expose the manual onboarding callback",
);

assertIncludes(
  floatingShellTsx,
  "initialSettingsSection?: SettingsSectionId;",
  "FloatingShell must accept an initial settings section",
);

assertIncludes(
  floatingShellTsx,
  "useState<SettingsSectionId | undefined>(initialSettingsSection)",
  "FloatingShell must seed the settings modal with the requested section",
);

assertIncludes(
  settingsModalTsx,
  "onStartOnboarding?: () => void;",
  "SettingsModal must pass through the manual onboarding callback",
);

assertIncludes(
  settingsTsx,
  "onStartOnboarding?: () => void;",
  "Settings must accept the manual onboarding callback",
);

assertIncludes(
  settingsTsx,
  "export type SettingsSectionId = 'setup' | 'recording'",
  "Settings must have a setup section before recording",
);

assertIncludes(
  settingsTsx,
  "const SECTION_ORDER: SettingsSectionId[] = ['setup', 'recording'",
  "Settings setup section must be visible in the left rail",
);

assertIncludes(
  settingsTsx,
  "{section === 'setup' && <SetupSection onStartOnboarding={onStartOnboarding} />}",
  "Settings setup section must render the onboarding entry",
);

assertIncludes(
  settingsTsx,
  "onboardingVersion: 0,",
  "manual onboarding entry must reset persisted onboarding completion",
);

assertIncludes(
  settingsTsx,
  "window.localStorage.removeItem(ONBOARDING_COMPLETE_KEY);",
  "manual onboarding entry must clear the legacy webview completion marker",
);
