import { FONT_SCALE_VALUES } from '../src/lib/fontScale';
import type { SettingsOperation, SettingsStatusFixture } from './fixtures/settings-status';
import { fixtureOrigin } from './support/server';
import { openExperiments } from './support/experiments';
import { settleNavigation } from './support/navigation';
import { expect, test, type Page } from '@playwright/test';

// Development React is intentional: production builds hide special-prop/ref warnings.
test.use({ baseURL: fixtureOrigin });
const messages = new WeakMap<Page, string[]>();

declare global {
  interface Window {
    settingsStatusFixture: SettingsStatusFixture;
  }
}
test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  messages.set(page, errors);
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => {
    // Chromium's software WebGL renderer reports GPU readback noise unrelated to React.
    if (message.text().includes('GL Driver Message')) return;
    if (message.type() === 'error' || message.type() === 'warning') errors.push(message.text());
  });
});
test.afterEach(async ({ page }) => {
  expect(messages.get(page)).toEqual([]);
});

async function settings(page: Page, category: string) {
  await page.goto('/');
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
  await page.goto('/');
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '润色模式', exact: true }).click();
  await expect(page.locator('[data-ol-style-pack]').first()).toBeVisible();
  const switcher = page.locator('main .ol-segmented[data-compact]');
  const tabs = switcher.getByRole('button');
  const indicator = switcher.locator('.ol-segmented-indicator');
  await expect(tabs).toHaveCount(2);
  await expect(indicator).toBeVisible();
  const initial = (await indicator.boundingBox())!;
  await tabs.nth(1).click();
  await expect(tabs.nth(1)).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(async () => (await indicator.boundingBox())!.x).toBeGreaterThan(initial.x);
  await expect(page.locator('[data-ol-style-pack]').first()).toBeVisible();
  await tabs.nth(0).click();
  await expect(tabs.nth(0)).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(async () => (await indicator.boundingBox())!.x).toBeCloseTo(initial.x, 0);
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

async function waitForSettingsOperation(page: Page, operation: SettingsOperation, attempt: number) {
  await expect
    .poll(() =>
      page.evaluate((kind) => window.settingsStatusFixture.pendingAttempt(kind), operation),
    )
    .toBe(attempt);
}

async function finishSettingsOperation(
  page: Page,
  operation: SettingsOperation,
  attempt: number,
  error = false,
) {
  await waitForSettingsOperation(page, operation, attempt);
  await page.evaluate(
    ({ operation, attempt, error }) =>
      window.settingsStatusFixture.finish(operation, attempt, error),
    { operation, attempt, error },
  );
}

