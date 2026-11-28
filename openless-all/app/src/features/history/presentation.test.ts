import assert from 'node:assert/strict';
import { createInstance } from 'i18next';
import type { PolishMode, StylePack } from '../../api/contracts';
import { zhCN } from '../../i18n/zh-CN';
import { zhTW } from '../../i18n/zh-TW';
import { en } from '../../i18n/en';
import { ja } from '../../i18n/ja';
import { ko } from '../../i18n/ko';
import { de } from '../../i18n/de';
import { fr } from '../../i18n/fr';
import { es } from '../../i18n/es';
import { styleLabelFor } from './presentation';

const locales = { 'zh-CN': zhCN, 'zh-TW': zhTW, en, ja, ko, de, fr, es };
const packs: StylePack[] = [
  {
    id: 'imported-user', name: 'My authored style', description: '', version: '1.0.0',
    kind: 'imported', baseMode: 'formal', prompt: 'Authored prompt', selectionPrompt: '',
    voiceEditPrompt: '', examples: [], tags: [], active: false, enabled: false,
  },
  {
    id: 'builtin.light', name: '轻度润色', description: '', version: '1.0.0',
    kind: 'builtin', baseMode: 'light', prompt: '', selectionPrompt: 'Selection prompt',
    voiceEditPrompt: '', examples: [], tags: [], active: true, enabled: true,
  },
  {
    id: 'builtin.intent', name: '专为构建而生', description: '', version: '1.0.0',
    kind: 'builtin', baseMode: 'structured', prompt: '', selectionPrompt: 'Intent prompt',
    voiceEditPrompt: '', examples: [], tags: [], active: false, enabled: true,
  },
];

for (const [locale, resource] of Object.entries(locales)) {
  const i18n = createInstance();
  await i18n.init({ lng: locale, fallbackLng: false, resources: { [locale]: { translation: resource } } });
  const labels: Record<PolishMode, string> = {
    raw: resource.style.modes.raw.name,
    light: resource.style.modes.light.name,
    structured: resource.style.modes.structured.name,
    formal: resource.style.modes.formal.name,
  };
  const label = (stylePackId: string | null, source: 'selection_polish' | 'selection_voice_edit' | 'voice_edit' | 'voice' = 'selection_polish') =>
    styleLabelFor({ stylePackId, source, mode: 'raw' }, packs, labels, i18n.t);
  assert.equal(label('imported-user'), resource.history.selectionPolishStyle.replace('{{style}}', packs[0].name));
  assert.equal(label('builtin.light'), resource.history.selectionPolishStyle.replace('{{style}}', labels.light));
  assert.equal(label('builtin.intent'), resource.history.selectionPolishStyle.replace('{{style}}', packs[2].name));
  assert.equal(label('missing'), resource.history.selectionPolish, 'deleted styles must not be mislabeled Raw');
  assert.equal(label(null), resource.history.selectionPolish, 'legacy original rows remain identifiable');
  assert.equal(styleLabelFor({ source: 'selection_polish', stylePackId: 'imported-user', mode: 'raw' }, null, labels, i18n.t), resource.history.selectionPolish);
  assert.equal(label(null, 'selection_voice_edit'), resource.history.selectionVoiceEdit);
  assert.equal(label(null, 'voice_edit'), resource.history.voiceEdit);
  assert.notEqual(label(null, 'selection_voice_edit'), label(null));
  assert.equal(label('imported-user', 'voice'), packs[0].name, 'ordinary dictation keeps its style label');
  assert.equal(label(null, 'voice'), labels.raw);
  assert.ok(resource.history.selectionPolishStyle.includes('{{style}}'));
  if (locale === 'zh-CN') assert.equal(label('builtin.intent'), '选区润色：专为构建而生');
  console.log(`selection history style labels ${locale}: imported, builtin, legacy and voice edit passed`);
}
