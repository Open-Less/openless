// Opt-in source-component GUI regression. All credentials and native responses are synthetic.
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync, unlinkSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import { createServer } from 'vite';
import { chromium, expect } from '@playwright/test';

const out = resolve(process.env.OPENLESS_GUI_ARTIFACT_DIR || 'output/playwright/realtime-omni');
mkdirSync(out, { recursive: true });
const port = Number(process.env.OPENLESS_GUI_PORT || 15377);
const fixture = resolve(`.realtime-omni-h5-${process.pid}-${Date.now()}.html`);
const server = await createServer({
  server: { host: '127.0.0.1', port, strictPort: true, watch: null },
});
const report = {
  scope:
    'Clean headless Chrome; real source components with synthetic browser/native fixtures. No installed app, system vault, real API, microphone or insertion.',
  checks: [],
  screenshots: [],
  validationScopes: [],
  blockedExternalRequests: [],
  pageErrors: [],
};
let browser;
let page;
writeFileSync(
  fixture,
  `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"></head><body><h1>Realtime 配置验证（合成数据）</h1><div id="root"></div><script type="module">
import React from 'react';
import {createRoot} from 'react-dom/client';
import {flushSync} from 'react-dom';
import {OmniChannelSection} from '/src/pages/settings/ProvidersSection.tsx';
import {HotkeySettingsProvider} from '/src/state/HotkeySettingsContext.tsx';
import {TooltipProvider} from '/src/components/Tooltip.tsx';
import {mockCredentialValues,mockSettings,mockSetSettings,mockHotkeyCapability} from '/src/lib/ipc/mock-data.ts';
import descriptors from '/src/lib/ipc/mock-provider-descriptors.json';
import i18n,{i18nReady} from '/src/i18n/index.ts';
import '/src/styles/tokens.css';
import '/src/styles/global.css';
import '/src/styles/overlays.css';
import '/src/styles/refinements.css';
await i18nReady; await i18n.changeLanguage('zh-CN');
document.documentElement.dataset.olPlatform='win';
Object.assign(mockSettings,{activeOmniProvider:'openai-realtime',pipelineMode:'multimodal',multimodalPipelineEnabled:true,themeMode:'dark',stackedRowLayout:false,conservativeLayout:false});
mockCredentialValues.clear();
mockCredentialValues.set('openai-realtime:omni.endpoint','https://fixture-gateway.invalid/custom/v1');
mockCredentialValues.set('openai-realtime:omni.model','fixture-original-model');
mockCredentialValues.set('openai-realtime:omni.api_key','');
mockCredentialValues.set('openai-realtime:omni.extra_headers','{"X-Fixture":"synthetic-only"}');
window.fixtureRead=(provider,account)=>mockCredentialValues.get(provider+':'+account)??null;
window.fixtureCalls=[]; window.fixtureUnexpected=[];
let revision=0;let releaseWrite;
window.fixtureReleaseWrite=()=>{releaseWrite?.();releaseWrite=undefined};
window.fixtureDelayNextOptionsWrite=false;
window.fixtureEnableNative=()=>{window.__TAURI_INTERNALS__={invoke:async(cmd,args={})=>{
  window.fixtureCalls.push({cmd,args:cmd==='set_credential'?{account:args.account,provider:args.provider,value:'[synthetic fixture]'}:args});
  if(cmd==='get_startup_snapshot')return {contractVersion:'2.0.0',backend:{running:true}};
  if(cmd==='set_windows_caption_theme')return;
  if(cmd==='get_hotkey_capability')return structuredClone(mockHotkeyCapability);
  if(cmd==='read_credential')return window.fixtureRead(args.provider,args.account);
  if(cmd==='set_credential'){
    if(args.account==='omni.realtime_options'&&window.fixtureDelayNextOptionsWrite){window.fixtureDelayNextOptionsWrite=false;await new Promise(resolve=>releaseWrite=resolve)}
    mockCredentialValues.set((args.provider??'')+':'+args.account,args.value);return;
  }
  if(cmd==='set_active_omni_provider')return;
  if(cmd==='validate_provider_credentials')return {ok:true};
  if(cmd==='list_provider_descriptors')return descriptors.filter(x=>x.kind===args.kind);
  if(cmd==='list_provider_models')return {models:descriptors.find(x=>x.kind===args.kind&&x.providerType===args.channelId)?.staticModels??[]};
  if(cmd==='get_settings_snapshot')return {preferences:structuredClone(mockSettings),revision};
  if(cmd==='update_setting_fields'){
    const prefs=structuredClone(mockSettings);
    for(const [path,value]of Object.entries(args.edits)){const keys=path.slice(1).split('/').map(k=>k.replace(/~1/g,'/').replace(/~0/g,'~'));let target=prefs;for(const key of keys.slice(0,-1))target=target[key];target[keys.at(-1)]=value}
    mockSetSettings(prefs);return {preferences:structuredClone(prefs),revision:++revision};
  }
  window.fixtureUnexpected.push(cmd);throw Error('Unexpected synthetic command: '+cmd);
}}};
const root=createRoot(document.getElementById('root'));let epoch=0;
window.fixtureMount=()=>flushSync(()=>root.render(React.createElement(HotkeySettingsProvider,{key:++epoch},React.createElement(TooltipProvider,null,React.createElement(OmniChannelSection)))));
window.fixtureLabels={provider:i18n.t('settings.providers.providerLabel'),model:i18n.t('settings.providers.modelLabel'),customModel:i18n.t('settings.providers.customModelLabel'),endpoint:i18n.t('settings.providers.baseUrlLabel'),headers:i18n.t('settings.providers.extraHeadersLabel'),key:i18n.t('settings.providers.apiKeyLabel'),verify:i18n.t('settings.channels.verify'),auth:i18n.t('settings.providers.realtimeOptions.authLabel'),effort:i18n.t('settings.providers.realtimeOptions.effortLabel'),fallback:i18n.t('settings.providers.realtimeOptions.fallbackLabel'),optionalKey:i18n.t('settings.providers.realtimeOptions.optionalKey'),candidates:i18n.t('settings.providers.realtimeOptions.modelCandidates'),profiles:{'openai-realtime':i18n.t('settings.providers.presets.omniOpenaiRealtime'),'dashscope-realtime':i18n.t('settings.providers.presets.omniDashscopeRealtime'),'dashscope-realtime-legacy':i18n.t('settings.providers.presets.omniDashscopeRealtimeLegacy')},efforts:Object.fromEntries(['auto','omit','minimal','low','medium','high','xhigh','none'].map(k=>[k,i18n.t('settings.providers.realtimeOptions.'+k)])),authModes:{bearer:i18n.t('settings.providers.realtimeOptions.bearer'),'api-key':i18n.t('settings.providers.realtimeOptions.apiKey'),headers:i18n.t('settings.providers.realtimeOptions.headers')}};
window.fixtureMount();window.fixtureReady=true;
</script><style>body{margin:0;padding:28px;background:var(--ol-bg);color:var(--ol-ink);overflow:auto}h1{max-width:1040px;margin:0 auto 20px;font-size:20px}#root{max-width:1040px;margin:auto}</style></body></html>`,
);