for (const scale of ['small', 'medium', 'large'] as const) {
  for (const theme of ['light', 'dark']) {
    test(`real Save Configuration updates one compact status surface at ${scale}/${theme}`, async ({
      page,
    }) => {
      await page.goto(`/e2e/fixtures/settings-status.html?scale=${scale}&theme=${theme}`);
      const input = page.getByLabel('DeepSeek suite API Key');
      await expect(input).toHaveValue('fixture-key');
      await input.fill('  saved-key  ');
      const save = page.getByRole('button', { name: '保存配置', exact: true });
      await save.click();
      const notice = page.locator('.ol-notification-card:not([aria-hidden])');
      await expect(notice).toHaveCount(1);
      await expect(notice).toHaveAttribute('data-state', 'saving');
      await notice.evaluate((node) => node.setAttribute('data-same-operation', 'save'));
      await expect(save).toBeDisabled();
      await expect(notice).not.toContainText('已保存');
      await finishSettingsOperation(page, 'save', 1);
      await expect(notice).toHaveAttribute('data-state', 'saved');
      await expect(notice).toHaveAttribute('data-same-operation', 'save');
      await expect(notice).toContainText('DeepSeek 配置已保存');
      const surface = notice.locator('.ol-toast-panel');
      await expect(surface).toHaveCSS('clip-path', /^path\(/);
      expect(
        await surface.evaluate((node: HTMLElement) => [
          node.offsetWidth,
          node.offsetHeight,
          node.style.borderRadius,
        ]),
      ).toEqual([356, 56, '21px']);
      await expect(save).toBeEnabled();
      expect(
        await page.evaluate(() => JSON.parse(window.settingsStatusFixture.stored()).apiKey),
      ).toBe('saved-key');
      await expect(notice).toHaveCSS('transform', 'none');
      const box = (await notice.boundingBox())!;
      const viewport = (await page.locator('.ol-app-viewport').boundingBox())!;
      // Confirm that the scaled card stays within the viewport as well as its CSS box.
      expect(box.height).toBeLessThanOrEqual(60 * FONT_SCALE_VALUES[scale]);
      expect(box.x).toBeGreaterThanOrEqual(viewport.x);
      expect(box.x + box.width).toBeLessThanOrEqual(viewport.x + viewport.width);
      await page.screenshot({
        path: test.info().outputPath(`acceptance-save-status-${scale}-${theme}.png`),
      });
      const priorSave = await notice.elementHandle();
      await save.click();
      await expect(notice).toHaveAttribute('data-state', 'saving');
      await expect(notice).not.toHaveAttribute('data-same-operation', 'save');
      expect(
        await priorSave!.evaluate(
          (node) => !node.isConnected || node.getAttribute('aria-hidden') === 'true',
        ),
      ).toBe(true);
      await finishSettingsOperation(page, 'save', 2, true);
      await expect(notice).toHaveAttribute('data-variant', 'alert');
      await expect(notice).toContainText('操作失败');
      await expect(notice.locator('[role="alert"]')).toBeVisible();
      await page.evaluate(() => window.settingsStatusFixture.hide());
      await expect(notice).toHaveCount(0);
    });
  }
}

test('Sync Now keeps one spinner-only notice until native completion', async ({ page }) => {
  await page.goto('/e2e/fixtures/settings-status.html?theme=dark&delaySyncStart');
  const sync = page.getByRole('button', { name: '立即同步', exact: true });
  const warningOffset = () =>
    page
      .locator('.ol-cloud-sync-scope')
      .evaluate(
        (node) =>
          node.getBoundingClientRect().top -
          node.closest('.ol-cloud-sync')!.getBoundingClientRect().top,
      );
  await expect(sync).toBeEnabled();
  const warningTop = await warningOffset();
  await sync.click();
  const notice = page.locator('.ol-notification-card');
  await expect(notice).toHaveCount(1);
  await expect(notice).toContainText('正在同步云端最新内容');
  await expect(notice.locator('.ol-toast-spinner')).toBeVisible();
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  await notice.evaluate((node) => node.setAttribute('data-same-operation', 'sync'));
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(notice).not.toContainText('同步完成');
  await waitForSettingsOperation(page, 'sync', 1);
  const cancel = page.getByRole('button', { name: '取消当前任务', exact: true });
  const noticeCancel = notice.getByRole('button', { name: '取消同步', exact: true });
  await expect(noticeCancel).toHaveCount(0);
  await page.evaluate(() => window.settingsStatusFixture.startSync());
  await expect(noticeCancel).toBeEnabled();
  await expect(cancel).toHaveCount(0);
  await expect(noticeCancel).toHaveAttribute('data-action-style', 'cancel-icon');
  expect(await warningOffset()).toBeCloseTo(warningTop, 0);
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.canRestoreDuringSync()))
    .toBe(true);
  await page.evaluate(() => window.settingsStatusFixture.restoreDuringSync());
  await expect(notice).toHaveAttribute('data-state', 'saving');
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  await expect(notice).not.toContainText('已恢复');
  await page.evaluate(() => window.settingsStatusFixture.commitSync());
  await noticeCancel.click();
  await expect(notice).toContainText('已请求取消');
  await expect(noticeCancel).toBeDisabled();
  await expect(sync).toBeDisabled();
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.cancelledSyncTasks.length))
    .toBe(1);
  await finishSettingsOperation(page, 'sync', 1);
  await expect(notice).toHaveAttribute('data-state', 'saved');
  await expect(notice).toHaveAttribute('data-same-operation', 'sync');
  await expect(notice).toContainText('同步完成');
  await expect(cancel).toHaveCount(0);
  await expect(page.locator('.ol-cloud-sync-backup small')).toContainText('上次同步：');
  expect(await warningOffset()).toBeCloseTo(warningTop, 0);
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  await expect(sync).toBeEnabled();
  await expect(notice).toHaveCSS('transform', 'none');
  await page.screenshot({
    path: test.info().outputPath('acceptance-cloud-sync-completed-status.png'),
  });
  await sync.click();
  await noticeCancel.click();
  await expect(notice).toContainText('已请求取消');
  await expect(noticeCancel).toBeDisabled();
  await expect(notice).toHaveAttribute('data-state', 'saving');
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  await expect(cancel).toHaveCount(0);
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.cancelledSyncTasks.length))
    .toBe(2);
  await finishSettingsOperation(page, 'sync', 2);
  await expect(notice).toContainText('任务已取消。');
  expect(await warningOffset()).toBeCloseTo(warningTop, 0);
  await expect(notice).toHaveAttribute('data-state', 'failed');
  await expect(notice).toHaveAttribute('data-severity', 'error');
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  await notice.getByRole('button', { name: '关闭', exact: true }).click();
  await expect(notice).toHaveCount(0);

  await sync.click();
  await expect(notice).toHaveAttribute('data-state', 'saving');
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  await waitForSettingsOperation(page, 'sync', 3);
  await page.evaluate(() => window.settingsStatusFixture.hide());
  await expect(sync).toHaveCount(0);
  await finishSettingsOperation(page, 'sync', 3);
  await expect(notice).toHaveCount(0);
  expect(await page.evaluate(() => window.settingsStatusFixture.calls)).toEqual([
    'cloud_sync_e2ee_sync_now',
    'cloud_sync_e2ee_sync_now',
    'cloud_sync_e2ee_sync_now',
  ]);
});

