import { fixtureOrigin } from './support/server';
import { openExperiments } from './support/experiments';
import { settleNavigation } from './support/navigation';
import { expect, test, type Page } from '@playwright/test';

// Development React is intentional: production builds hide special-prop/ref warnings.
test.use({ baseURL: fixtureOrigin });
const messages = new WeakMap<Page, string[]>();
test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  messages.set(page, errors);
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => {
    // Chromium's software WebGL renderer reports GPU readback noise unrelated to React.
    if (message.text().includes('GL Driver Message')) return;
    if (message.type() === 'error' || message.type() === 'warning') errors.push(message.text());
  });
  await page.goto('/');
});
test.afterEach(async ({ page }) => {
  expect(messages.get(page)).toEqual([]);
});

async function settings(page: Page, category: string) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .getByRole('navigation', { name: '设置分类' })
    .getByRole('button', { name: category === '实验与扩展' ? '高级功能' : category, exact: true })
    .click();
  await settleNavigation(page);
}

async function expectInViewport(page: Page) {
  const bounds = (await page.getByRole('tooltip').boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.y).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width + 1);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(viewport.height + 1);
}

test('explanatory labels replace help badges, open beside the words and dismiss with Escape', async ({
  page,
}) => {
  await settings(page, 'AI 与模型');
  const label = page.getByRole('button', { name: '识别管线', exact: true });
  const row = page.locator('.ol-service-pipeline-row');
  await expect(label).toHaveCSS('text-decoration-style', 'dotted');
  await expect(label).toHaveCSS('cursor', 'pointer');
  await expect(row.locator('svg.lucide-circle-question-mark')).toHaveCount(0);
  const before = await row.boundingBox();
  await label.hover();
  const tooltip = page.getByRole('tooltip');
  await expect(tooltip).toContainText('ASR');
  await expect(row.getByRole('tooltip')).toHaveCount(0);
  await expectInViewport(page);
  expect((await tooltip.boundingBox())!.x).toBeGreaterThanOrEqual(
    (await label.boundingBox())!.x + (await label.boundingBox())!.width,
  );
  expect((await row.boundingBox())!.height).toBe(before!.height);
  expect((await row.boundingBox())!.width).toBe(before!.width);
  // Moving onto the popup must not make a multi-line explanation disappear.
  await tooltip.hover();
  await expect(tooltip).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(tooltip).toBeHidden();
  await expect(page.getByRole('dialog', { name: '设置', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '传统模式', exact: true }).focus();
  await page.keyboard.press('Shift+Tab');
  await expect(label).toBeFocused();
  await expect(tooltip).toBeVisible();
  await expect(label).toHaveAttribute(
    'aria-describedby',
    (await tooltip.getAttribute('id')) as string,
  );
  await page.keyboard.press('Escape');
  await expect(tooltip).toBeHidden();
});

test('experimental models show three compact switches; memory warnings stay in the shared popup', async ({
  page,
}) => {
  await settings(page, '实验与扩展');
  await page.locator('[data-ol-advanced-entry="localModels"]').click();
  const section = page.locator('.ol-experimental-models');
  await expect(section.getByRole('switch')).toHaveCount(3);
  await expect(section.locator('.ol-channel-local-hint, p:visible')).toHaveCount(0);
  expect((await section.boundingBox())!.height).toBeLessThan(200);
  const label = section.getByRole('button', { name: 'SenseVoice Small', exact: true });
  await label.hover();
  await expect(page.getByRole('tooltip')).toContainText('240 MB');
  await expect(page.getByRole('tooltip')).toContainText('删除');
  await expect(page.getByRole('tooltip')).toContainText('使用本地模型会增加内存占用');
  await expect(section.getByRole('tooltip')).toHaveCount(0);
  await label.click();
  await expect(section.getByRole('switch', { name: 'SenseVoice Small' })).toHaveAttribute(
    'aria-checked',
    'false',
  );
  await page.keyboard.press('Escape');
  await expect(page.getByRole('tooltip')).toBeHidden();
  await page.getByRole('button', { name: '快捷键', exact: true }).click();
  await expect(page.locator('.ol-tooltip-positioner')).toHaveCount(0);
});

for (const zoom of ['0.9', '1', '1.1']) {
  test(`font menu and tooltip remain anchored at interface zoom ${zoom}`, async ({ page }) => {
    await settings(page, '外观与语言');
    await page.getByRole('combobox', { name: '界面大小', exact: true }).click();
    await page
      .getByRole('option', { name: `${Math.round(Number(zoom) * 100)}%`, exact: true })
      .click();
    await expect(page.locator('.ol-select-popup[data-open]')).toHaveCount(0);
    const trigger = page.getByRole('combobox', { name: '字体系列', exact: true });
    await trigger.press('Enter');
    const popup = page.locator('.ol-select-popup[data-open]');
    await expect(popup).toBeVisible();
    await expect(popup).toHaveCSS('opacity', '1');
    const anchor = (await trigger.boundingBox())!;
    const bounds = (await popup.boundingBox())!;
    expect(bounds.y).toBeGreaterThanOrEqual(anchor.y + anchor.height - 1);
    expect(Math.abs(bounds.x - anchor.x)).toBeLessThan(2);
    expect(Math.abs(bounds.width - anchor.width)).toBeLessThan(2);
    await page.keyboard.press('Escape');
    await expect(popup).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await trigger.press('Enter');
    await expect(page.getByRole('option', { name: '系统默认', exact: true })).toBeFocused();
    await page.keyboard.press('End');
    await expect(page.getByRole('option', { name: 'Menlo · Consolas', exact: true })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(trigger).toContainText('Menlo');
    const label = page.getByRole('button', { name: '字体系列', exact: true });
    await label.hover();
    await expect(page.getByRole('tooltip')).toContainText('已安装字体');
    await expectInViewport(page);
    const hint = (await page.getByRole('tooltip').boundingBox())!;
    const labelBounds = (await label.boundingBox())!;
    expect(hint.x).toBeGreaterThanOrEqual(labelBounds.x + labelBounds.width);
  });
}

test('narrow windows keep select and long model explanations within the viewport', async ({
  page,
}) => {
  await settings(page, '外观与语言');
  await page.setViewportSize({ width: 375, height: 812 });
  const trigger = page.getByRole('combobox', { name: '字体系列', exact: true });
  await trigger.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  const bounds = (await page.locator('.ol-select-popup').boundingBox())!;
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(375);
  await page.keyboard.press('Escape');
  await openExperiments(page);
  await page.locator('[data-ol-advanced-entry="localModels"]').click();
  await page.getByRole('button', { name: 'Embedding Gemma', exact: true }).click();
  await expect(page.getByRole('tooltip')).toBeVisible();
  await expectInViewport(page);
});

test.describe('touch hints', () => {
  test.use({ hasTouch: true });
  test('a tap opens the description without changing the model switch; outside tap dismisses', async ({
    page,
  }) => {
    await settings(page, '实验与扩展');
    await page.locator('[data-ol-advanced-entry="localModels"]').click();
    await page.getByRole('button', { name: 'SenseVoice Small', exact: true }).tap();
    await expect(page.getByRole('tooltip')).toContainText('240 MB');
    // Stay open through Chromium's synthetic hover/leave events following touch.
    await page.waitForTimeout(350);
    await expect(page.getByRole('tooltip')).toBeVisible();
    await expect(page.getByRole('switch', { name: 'SenseVoice Small' })).toHaveAttribute(
      'aria-checked',
      'false',
    );
    await page.getByRole('heading', { name: '实验性本地语音模型 Beta', exact: true }).tap();
    await expect(page.getByRole('tooltip')).toBeHidden();
  });
});

test('style card lists mount and switch without the React 18 PopChild ref warning', async ({
  page,
}) => {
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '润色模式', exact: true }).click();
  await expect(page.locator('[data-ol-style-pack]').first()).toBeVisible();
  const tabs = page.locator('main .ol-choice-chips').first().getByRole('button');
  await expect(tabs).toHaveCount(2);
  await tabs.nth(1).click();
  await expect(page.locator('[data-ol-style-pack]').first()).toBeVisible();
  await tabs.nth(0).click();
  await expect(page.locator('[data-ol-style-pack]').first()).toBeVisible();
});

