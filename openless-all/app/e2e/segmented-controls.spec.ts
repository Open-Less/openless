import { expect, test, type Locator, type Page } from '@playwright/test';
import { fixtureOrigin } from './support/server';
import { settleNavigation } from './support/navigation';

test.use({ baseURL: fixtureOrigin });

async function chooseAnother(group: Locator) {
  const indicator = group.locator(':scope > .ol-segmented-indicator');
  await expect(indicator).toBeVisible();
  await expect(indicator).toHaveCount(1);
  const before = (await indicator.boundingBox())!;
  const next = group.locator('button[aria-pressed=false]:not(:disabled)').first();
  const label = (await next.innerText()).trim();
  await next.click();
  await expect(group.locator('button[aria-pressed=true]')).toHaveText(label);
  await expect
    .poll(async () => {
      const box = (await indicator.boundingBox())!;
      const chosen = (await group.locator('button[aria-pressed=true]').boundingBox())!;
      return Math.max(Math.abs(box.x - chosen.x), Math.abs(box.width - chosen.width));
    })
    .toBeLessThan(1);
  expect(Math.abs((await indicator.boundingBox())!.x - before.x)).toBeGreaterThan(1);
}

async function settings(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  return page.getByRole('dialog', { name: '设置', exact: true });
}

test('shared horizontal selectors follow selection in Overview, Usage, Dictionary, History and Style', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  const sidebar = page.locator('.ol-sidebar-surface');
  const groups = [
    ['概览', '.ol-overview-period-controls > .ol-segmented', 2],
    ['用量', '.ol-usage-controls .ol-segmented', 2],
    ['词典', '.ol-vocab-filters', 1],
  ] as const;
  for (const [view, selector, count] of groups) {
    await sidebar.getByRole('button', { name: view, exact: true }).click();
    await settleNavigation(page);
    await expect(page.locator(selector)).toHaveCount(count);
    for (const group of await page.locator(selector).all()) await chooseAnother(group);
  }
  await sidebar.getByRole('button', { name: '历史', exact: true }).click();
  await settleNavigation(page);
  await page.locator('[data-history-record="mock-quick-note"]').click();
  await chooseAnother(page.locator('.ol-history-view-segmented'));
  await sidebar.getByRole('button', { name: '风格', exact: true }).click();
  await sidebar.getByRole('button', { name: '润色模式', exact: true }).click();
  await settleNavigation(page);
  await chooseAnother(page.locator('main .ol-segmented'));
});

test('shared horizontal selectors follow selection across desktop settings', async ({ page }) => {
  await page.goto('/?os=mac');
  await page.evaluate(async () => {
    const { updateSettingFields } = await import('/src/api/preferences.ts');
    await updateSettingFields({ '/selectionVoiceEnabled': true });
  });
  const dialog = await settings(page);
  const category = async (label: string) => {
    await dialog.getByRole('button', { name: label, exact: true }).click();
    await settleNavigation(dialog);
  };
  const general = async (id: string) => {
    await category('外观与语言');
    await category('通用');
    await dialog.locator(`[data-ol-general-entry="${id}"]`).click();
    await settleNavigation(dialog);
  };
  await general('recording');
  await chooseAnother(dialog.getByRole('group', { name: '录音方式', exact: true }));
  await general('capsule');
  await chooseAnother(dialog.locator('.ol-capsule-style-segmented'));
  await general('phone');
  await chooseAnother(dialog.getByRole('group', { name: '默认录音方式', exact: true }));
  await general('textInput');
  await dialog.getByRole('button', { name: '更多', exact: true }).click();
  await chooseAnother(page.getByRole('group', { name: '编辑方案格式', exact: true }));
  await page.keyboard.press('Escape');
  await category('外观与语言');
  await chooseAnother(dialog.getByRole('group', { name: '主题', exact: true }));
  await category('快捷键');
  await page.setViewportSize({ width: 820, height: 850 });
  const filters = dialog.locator('.ol-shortcut-filter-options');
  await expect(filters).toHaveCount(2);
  for (const group of await filters.all()) await chooseAnother(group);
});

test('the model catalog engine filter and shared chip wrapper use the same moving indicator', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/refinements.html?catalog');
  const engines = page.locator('.ol-model-filters');
  await chooseAnother(engines);
  await expect(page.locator('.ol-model-catalog-list .ol-model-choice')).toHaveCount(1);
  await page.goto('/e2e/fixtures/refinements.html');
  await chooseAnother(page.getByRole('group', { name: 'Choices', exact: true }));
});