test('cloud sync keeps keyboard cancellation inside the welcome modal', async ({ page }) => {
  await page.goto('/e2e/fixtures/settings-status.html?cloudWelcome');
  await page.getByRole('button', { name: '从云端恢复', exact: true }).click();
  const dialog = page.getByRole('dialog');
  const sync = dialog.getByRole('button', { name: '立即同步', exact: true });
  const notice = page.locator('.ol-notification-card');
  const cancel = dialog.getByRole('button', { name: '取消当前任务', exact: true });
  await sync.click();
  await waitForSettingsOperation(page, 'sync', 1);
  await expect(notice).toHaveAttribute('data-state', 'saving');
  await expect(notice.getByRole('progressbar')).toHaveCount(0);
  const toastCancel = notice.getByRole('button', { name: '取消同步', exact: true });
  await expect(toastCancel).toBeEnabled();
  await expect(toastCancel).toHaveAttribute('tabindex', '-1');
  await expect(
    dialog.locator('header').getByRole('button', { name: '取消当前任务' }),
  ).toBeEnabled();
  await toastCancel.click();
  await expect(cancel).toBeDisabled();
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.cancelledSyncTasks.length))
    .toBe(1);
  await finishSettingsOperation(page, 'sync', 1);
  await expect(notice).toContainText('任务已取消。');
  await expect(cancel).toHaveCount(0);
  await expect(dialog).toBeVisible();

  await sync.click();
  await waitForSettingsOperation(page, 'sync', 2);
  const loading = notice.and(page.locator('[data-type="loading"]'));
  await loading.getByRole('button', { name: '关闭', exact: true }).click();
  await expect(loading).toHaveCount(0);
  expect(await page.evaluate(() => window.settingsStatusFixture.cancelledSyncTasks.length)).toBe(1);
  await cancel.focus();
  await expect(cancel).toBeFocused();
  await page.keyboard.press('Enter');
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.cancelledSyncTasks.length))
    .toBe(2);
  await finishSettingsOperation(page, 'sync', 2);
});

async function clickDecisionBackdrop(page: Page) {
  const overlay = page.locator('.ol-dialog-overlay:not([inert])');
  const bounds = (await overlay.boundingBox())!;
  await page.mouse.click(bounds.x + 2, bounds.y + 2);
}

