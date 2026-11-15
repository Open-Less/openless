import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const section = await readFile(
  new URL('../src/pages/settings/SelectionWorkspaceSection.tsx', import.meta.url),
  'utf8',
);
const dialog = await readFile(
  new URL('../src/pages/settings/SelectionWorkspaceExtraControlsDialog.tsx', import.meta.url),
  'utf8',
);

assert.match(section, /SelectionWorkspaceExtraControlsDialog/);
assert.match(section, /aria-haspopup="dialog"/);
assert.match(section, /openExtraControls/);
assert.doesNotMatch(section, /<textarea/);
assert.doesNotMatch(section, /editSystemPromptReset/);

assert.match(dialog, /<PresenceModal/);
assert.match(dialog, /extraControlsSaveHint/);
assert.match(dialog, /editKeywords/);
assert.match(dialog, /editPlanFormat/);
assert.match(dialog, /editSystemPrompt/);
assert.match(dialog, /aria-label=\{t\('common.close'\)\}/);

console.log(
  '[selection-workspace-extra-controls] advanced controls live in a modal and are absent from the primary section',
);
