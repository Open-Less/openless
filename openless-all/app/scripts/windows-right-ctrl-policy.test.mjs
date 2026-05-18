import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

function assertMatch(source, pattern, message) {
  assert.match(source, pattern, message);
}

const settingsTsx = await readFile(new URL("../src/pages/Settings.tsx", import.meta.url), "utf8");

assertMatch(
  settingsTsx,
  /const showRightCtrlHoldWarning = detectOS\(\) === 'win'[\s\S]*&& prefs\.hotkey\.mode === 'hold'[\s\S]*&& prefs\.dictationHotkey\.primary === 'RightControl'[\s\S]*&& prefs\.dictationHotkey\.modifiers\.length === 0;/,
  "Right Ctrl warning should be scoped to Windows + RightControl + hold mode only",
);

assertMatch(
  settingsTsx,
  /\{showRightCtrlHoldWarning && \([\s\S]*settings\.recording\.rightCtrlHoldWarningTitle[\s\S]*settings\.recording\.rightCtrlHoldWarningDesc[\s\S]*\)\}/,
  "Recording settings should render the scoped Right Ctrl hold warning",
);

for (const locale of ["en", "zh-CN", "zh-TW", "ja", "ko"]) {
  const source = await readFile(new URL(`../src/i18n/${locale}.ts`, import.meta.url), "utf8");
  assertMatch(
    source,
    /rightCtrlHoldWarningTitle:/,
    `${locale} should define the Right Ctrl warning title`,
  );
  assertMatch(
    source,
    /rightCtrlHoldWarningDesc:/,
    `${locale} should define the Right Ctrl warning description`,
  );
}