test('cloud backup deletion cancels without IPC and holds the decision until confirmation finishes', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/settings-status.html?theme=dark');
  const remove = page.getByRole('main').getByRole('button', { name: '删除云端备份', exact: true });
  const dialog = page.getByRole('dialog', { name: '删除云端加密备份？', exact: true });
  const cancel = dialog.getByRole('button', { name: '取消', exact: true });
  const confirm = dialog.getByRole('button', { name: '删除云端备份', exact: true });
  const requests = () => page.evaluate(() => window.settingsStatusFixture.deleteRequests);
  const deleteRequest = {
    expectedVaultId: 'fixture-vault',
    observedRevision: '7',
    confirmed: true,
  };

  for (const dismiss of [
    () => cancel.click(),
    () => page.keyboard.press('Escape'),
    () => clickDecisionBackdrop(page),
  ]) {
    await remove.click();
    await expect(dialog).toBeVisible();
    await expect(cancel).toBeFocused();
    await dismiss();
    await expect(dialog).toHaveCount(0);
    await expect(remove).toBeFocused();
    expect(await requests()).toEqual([]);
  }

  await remove.click();
  await expect(page.getByRole('dialog')).toHaveCount(1);
  await expect(dialog.getByRole('checkbox')).toHaveCount(0);
  await expect(dialog.locator('.ol-confirm-dialog')).toContainText('本机资料保留');
  const close = dialog.getByRole('button', { name: '关闭', exact: true });
  await expect(cancel).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(close).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(confirm).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(close).toBeFocused();
  await confirm.click();
  await waitForSettingsOperation(page, 'delete', 1);
  await expect(confirm).toBeDisabled();
  await expect(cancel).toBeDisabled();
  await expect(close).toBeDisabled();
  await page.keyboard.press('Escape');
  await clickDecisionBackdrop(page);
  await confirm.dispatchEvent('click');
  await expect(dialog).toBeVisible();
  expect(await requests()).toEqual([deleteRequest]);
  await expect(page.locator('.ol-notification-card')).toHaveCount(0);

  await finishSettingsOperation(page, 'delete', 1, true);
  await expect(dialog.getByRole('alert')).toContainText('暂时无法连接同步服务');
  await expect(confirm).toBeEnabled();
  await expect(cancel).toBeEnabled();
  await confirm.click();
  await waitForSettingsOperation(page, 'delete', 2);
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('alert')).toHaveCount(0);
  expect(await requests()).toEqual([deleteRequest, deleteRequest]);
  await finishSettingsOperation(page, 'delete', 2);
  await expect(dialog).toHaveCount(0);
  await expect(remove).toBeDisabled();
  await expect(page.locator('.ol-notification-card')).toContainText('同步设置已更新。');
});

test('saved status and the sync decision remain usable in a short narrow 110% viewport', async ({
  page,
}) => {
  await page.goto(
    '/e2e/fixtures/settings-status.html?scale=large&theme=light&width=330&height=260',
  );
  await expect(page.getByLabel('DeepSeek suite API Key')).toHaveValue('fixture-key');
  await page.getByRole('button', { name: '保存配置', exact: true }).click();
  await finishSettingsOperation(page, 'save', 1);
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
  const cancel = dialog.getByRole('button', { name: '取消', exact: true });
  await expect(dialog).toHaveCSS('transform', 'none');
  await expect(cancel).toBeFocused();
  for (const action of [confirm, cancel]) {
    await expect(action).toBeEnabled();
    const button = (await action.boundingBox())!;
    expect(button.x).toBeGreaterThanOrEqual(viewport.x);
    expect(button.y).toBeGreaterThanOrEqual(viewport.y);
    expect(button.x + button.width).toBeLessThanOrEqual(viewport.x + viewport.width);
    expect(button.y + button.height).toBeLessThanOrEqual(viewport.y + viewport.height);
  }
  await page.screenshot({
    path: test.info().outputPath('acceptance-cloud-sync-decision-narrow-light.png'),
  });
  await cancel.click();
  await expect(dialog).toHaveCount(0);
  await expect(remove).toBeFocused();
});

test('Web Search reports failures on its page and completes a departed save quietly', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/settings-status.html?webSearch');
  await page.getByRole('button', { name: 'Open web search', exact: true }).click();
  await settleNavigation(page);
  const provider = page.getByRole('combobox', { name: 'Web Search' });
  const save = page.getByRole('button', { name: '保存', exact: true });
  const notice = page.locator('.ol-notification-card');
  const key = page.getByLabel('Search API Key');
  await expect(provider).toHaveValue('free');
  await expect(key).toHaveCount(0);
  await provider.selectOption('hybrid');
  await expect(save).toBeDisabled();
  await key.fill('test-deepseek-key');
  await expect(save).toBeEnabled();
  await provider.selectOption('off');
  await save.click();
  await finishSettingsOperation(page, 'save', 1, true);
  await expect(notice.getByRole('alert')).toHaveText('保存失败，请重试。');

  await save.click();
  await waitForSettingsOperation(page, 'save', 2);
  await page.evaluate(() => {
    const animate = Element.prototype.animate;
    Element.prototype.animate = function (frames, options) {
      const animation = animate.call(this, frames, options);
      if (this.matches('[data-ol-page="web-search"][data-transition-phase="exit"]')) {
        animation.pause();
        Element.prototype.animate = animate;
      }
      return animation;
    };
  });
  await page.getByRole('button', { name: 'Leave web search', exact: true }).click();
  const outgoing = page.locator('[data-ol-page="web-search"][data-transition-phase="exit"]');
  await expect(outgoing).toHaveAttribute('inert', '');
  await finishSettingsOperation(page, 'save', 2);
  await expect
    .poll(() => page.evaluate(() => JSON.parse(window.settingsStatusFixture.stored()).provider))
    .toBe('off');
  // The retained save handler finished while its form was still mounted.
  await expect(outgoing.locator('button')).not.toHaveAttribute('disabled', '');
  await expect(notice.filter({ hasText: '已保存' })).toHaveCount(0);
  await outgoing.evaluate((element) =>
    element
      .getAnimations()
      .find(({ id }) => id === 'ol-page-exit')
      ?.finish(),
  );
  await expect(page.getByRole('heading', { name: 'Next settings page' })).toBeVisible();
  await expect(notice).toHaveCount(0);
  await page.getByRole('button', { name: 'Open web search', exact: true }).click();
  await expect(provider).toHaveValue('off');
});