test('Gemini Omni exposes the shared thinking setting and hides it for transcription models', async ({
  page,
}, testInfo) => {
  await settings(page, 'AI 与模型');
  await page.getByRole('button', { name: '多模态模式', exact: true }).click();
  const card = page.locator('.ol-omni-settings');
  await card.getByRole('combobox', { name: '供应商', exact: true }).click();
  await page.getByRole('option', { name: 'Google Gemini', exact: true }).click();
  const thinking = card.getByRole('switch', { name: '思考', exact: true });
  await expect(thinking).toHaveAttribute('aria-checked', 'false');
  await thinking.click();
  await expect(thinking).toHaveAttribute('aria-checked', 'true');
  await expect
    .poll(() =>
      page.evaluate(async () => {
        // Import the module instance the app loaded (Vite appends an HMR query).
        const url = performance
          .getEntriesByType('resource')
          .map((entry) => entry.name)
          .find((name) => name.includes('/src/lib/ipc/mock-data.ts'));
        const { mockSettings } = await import(url || '/src/lib/ipc/mock-data.ts');
        return mockSettings.llmThinkingEnabled;
      }),
    )
    .toBe(true);
  await page.screenshot({ path: testInfo.outputPath('gemini-omni-desktop.png') });

  await page.setViewportSize({ width: 375, height: 812 });
  await thinking.scrollIntoViewIfNeeded();
  await expect(thinking).toBeInViewport({ ratio: 1 });
  const model = card.getByLabel('模型', { exact: true });
  await model.fill('gemini-3.5-transcribe');
  await model.blur();
  await expect(thinking).toHaveCount(0);
  await model.fill('gemini-3.5-flash-lite');
  await model.blur();
  await expect(thinking).toHaveAttribute('aria-checked', 'true');
  await thinking.click();
  await expect(thinking).toHaveAttribute('aria-checked', 'false');
});

