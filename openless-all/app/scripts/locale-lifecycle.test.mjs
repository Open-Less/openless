import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
async function exercise(scenario) {
  const entries = new Map();
  if (scenario.stored) entries.set('ol.locale', scenario.stored);
  const storage = {
    getItem: (key) => entries.get(key) ?? null,
    setItem: (key, value) => entries.set(key, String(value)),
    removeItem: (key) => entries.delete(key),
    clear: () => entries.clear(),
  };
  // The provider/probe tree renders no HTML. These DOM surfaces let React run
  // its real mount/update effects without adding a browser emulation dependency.
  class Element extends EventTarget {
    ownerDocument;
    tagName;
    nodeType = 1;
    namespaceURI = 'http://www.w3.org/1999/xhtml';
    style = {};
    dataset = {};
    textContent = '';
    constructor(ownerDocument, tagName) {
      super();
      this.ownerDocument = ownerDocument;
      this.tagName = tagName;
    }
    get nodeName() {
      return this.tagName;
    }
    setAttribute() {}
    contains(value) {
      return value === this;
    }
  }
  class TestDocument extends EventTarget {
    nodeType = 9;
    documentElement = new Element(this, 'HTML');
    body = new Element(this, 'BODY');
    activeElement = this.body;
    createElement(tag) {
      return new Element(this, tag.toUpperCase());
    }
  }
  const doc = new TestDocument();
  const browser = Object.assign(new EventTarget(), {
    document: doc,
    localStorage: storage,
    sessionStorage: storage,
    HTMLElement: Element,
    HTMLIFrameElement: class extends Element {},
    matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
    setTimeout,
    clearTimeout,
  });
  Object.defineProperty(globalThis, 'window', { configurable: true, value: browser });
  Object.defineProperty(globalThis, 'document', { configurable: true, value: doc });
  Object.defineProperty(globalThis, 'navigator', {
    configurable: true,
    value: { language: scenario.system, platform: 'MacIntel', userAgent: 'Macintosh' },
  });
  Object.defineProperty(globalThis, 'IS_REACT_ACT_ENVIRONMENT', {
    configurable: true,
    value: true,
  });
  const locale = await import('../src/i18n/index.ts');
  await locale.i18nReady;
  const boot = locale.default.resolvedLanguage;
  const { getSettings, setSettings } = await import('../src/lib/ipc/index.ts');
  const speech = {
    chineseScriptPreference: 'traditional',
    outputLanguagePreference: 'ko',
    translationTargetLanguage: '日本語',
    workingLanguages: ['한국어'],
  };
  await setSettings({
    ...(await getSettings()),
    ...speech,
    workingLanguages: [...speech.workingLanguages],
  });
  const { act, createElement } = await import('react');
  let root;
  let current;
  if (scenario.renderPreferences) {
    const { createRoot } = await import('react-dom/client');
    const { HotkeySettingsProvider, useHotkeySettings } =
      await import('../src/state/HotkeySettingsContext.tsx');
    const { useTranslation } = await import('react-i18next');
    function Probe() {
      current = useHotkeySettings();
      useTranslation();
      return null;
    }
    const mounted = createRoot(doc.createElement('div'));
    root = mounted;
    await act(async () => {
      mounted.render(createElement(HotkeySettingsProvider, { children: createElement(Probe) }));
    });
    assert.ok(current?.prefs, 'the real preference provider has finished loading');
  }
  await act(async () => {
    if (scenario.action === 'switch') await locale.setLocalePreference('fr');
    if (scenario.action === 'system') await locale.setLocalePreference('system');
    if (scenario.action === 'rapid')
      await Promise.all([
        locale.setLocalePreference('de'),
        locale.setLocalePreference('es'),
        locale.setLocalePreference('fr'),
      ]);
    if (scenario.action === 'storage') {
      const changed = new Promise((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error('auxiliary window did not follow the storage event')),
          3000,
        );
        locale.default.once('languageChanged', () => {
          clearTimeout(timer);
          resolve();
        });
      });
      storage.setItem('ol.locale', 'ko');
      browser.dispatchEvent(
        Object.assign(new Event('storage'), {
          key: 'ol.locale',
          newValue: 'ko',
          storageArea: storage,
        }),
      );
      await changed;
    }
  });
  if (current?.prefs) {
    await act(async () => {
      await current.updatePrefs((prefs) => ({ ...prefs, startMinimized: !prefs.startMinimized }));
    });
    await act(async () => {
      await current.refresh();
    });
  }
  const settings = await getSettings();
  for (const [key, value] of Object.entries(speech)) {
    assert.deepEqual(settings[key], value, `UI locale must not overwrite ${key}`);
  }
  if (root) await act(async () => root.unmount());
  console.log(
    JSON.stringify({
      boot,
      active: locale.default.resolvedLanguage,
      saved: storage.getItem('ol.locale'),
      preference: locale.getLocalePreference(),
      title: locale.default.t('settings.title'),
      documentLanguage: doc.documentElement.lang,
    }),
  );
}
const input = process.env.OPENLESS_LOCALE_TEST_SCENARIO;
if (input) {
  await exercise(JSON.parse(input));
} else {
  const run = (scenario) => {
    const result = spawnSync(
      process.execPath,
      ['--import', 'tsx', fileURLToPath(import.meta.url)],
      {
        env: { ...process.env, OPENLESS_LOCALE_TEST_SCENARIO: JSON.stringify(scenario) },
        encoding: 'utf8',
        timeout: 10000,
      },
    );
    assert.equal(result.status, 0, result.stderr || result.error?.message);
    return JSON.parse(result.stdout.trim());
  };
  assert.equal(run({ system: 'de-DE' }).boot, 'de');
  assert.equal(run({ system: 'zh-HK' }).boot, 'zh-TW');
  assert.equal(run({ system: 'pt-BR' }).boot, 'en');
  assert.equal(run({ system: 'ko-KR', stored: 'es' }).boot, 'es');
  assert.equal(run({ system: 'fr-CA', stored: 'not-supported' }).boot, 'fr');
  const switched = run({
    system: 'de-DE',
    stored: 'es',
    action: 'switch',
    renderPreferences: true,
  });
  assert.equal(switched.active, 'fr');
  assert.equal(switched.saved, 'fr');
  assert.equal(switched.documentLanguage, 'fr');
  assert.equal(
    run({ system: 'ja-JP', stored: switched.saved }).boot,
    'fr',
    'restart preserves the explicit language',
  );
  const system = run({ system: 'de-DE', stored: 'fr', action: 'system', renderPreferences: true });
  assert.equal(system.active, 'de');
  assert.equal(system.saved, null);
  assert.equal(system.preference, 'system');
  assert.equal(run({ system: 'es-MX' }).boot, 'es', 'follow-system is resolved again on restart');
  assert.equal(
    run({ system: 'en-US', stored: 'de', action: 'storage', renderPreferences: true }).active,
    'ko',
  );
  const rapid = run({ system: 'en-US', action: 'rapid', renderPreferences: true });
  assert.equal(rapid.active, 'fr');
  assert.equal(rapid.saved, 'fr');
}