test('normal download cancellation is available only outside a modal and before terminal progress', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/settings-status.html?download');
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.hasDownloadListener()))
    .toBe(true);
  await page.evaluate(() => window.settingsStatusFixture.emitDownload('normal-model', 'started'));
  const card = page.locator('[data-notification-key="download:qwen3:normal-model"][data-type="loading"]');
  const cancel = card.getByRole('button', { name: '取消下载', exact: true });
  const result = page.locator('[data-notification-key="download:qwen3:normal-model"][data-type="success"]');
  await expect(card.getByRole('status')).toContainText('正在下载 normal-model');
  await expect(cancel).toBeEnabled();
  await expect(cancel).toHaveAttribute('title', '取消下载 normal-model');
  await expect(cancel).toHaveAttribute('data-action-style', 'cancel-icon');
  await expect(card.locator('.ol-toast-row [data-slot="toast-action"]')).toBeVisible();
  await page.getByRole('button', { name: 'Open download modal', exact: true }).click();
  await expect(page.getByRole('dialog', { name: 'Download modal' })).toBeVisible();
  await expect(cancel).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(cancel).toBeEnabled();

  await card.locator('.ol-notification-body').evaluate((element) =>
    element.setAttribute('data-same-job', 'true'),
  );
  await page.evaluate(() => {
    const button = document.querySelector<HTMLButtonElement>(
      '[data-notification-key="download:qwen3:normal-model"] [data-slot="toast-action"]',
    )!;
    const terminal = window.settingsStatusFixture.emitDownload('normal-model', 'finished');
    // A host event may invalidate the job before React commits its result.
    button.click();
    return terminal;
  });
  await expect(card).toHaveCount(0);
  await expect(result).toHaveAttribute('data-type', 'success');
  await expect(result.locator('.ol-notification-body')).toHaveAttribute('data-same-job', 'true');
  await expect(result.locator('[data-slot="toast-action"]')).toHaveCount(0);
  expect(await page.evaluate(() => window.settingsStatusFixture.cancelledDownloads)).toEqual([]);

  await page.evaluate(() => window.settingsStatusFixture.emitDownload('normal-model', 'started'));
  await expect(result).toHaveCount(0);
  await expect(card.locator('.ol-notification-body')).not.toHaveAttribute('data-same-job', 'true');
  await cancel.click();
  await expect(cancel).toBeDisabled();
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.cancelledDownloads))
    .toEqual(['normal-model']);
  await page.evaluate(async () => {
    const { emitDownload } = window.settingsStatusFixture;
    await emitDownload('normal-model', 'cancelled');
    await emitDownload('normal-model', 'progress');
    await emitDownload('normal-model', 'finished');
  });
  await expect(card).toHaveCount(0);
  await expect(result).toHaveCount(0);

  await page.evaluate(() => window.settingsStatusFixture.emitDownload('broken-model', 'started'));
  await page.evaluate(() => window.settingsStatusFixture.emitDownload('broken-model', 'failed'));
  await expect(page.locator('[data-notification-key="download:qwen3:broken-model"][data-type="loading"]')).toHaveCount(
    0,
  );
  await expect(page.getByRole('alert').filter({ hasText: '模型下载失败' })).toContainText(
    'broken-model',
  );
  await expect(
    page.locator('[data-notification-key="download:qwen3:broken-model"][data-type="warning"]'),
  ).toHaveAttribute('data-type', 'warning');
});