async function finishSettingsOperation(page: Page, operation: 'Save' | 'Sync', error = false) {
  await page.evaluate(({ operation, error }) => {
    const fixture = (window as unknown as {
      settingsStatusFixture: { finishSave: (error: boolean) => void; finishSync: (error: boolean) => void };
    }).settingsStatusFixture;
    fixture[`finish${operation}`](error);
  }, { operation, error });
}

for (const scale of ['small', 'medium', 'large']) {
  for (const theme of ['light', 'dark']) {
    test(`real Save Configuration updates one compact status surface at ${scale}/${theme}`, async ({ page }) => {
      await page.goto(`/e2e/fixtures/settings-status.html?scale=${scale}&theme=${theme}`);
      const input = page.getByLabel('DeepSeek suite API Key');
      await expect(input).toHaveValue('fixture-key');
      await input.fill('  saved-key  ');
      const save = page.getByRole('button', { name: '保存配置', exact: true });
      await save.click();
      const notice = page.locator('.ol-notification-card');
      await expect(notice).toHaveCount(1);
      await expect(notice).toHaveAttribute('data-state', 'saving');
      await notice.evaluate((node) => node.setAttribute('data-same-operation', 'save'));
      await expect(save).toBeDisabled();
      await expect(notice).not.toContainText('已保存');
      await finishSettingsOperation(page, 'Save');
      await expect(notice).toHaveAttribute('data-state', 'saved');
      await expect(notice).toHaveAttribute('data-same-operation', 'save');
      await expect(notice).toContainText('DeepSeek 配置已保存');
      await expect(notice).toHaveCSS('border-radius', '8px');
      await expect(save).toBeEnabled();
      expect(await page.evaluate(() => JSON.parse((window as any).settingsStatusFixture.stored()).apiKey)).toBe('saved-key');
      await expect(notice).toHaveCSS('transform', 'none');
      const box = (await notice.boundingBox())!;
      const viewport = (await page.locator('.ol-app-viewport').boundingBox())!;
      expect(box.height).toBeLessThan(42);
      expect(box.x).toBeGreaterThanOrEqual(viewport.x);
      expect(box.x + box.width).toBeLessThanOrEqual(viewport.x + viewport.width);
      await page.screenshot({ path: test.info().outputPath(`save-status-${scale}-${theme}.png`) });
      await save.click();
      await finishSettingsOperation(page, 'Save', true);
      await expect(notice).toHaveAttribute('data-variant', 'alert');
      await expect(notice).toContainText('操作失败');
      await expect(notice.locator('[role="alert"]')).toBeVisible();
      await page.evaluate(() => (window as any).settingsStatusFixture.hide());
      await expect(notice).toHaveCount(0);
    });
  }
}

