import { register } from 'tsx/esm/api';
register();
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createInstance } from 'i18next';
const { zhCN } = await import('../src/i18n/zh-CN.ts');
const { zhTW } = await import('../src/i18n/zh-TW.ts');
const { en } = await import('../src/i18n/en.ts');
const { de } = await import('../src/i18n/de.ts');
const { es } = await import('../src/i18n/es.ts');
const { fr } = await import('../src/i18n/fr.ts');
const { ja } = await import('../src/i18n/ja.ts');
const { ko } = await import('../src/i18n/ko.ts');
const { getStylePackPresentation } = await import('../src/lib/stylePackPresentation.ts');
const core = readFileSync(
  new URL('../crates/openless-core/src/style_packs.rs', import.meta.url),
  'utf8',
);
const coreDefaults = [
  ...core.matchAll(
    /PolishMode::(Raw|Light|Structured|Formal) => StylePack \{\s*id:[^\n]+\n\s*name: ("(?:[^"\\]|\\.)*")\.into\(\),\s*description: ("(?:[^"\\]|\\.)*")\.into\(\),/g,
  ),
].map((match) => ({
  id: `builtin.${match[1].toLowerCase()}`,
  kind: 'builtin',
  baseMode: match[1].toLowerCase(),
  name: JSON.parse(match[2]),
  description: JSON.parse(match[3]),
}));
assert.equal(coreDefaults.length, 4, 'check every shipped Core builtin');
const mock = readFileSync(new URL('../src/lib/ipc/mock-data.ts', import.meta.url), 'utf8');
const mockDefaults = [
  ...mock.matchAll(
    /makeMockStylePack\(\s*'builtin\.(raw|light|structured|formal)',\s*'builtin',\s*'[^']+',\s*'([^']+)',\s*'([^']+)'/g,
  ),
].map((match) => ({
  id: `builtin.${match[1]}`,
  kind: 'builtin',
  baseMode: match[1],
  name: match[2],
  description: match[3],
}));
assert.ok(mockDefaults.length >= 4, 'check browser preview and reset defaults');
const locales = { 'zh-CN': zhCN, 'zh-TW': zhTW, en, de, es, fr, ja, ko };
const instance = createInstance();
await instance.init({
  lng: 'en',
  fallbackLng: false,
  resources: Object.fromEntries(
    Object.entries(locales).map(([key, translation]) => [key, { translation }]),
  ),
});
for (const locale of Object.keys(locales)) {
  await instance.changeLanguage(locale);
  for (const pack of [...coreDefaults, ...mockDefaults]) {
    const before = JSON.stringify(pack);
    const display = getStylePackPresentation(pack, instance.t);
    assert.equal(
      display.name,
      instance.t(`style.modes.${pack.baseMode}.name`),
      `${locale}: builtin name`,
    );
    assert.equal(
      display.description,
      instance.t(`style.modes.${pack.baseMode}.desc`),
      `${locale}: builtin description`,
    );
    assert.equal(JSON.stringify(pack), before, 'display must not alter stored metadata');
    const renamed = getStylePackPresentation({ ...pack, name: 'My own style' }, instance.t);
    assert.equal(renamed.name, 'My own style');
    assert.equal(
      renamed.description,
      display.description,
      'renaming does not suppress the default description translation',
    );
    const edited = getStylePackPresentation(
      { ...pack, description: '用户自己写的说明' },
      instance.t,
    );
    assert.equal(
      edited.name,
      display.name,
      'editing the description does not suppress the default name translation',
    );
    assert.equal(edited.description, '用户自己写的说明');
    const imported = getStylePackPresentation({ ...pack, kind: 'imported' }, instance.t);
    assert.deepEqual(
      imported,
      { name: pack.name, description: pack.description, tags: [] },
      'authored package content stays original',
    );
  }
  for (const [baseMode, tag, key] of [
    ['raw', '最小改写', 'minimalEdits'],
    ['light', '强纠错', 'strongCorrection'],
    ['light', '沟通', 'communication'],
    ['light', '自然', 'natural'],
    ['structured', '条理', 'organized'],
    ['structured', 'AI 编程', 'aiCoding'],
    ['structured', '技术结构化', 'technicalStructure'],
    ['formal', '工作沟通', 'workplaceCommunication'],
  ]) {
    const base = coreDefaults.find((pack) => pack.baseMode === baseMode);
    const tags = [tag, '我的自定义标签'];
    const translated = getStylePackPresentation({ ...base, tags }, instance.t).tags;
    assert.deepEqual(translated, [instance.t(`style.pack.builtinTags.${key}`), '我的自定义标签']);
    assert.notEqual(
      translated[0],
      `style.pack.builtinTags.${key}`,
      `${locale}: tag resource exists`,
    );
    assert.deepEqual(tags, [tag, '我的自定义标签'], 'original tags stay untouched');
    assert.deepEqual(
      getStylePackPresentation({ ...base, kind: 'imported', tags }, instance.t).tags,
      tags,
    );
  }
  assert.deepEqual(
    getStylePackPresentation({ ...coreDefaults[0], tags: ['自然'] }, instance.t).tags,
    ['自然'],
    'a known label added to another builtin is user content',
  );
}
const pack = coreDefaults[1];
await instance.changeLanguage('en');
const english = getStylePackPresentation(pack, instance.t);
await instance.changeLanguage('de');
const german = getStylePackPresentation(pack, instance.t);
assert.notEqual(german.name, english.name, 'existing data responds to a locale switch');
assert.notEqual(german.description, english.description);
