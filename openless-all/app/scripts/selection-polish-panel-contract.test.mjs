// Selection polish stays in its own tool window. The QA panel is a separate
// surface. Routing, the capability grant, and the pending flag have to move
// together: a missing route shows the main shell inside the preview window,
// and a missing pending flag treats a leftover selection snapshot as a new result.

import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const read = (relativePath) => readFile(`${appRoot}/${relativePath}`, 'utf8');
const violations = [];

const lib = await read('src-tauri/src/lib.rs');
const capabilities = await read('src-tauri/capabilities/default.json');
const commands = await read('src-tauri/src/commands/selection_polish_preview.rs');
const appTsx = await read('src/App.tsx');
const mainTsx = await read('src/main.tsx');
const page = await read('src/pages/SelectionPolishPreview.tsx');

const windowCapabilities = JSON.parse(capabilities).windows;
for (const label of ['qa', 'selection-polish-preview', 'selection-voice-intent']) {
  if (!windowCapabilities.includes(label)) {
    violations.push(`capabilities/default.json does not grant the ${label} window`);
  }
}
if (!mainTsx.includes("windowKind === 'selection-polish-preview'")) {
  violations.push('src/main.tsx does not route ?window=selection-polish-preview');
}
if (
  !appTsx.includes('isSelectionPolishPreview') ||
  !appTsx.includes('<SelectionPolishPreview />')
) {
  violations.push('src/App.tsx does not render SelectionPolishPreview');
}
if (!page.includes('className="ol-tool-window"')) {
  violations.push('SelectionPolishPreview.tsx is not using the tool window');
}
if (!page.includes("listen('selection-polish-preview:shown'")) {
  violations.push('SelectionPolishPreview.tsx does not refresh when the preview is shown');
}
if (!page.includes('selectionPolishPreview.confirmReplace')) {
  violations.push('SelectionPolishPreview.tsx dropped the confirm action');
}
if (!/<textarea\b/.test(page)) {
  violations.push('SelectionPolishPreview.tsx no longer has an editable result');
}

if (!/WebviewWindowBuilder::new\(\s*app,\s*"selection-polish-preview"/.test(lib)) {
  violations.push('lib.rs does not build the selection-polish-preview window');
}
if (!/emit_to\(\s*"selection-polish-preview",\s*"selection-polish-preview:shown"/.test(lib)) {
  violations.push('the shown event is not emitted to the preview window');
}
if (
  !/fn show_selection_polish_preview[\s\S]{0,700}?mark_selection_polish_preview_pending\(\)/.test(
    lib,
  )
) {
  violations.push('show_selection_polish_preview does not mark the preview pending');
}
if (
  !/fn hide_selection_polish_preview[\s\S]{0,350}?clear_selection_polish_preview_pending\(\)/.test(
    lib,
  )
) {
  violations.push('hide_selection_polish_preview does not clear the pending flag');
}
if (/show_qa_window\(app, "polish-preview"\)/.test(lib)) {
  violations.push('polish preview was folded back into the qa window');
}

if (
  !/fn get_selection_polish_preview[\s\S]{0,500}?if !crate::selection_polish_preview_pending\(\)/.test(
    commands,
  )
) {
  violations.push('get_selection_polish_preview lost its pending gate');
}
if (
  !/fn confirm_selection_polish_preview[\s\S]{0,250}?clear_selection_polish_preview_pending\(\)/.test(
    commands,
  )
) {
  violations.push('confirm_selection_polish_preview no longer clears the pending flag');
}
if (
  !/fn cancel_selection_polish_preview[\s\S]{0,250}?clear_selection_polish_preview_pending\(\)/.test(
    commands,
  )
) {
  violations.push('cancel_selection_polish_preview no longer clears the pending flag');
}

for (const locale of ['zh-CN', 'zh-TW', 'en', 'ja', 'ko']) {
  const source = await read(`src/i18n/${locale}.ts`);
  if (!source.includes('selectionPolishPreview:')) {
    violations.push(`src/i18n/${locale}.ts lost the selectionPolishPreview strings`);
  }
}

if (violations.length) {
  throw new Error(`Selection polish panel contract failed:\n${violations.join('\n')}`);
}

console.log('selection-polish-panel-contract.test.mjs passed');