test('Sync Now keeps one progress notice until native completion; actual decisions remain modal', async ({ page }) => {
  await page.goto('/e2e/fixtures/settings-status.html?theme=dark');
  const sync = page.getByRole('button', { name: '立即同步', exact: true });
  await expect(sync).toBeEnabled();
  await sync.click();
  const notice = page.locator('.ol-notification-card');
  await expect(notice).toHaveCount(1);
  await expect(notice).toContainText('正在同步云端最新内容');
  await expect(notice.getByRole('progressbar')).toBeVisible();
  await notice.evaluate((node) => node.setAttribute('data-same-operation', 'sync'));
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(notice).not.toContainText('同步完成');
  await page.evaluate(() => (window as any).settingsStatusFixture.restoreDuringSync());
  await expect(notice.getByRole('progressbar')).toBeVisible();
  await expect(notice).not.toContainText('已恢复');
  await finishSettingsOperation(page, 'Sync');
  await expect(notice).toHaveAttribute('data-state', 'saved');
  await expect(notice).toHaveAttribute('data-same-operation', 'sync');
  await expect(notice).toContainText('同步完成');
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  await expect(sync).toBeEnabled();
  await expect(notice).toHaveCSS('transform', 'none');
  await page.screenshot({ path: test.info().outputPath('cloud-sync-completed-status.png') });
  await sync.click();
  await finishSettingsOperation(page, 'Sync', true);
  await expect(notice).toHaveAttribute('data-state', 'failed');
  await expect(notice).toHaveAttribute('data-severity', 'error');
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  await notice.getByRole('button', { name: '关闭', exact: true }).click();
  await expect(notice).toHaveCount(0);

  const remove = page.getByRole('button', { name: '删除云端备份', exact: true });
  await remove.click();
  const dialog = page.getByRole('dialog', { name: '删除云端加密备份？', exact: true });
  await expect(dialog).toBeVisible();
  await expect(page.locator('[role="dialog"]')).toHaveCount(1);
  await expect(dialog).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(dialog.getByRole('button', { name: '关闭', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(remove).toBeFocused();
  expect(await page.evaluate(() => (window as any).settingsStatusFixture.calls)).toEqual([
    'cloud_sync_e2ee_sync_now', 'cloud_sync_e2ee_sync_now',
  ]);
  await remove.click();
  const overlay = page.locator('.ol-dialog-overlay:not([inert])');
  const bounds = (await overlay.boundingBox())!;
  await page.mouse.click(bounds.x + 2, bounds.y + 2);
  await expect(dialog).toHaveCount(0);
  await expect(remove).toBeFocused();
  await sync.click();
  await expect(notice.getByRole('progressbar')).toBeVisible();
  await page.evaluate(() => (window as any).settingsStatusFixture.hide());
  await finishSettingsOperation(page, 'Sync');
  await expect(notice).toHaveCount(0);
});

test('saved status and the sync decision remain usable in a short narrow 110% viewport', async ({ page }) => {
  await page.goto('/e2e/fixtures/settings-status.html?scale=large&theme=light&width=330&height=260');
  await expect(page.getByLabel('DeepSeek suite API Key')).toHaveValue('fixture-key');
  await page.getByRole('button', { name: '保存配置', exact: true }).click();
  await finishSettingsOperation(page, 'Save');
  const notice = page.locator('.ol-notification-card');
  await expect(notice).toHaveAttribute('data-state', 'saved');
  await expect(notice).toHaveCSS('transform', 'none');
  const box = (await notice.boundingBox())!;
  const viewport = (await page.locator('.ol-app-viewport').boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(viewport.x);
  expect(box.x + box.width).toBeLessThanOrEqual(viewport.x + viewport.width);
  const remove = page.getByRole('button', { name: '删除云端备份', exact: true });
  await remove.click();
  const dialog = page.getByRole('dialog', { name: '删除云端加密备份？', exact: true });
  await expect(dialog).toBeVisible();
  const confirm = dialog.getByRole('button', { name: '删除云端备份', exact: true });
  await confirm.scrollIntoViewIfNeeded();
  await expect(confirm).toBeDisabled();
  await page.screenshot({ path: test.info().outputPath('cloud-sync-decision-narrow-light.png') });
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(remove).toBeFocused();
});