try {
  await server.listen();
  browser = await chromium.launch({
    headless: true,
    ...(process.platform === 'win32' ? { channel: 'chrome' } : {}),
  });
  const context = await browser.newContext({
    viewport: { width: 1280, height: 1280 },
    locale: 'zh-CN',
    colorScheme: 'dark',
    reducedMotion: 'reduce',
  });
  await context.route('**/*', async (route) => {
    const url = new URL(route.request().url());
    if (url.hostname === '127.0.0.1' || url.protocol === 'data:') await route.continue();
    else {
      report.blockedExternalRequests.push(url.origin);
      await route.abort();
    }
  });
  page = await context.newPage();
  page.on('pageerror', (error) => report.pageErrors.push(error.message));
  await page.goto(`http://127.0.0.1:${port}/${basename(fixture)}`);
  await page.waitForFunction(() => window.fixtureReady === true);
  const labels = await page.evaluate(() => window.fixtureLabels);
  const card = page.locator('.ol-omni-settings');
  const snapshot = async (name) =>
    writeFileSync(resolve(out, name + '.aria.txt'), await page.locator('body').ariaSnapshot());
  const screenshot = async (name) => {
    await snapshot(name);
    const path = resolve(out, name + '.png');
    await page.screenshot({ path, fullPage: true });
    report.screenshots.push(path);
  };
  const check = (name) => report.checks.push(name);
  const choose = async (label, option) => {
    const control = card.getByRole('combobox', { name: label, exact: true });
    await expect(control).toBeEnabled();
    await control.click();
    await snapshot('latest-open-menu');
    await page.getByRole('option', { name: option, exact: true }).click();
  };
  const read = (provider, account) =>
    page.evaluate(({ provider, account }) => window.fixtureRead(provider, account), {
      provider,
      account,
    });
  const options = async (provider) =>
    JSON.parse((await read(provider, 'omni.realtime_options')) || '{}');
  const settled = async () =>
    expect(card.getByRole('button', { name: labels.verify, exact: true })).toBeEnabled();
  const verifyScope = async (provider) => {
    await settled();
    await card.getByRole('button', { name: labels.verify, exact: true }).click();
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            window.fixtureCalls.filter((c) => c.cmd === 'validate_provider_credentials').at(-1)
              ?.args.channelId,
        ),
      )
      .toBe(provider);
    report.validationScopes.push(provider);
    await settled();
  };
  const switchProfile = async (provider) => {
    await choose(labels.provider, labels.profiles[provider]);
    await expect(card.getByRole('combobox', { name: labels.provider, exact: true })).toContainText(
      labels.profiles[provider],
    );
    await settled();
  };
  await settled();
  await snapshot('00-initial');
  check('Source Omni form loads in a clean browser mock');
  await screenshot('01-openai-auto');
  await card.getByRole('combobox', { name: labels.provider, exact: true }).click();
  await screenshot('00-protocol-menu');
  await page.keyboard.press('Escape');
  await page.evaluate(() => window.fixtureEnableNative());

  await card.getByRole('combobox', { name: labels.effort, exact: true }).click();
  await screenshot('02-effort-menu');
  assert.equal(await page.getByRole('option').count(), 8);
  for (const value of Object.values(labels.efforts))
    await expect(page.getByRole('option', { name: value, exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
  for (const effort of ['auto', 'omit', 'minimal', 'low', 'medium', 'high', 'xhigh', 'none']) {
    await choose(labels.effort, labels.efforts[effort]);
    // Auto is already the default and may not emit a mutation.
    if (effort !== 'auto')
      await expect
        .poll(async () => (await options('openai-realtime')).reasoningEffort)
        .toBe(effort);
    if (effort !== 'none')
      assert.equal(
        await card.getByRole('switch', { name: labels.fallback, exact: true }).count(),
        0,
      );
  }
  await expect(card.getByRole('switch', { name: labels.fallback, exact: true })).toHaveAttribute(
    'aria-checked',
    'true',
  );
  await card.getByRole('switch', { name: labels.fallback, exact: true }).click();
  await expect.poll(async () => (await options('openai-realtime')).reasoningFallback).toBe(false);
  check('All eight reasoning choices persist; fallback control is shown only for none');
  await screenshot('03-openai-none-fallback');

  await choose(labels.auth, labels.authModes.headers);
  await expect.poll(async () => (await options('openai-realtime')).authMode).toBe('headers');
  await expect(card.getByLabel(labels.optionalKey, { exact: true })).toHaveValue('');
  await card
    .getByLabel(labels.endpoint, { exact: true })
    .fill('https://fixture-gateway.invalid/custom-v2/v1');
  await page.getByRole('heading', { level: 1 }).click();
  await expect
    .poll(() => read('openai-realtime', 'omni.endpoint'))
    .toBe('https://fixture-gateway.invalid/custom-v2/v1');
  await choose(labels.model, labels.customModel);
  await card.getByLabel(labels.model, { exact: true }).fill('fixture-custom-model-v2');
  await page.getByRole('heading', { level: 1 }).click();
  await expect.poll(() => read('openai-realtime', 'omni.model')).toBe('fixture-custom-model-v2');
  const headers = card.getByLabel(labels.headers, { exact: true });
  await headers.fill('{"Authorization":"Bearer SYNTHETIC-ONLY","X-Fixture":"headers-mode"}');
  await page.getByRole('heading', { level: 1 }).click();
  await expect
    .poll(() => read('openai-realtime', 'omni.extra_headers'))
    .toContain('SYNTHETIC-ONLY');
  await expect(headers).toHaveAttribute('type', 'password');
  await verifyScope('openai-realtime');
  check(
    'Headers-only mode keeps an empty main key, masks headers, and validates explicit OpenAI scope',
  );
  await screenshot('04-openai-headers-empty-key');

  for (const provider of ['dashscope-realtime', 'dashscope-realtime-legacy']) {
    await switchProfile(provider);
    assert.equal(await card.getByRole('combobox', { name: labels.effort, exact: true }).count(), 0);
    assert.equal(await card.getByRole('switch', { name: labels.fallback, exact: true }).count(), 0);
    await expect(card).toContainText('16 kHz PCM');
    if (provider === 'dashscope-realtime') {
      await expect(card).toContainText('Qwen 3.8');
      await choose(labels.auth, labels.authModes['api-key']);
      await card.getByLabel(labels.key, { exact: true }).fill('SYNTHETIC-QWEN-KEY');
      await page.getByRole('heading', { level: 1 }).click();
      await expect.poll(async () => (await options(provider)).authMode).toBe('api-key');
    }
    await verifyScope(provider);
    await screenshot(
      provider === 'dashscope-realtime' ? '05-dashscope-current' : '06-dashscope-legacy',
    );
  }
  check(
    'Both Qwen profiles hide OpenAI reasoning, show 16 kHz boundaries, and validate their own scope',
  );
  await switchProfile('openai-realtime');
  assert.equal(
    await read('openai-realtime', 'omni.endpoint'),
    'https://fixture-gateway.invalid/custom-v2/v1',
  );
  assert.equal(await read('openai-realtime', 'omni.model'), 'fixture-custom-model-v2');
  assert.deepEqual(await options('openai-realtime'), {
    authMode: 'headers',
    reasoningEffort: 'none',
    reasoningFallback: false,
  });
  await expect(card.getByLabel(labels.headers, { exact: true })).toHaveAttribute(
    'type',
    'password',
  );
  await expect(card.getByLabel(labels.optionalKey, { exact: true })).toHaveValue('');
  await screenshot('07-restored-gateway-options');
  check(
    'Switching away and back preserves gateway, custom model, empty key, headers and typed options',
  );

  await page.evaluate(() => {
    window.fixtureDelayNextOptionsWrite = true;
  });
  await choose(labels.effort, labels.efforts.minimal);
  await expect(card.getByRole('button', { name: labels.verify, exact: true })).toBeDisabled();
  await choose(labels.provider, labels.profiles['dashscope-realtime']);
  // The selected profile must remain until the delayed old-profile write converges.
  await expect(card.getByRole('combobox', { name: labels.provider, exact: true })).toContainText(
    labels.profiles['openai-realtime'],
  );
  await screenshot('08-waiting-for-profile-save');
  await page.evaluate(() => window.fixtureReleaseWrite());
  await expect(card.getByRole('combobox', { name: labels.provider, exact: true })).toContainText(
    labels.profiles['dashscope-realtime'],
  );
  await settled();
  assert.equal((await options('openai-realtime')).reasoningEffort, 'minimal');
  assert.equal((await options('dashscope-realtime')).authMode, 'api-key');
  check(
    'Provider switching waits for pending options; delayed writes stay in the original profile',
  );

  await switchProfile('openai-realtime');
  await choose(labels.effort, labels.efforts.none);
  await settled();
  await page.evaluate(() => window.fixtureMount());
  await settled();
  assert.deepEqual(await options('openai-realtime'), {
    authMode: 'headers',
    reasoningEffort: 'none',
    reasoningFallback: false,
  });
  await expect(card.getByRole('switch', { name: labels.fallback, exact: true })).toHaveAttribute(
    'aria-checked',
    'false',
  );
  await screenshot('09-reopened-form');
  check('Reopening the source form restores mock-vault options without migration');
  await card.getByRole('button', { name: labels.candidates, exact: true }).click();
  await settled();
  const candidatePicker = card.getByRole('combobox', {
    name: '选择一个模型写入上方字段',
    exact: true,
  });
  await expect(candidatePicker).toBeEnabled();
  await candidatePicker.click();
  await expect(page.getByRole('option', { name: 'gpt-realtime-2.1', exact: true })).toBeVisible();
  await expect(
    page.getByRole('option', { name: 'gpt-realtime-2.1-mini', exact: true }),
  ).toBeVisible();
  await screenshot('10a-model-candidate-menu');
  await page.keyboard.press('Escape');
  await screenshot('10-model-candidates');
  check(
    'Model candidates and their catalog/service limitation are exposed without claiming remote completeness',
  );
  await page.setViewportSize({ width: 900, height: 1200 });
  await screenshot('11-narrow-form');
  assert.equal(
    await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1),
    false,
  );
  assert.deepEqual(await page.evaluate(() => window.fixtureUnexpected), []);
  assert.deepEqual(report.pageErrors, []);
  assert.deepEqual(report.blockedExternalRequests, []);
  report.status = 'passed';
  console.log(
    JSON.stringify({
      status: report.status,
      checks: report.checks.length,
      validationScopes: report.validationScopes,
      artifactDirectory: out,
    }),
  );
} catch (error) {
  report.status = 'failed';
  report.error = String(error?.stack || error);
  if (page) {
    try {
      writeFileSync(resolve(out, 'failure.aria.txt'), await page.locator('body').ariaSnapshot());
      await page.screenshot({ path: resolve(out, 'failure.png'), fullPage: true });
    } catch {}
  }
  console.error(report.error);
  process.exitCode = 1;
} finally {
  await browser?.close();
  await server.close();
  unlinkSync(fixture);
  writeFileSync(resolve(out, 'report.private.json'), JSON.stringify(report, null, 2) + '\n');
}
