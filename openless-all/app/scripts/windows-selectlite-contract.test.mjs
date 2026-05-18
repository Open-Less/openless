import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const srcRootPath = fileURLToPath(new URL("../src/", import.meta.url));

async function collectTsxFiles(dirPath) {
  const entries = await readdir(dirPath, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const childPath = join(dirPath, entry.name);
    if (entry.isDirectory()) {
      files.push(...await collectTsxFiles(childPath));
    } else if (entry.name.endsWith(".tsx")) {
      files.push(childPath);
    }
  }
  return files;
}

function stripComments(source) {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");
}

function assertMatch(source, pattern, message) {
  assert.match(source, pattern, message);
}

function assertSelectLiteUsage(source, minCount, fileLabel) {
  const count = source.match(/<SelectLite\b/g)?.length ?? 0;
  assert.ok(count >= minCount, `${fileLabel} should use SelectLite at least ${minCount} time(s); found ${count}`);
}

const selectLite = await readFile(new URL("../src/components/ui/SelectLite.tsx", import.meta.url), "utf8");
const settings = await readFile(new URL("../src/pages/Settings.tsx", import.meta.url), "utf8");
const localAsr = await readFile(new URL("../src/pages/LocalAsr.tsx", import.meta.url), "utf8");
const translation = await readFile(new URL("../src/pages/Translation.tsx", import.meta.url), "utf8");
const languageSection = await readFile(new URL("../src/pages/settings/LanguageSection.tsx", import.meta.url), "utf8");

const tsxFiles = await collectTsxFiles(srcRootPath);
for (const filePath of tsxFiles) {
  const source = stripComments(await readFile(filePath, "utf8"));
  assert.doesNotMatch(
    source,
    /<select\b/i,
    `${join("src", relative(srcRootPath, filePath))} should not render native <select>`,
  );
}

assertMatch(selectLite, /export interface SelectOption \{[\s\S]*value: string;[\s\S]*label: string;[\s\S]*disabled\?: boolean;/, "SelectLite should expose value/label/disabled options");
assertMatch(selectLite, /interface SelectLiteProps \{[\s\S]*value: string;[\s\S]*onChange: \(value: string\) => void;[\s\S]*options: SelectOption\[\];[\s\S]*disabled\?: boolean;/, "SelectLite should accept value, onChange, options, and disabled");
assertMatch(selectLite, /createPortal\(/, "SelectLite popover should render through a portal");
assertMatch(selectLite, /role="combobox"[\s\S]*aria-haspopup="listbox"[\s\S]*aria-expanded=\{open\}/, "SelectLite trigger should expose combobox/listbox a11y state");
assertMatch(selectLite, /role="listbox"/, "SelectLite popover should use role=listbox");
assertMatch(selectLite, /role="option"[\s\S]*aria-selected=\{isSelected\}[\s\S]*aria-disabled=\{option\.disabled\}/, "SelectLite options should expose selected and disabled state");
assertMatch(selectLite, /event\.key === 'Escape'[\s\S]*closeMenu\(\)/, "SelectLite should close on Escape");
assertMatch(selectLite, /event\.key === 'ArrowDown'[\s\S]*moveHighlight\(1\)/, "SelectLite should move highlight down from keyboard");
assertMatch(selectLite, /event\.key === 'ArrowUp'[\s\S]*moveHighlight\(-1\)/, "SelectLite should move highlight up from keyboard");
assertMatch(selectLite, /event\.key === 'Enter'[\s\S]*selectIndex\(highlight\)/, "SelectLite should select highlighted option on Enter");
assertMatch(selectLite, /document\.addEventListener\('mousedown', handlePointerDown\)/, "SelectLite should close on outside pointer down");
assertMatch(selectLite, /window\.addEventListener\('wheel', handleScrollOutside/, "SelectLite should close on outside wheel/scroll");
assertMatch(selectLite, /textOverflow: 'ellipsis'/, "SelectLite should constrain long labels instead of expanding layout");

assertSelectLiteUsage(settings, 4, "Settings.tsx");
assertSelectLiteUsage(localAsr, 5, "LocalAsr.tsx");
assertSelectLiteUsage(translation, 1, "Translation.tsx");
assertSelectLiteUsage(languageSection, 1, "LanguageSection.tsx");
