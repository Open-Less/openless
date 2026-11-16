import { expect, test, type Page } from '@playwright/test';

test.use({ baseURL: 'http://127.0.0.1:5326' });

async function openAiSettings(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await page.waitForTimeout(300);
}

async function openVoiceEnhancement(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '高级功能', exact: true }).click();
  await page.getByRole('button').filter({ hasText: '语音增强与播放抑制' }).click();
  await page.waitForTimeout(800);
}

async function expectRed(element: ReturnType<Page['locator']>) {
  const color = await element.evaluate((node) => getComputedStyle(node).color);
  const [red, green] = color.match(/\d+/g)!.map(Number);
  expect(red).toBeGreaterThan(200);
  expect(green).toBeLessThan(160);
}

test('sidebar items highlight on hover without a transition, and the brand returns to overview', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  const history = page
    .locator('.ol-sidebar-surface .ol-nav-btn')
    .filter({ hasText: '历史' })
    .first();
  await history.hover();
  await page.waitForTimeout(120);
  const background = await history.evaluate((node) => getComputedStyle(node).backgroundColor);
  expect(background).not.toBe('rgba(0, 0, 0, 0)');

  await history.click();
  await expect(page.getByRole('heading', { name: '历史记录', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '返回概览', exact: true }).click();
  await expect(page.getByRole('heading', { name: '今日概览', exact: true })).toBeVisible();
});

test('channel deletion uses a blocking centered confirmation and a red trash toast', async ({
  page,
}) => {
  await openAiSettings(page);
  await page.locator('.ol-channel-edit-button').first().click();
  await expect(page.locator('.ol-channel-dialog')).toBeVisible();

  await page.locator('.ol-channel-delete-button').click();
  const confirm = page.locator('.ol-confirm-dialog');
  await expect(confirm).toBeVisible();
  await page.waitForTimeout(900);
  await expect(confirm).toContainText('删除后该渠道保存的密钥也会一并清除。');
  await expect(page.locator('.ol-channel-delete-confirm')).toHaveCount(0);
  await expectRed(confirm.locator('.ol-confirm-dialog-description'));

  const box = (await confirm.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(Math.abs(box.x + box.width / 2 - viewport.width / 2)).toBeLessThan(10);
  expect(Math.abs(box.y + box.height / 2 - viewport.height / 2)).toBeLessThan(10);

  const confirmDelete = confirm.getByRole('button', { name: '确认删除', exact: true });
  const cancel = confirm.getByRole('button', { name: '取消', exact: true });
  expect((await confirmDelete.boundingBox())!.x).toBeLessThan((await cancel.boundingBox())!.x);

  await cancel.click();
  await expect(confirm).toHaveCount(0);
  await expect(page.locator('.ol-channel-dialog')).toBeVisible();

  await page.locator('.ol-channel-delete-button').click();
  await confirm.getByRole('button', { name: '确认删除', exact: true }).click();
  await expect(page.locator('.ol-channel-dialog')).toHaveCount(0);
  const toast = page.locator('.ol-notification-card').filter({ hasText: '已删除' });
  await expect(toast).toBeVisible();
  const icon = toast.locator('.ol-notification-icon--deleted');
  await expect(icon).toBeVisible();
  const iconColor = await icon.evaluate((node) => getComputedStyle(node).backgroundColor);
  const [red, green] = iconColor.match(/\d+/g)!.map(Number);
  expect(red).toBeGreaterThan(200);
  expect(green).toBeLessThan(160);
});

test('toggling echo suppression does not nudge the voice enhancement switch', async ({ page }) => {
  await openVoiceEnhancement(page);
  const switches = page.locator('.ol-switch');
  await expect(switches).toHaveCount(2);
  const second = switches.nth(1);
  const before = (await second.boundingBox())!;
  await switches.first().click();
  await page.waitForTimeout(120);
  const animations = await second.evaluate((node) => node.getAnimations().length);
  expect(animations).toBe(0);
  const after = (await second.boundingBox())!;
  expect(Math.abs(after.y - before.y)).toBeLessThan(0.5);
});

test('language search is one field instead of a box inside a box', async ({ page }) => {
  await page.goto('/?os=mac');
  const sidebar = page.locator('.ol-sidebar-surface');
  await sidebar.getByRole('button', { name: '工具', exact: true }).click();
  await sidebar.getByRole('button', { name: '翻译', exact: true }).click();
  await expect(page.getByRole('heading', { name: '翻译', exact: true })).toBeVisible();

  const label = page.locator('.ol-language-search');
  const input = label.locator('input');
  const labelBox = (await label.boundingBox())!;
  const inputBox = (await input.boundingBox())!;
  expect(inputBox.height).toBeGreaterThan(labelBox.height - 4);
  await expect(input).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(input).toHaveCSS('border-top-width', '0px');

  await label.click({ position: { x: 12, y: 12 } });
  await expect(input).toBeFocused();
});

test('general subpages reuse the advanced header and back control', async ({ page }) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '通用', exact: true }).click();
  await page.getByRole('button').filter({ hasText: '麦克风与录音' }).click();
  await expect(page.getByRole('heading', { name: '麦克风与录音', exact: true })).toBeVisible();
  await expect(page.locator('.ol-settings-back')).toBeVisible();
  await expect(page.locator('.ol-settings-back-button')).toHaveCount(0);
  await expect(page.getByRole('heading', { name: '通用', exact: true })).toHaveCount(0);
});

test('experimental ASR keeps the switch after a fixed-width action slot', async ({ page }) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '高级功能', exact: true }).click();
  await page.getByRole('button').filter({ hasText: '实验性本地语音模型' }).click();
  const actions = page
    .locator('[data-experimental-model="text-stream"] .ol-experimental-model-actions')
    .first();
  await expect(actions).toBeVisible();
  const children = await actions.evaluate((element) =>
    [...element.children].map((child) => child.className),
  );
  expect(children[0]).toContain('ol-experimental-model-action-slot');
  expect(children[1]).toContain('ol-switch');
});

test('usage tabs and footer stay compact without an extra active box', async ({ page }) => {
  await page.goto('/?os=mac');
  const usage = page.locator('.ol-sidebar-surface .ol-nav-btn').filter({ hasText: '用量' }).first();
  await usage.click();
  await expect(page.getByRole('heading', { name: '用量', exact: true })).toBeVisible();
  for (const tab of await page.locator('nav[aria-label="用量页面"] button').all()) {
    await expect(tab).toHaveCSS('border-bottom-color', 'rgba(0, 0, 0, 0)');
  }
  expect((await usage.boundingBox())!.width).toBeLessThan(120);
  await expect(usage).toHaveCSS('justify-self', 'end');
});

test('style pack deletion uses the red trash notification and keeps undo', async ({ page }) => {
  await page.goto('/?os=mac');
  const sidebar = page.locator('.ol-sidebar-surface');
  await sidebar.getByRole('button', { name: '风格', exact: true }).click();
  await sidebar.getByRole('button', { name: '润色模式', exact: true }).click();
  await expect(page.getByRole('heading', { name: '风格包', exact: true })).toBeVisible();
  const deleteButtons = page.getByRole('button', { name: '删除', exact: true });
  await expect(deleteButtons.first()).toBeVisible();
  await deleteButtons.first().click();
  const toast = page.locator('.ol-notification-card').filter({ hasText: '已删除' });
  await expect(toast).toBeVisible();
  const icon = toast.locator('.ol-notification-icon--deleted');
  await expect(icon).toBeVisible();
  const iconColor = await icon.evaluate((node) => getComputedStyle(node).backgroundColor);
  const [red, green] = iconColor.match(/\d+/g)!.map(Number);
  expect(red).toBeGreaterThan(200);
  expect(green).toBeLessThan(160);
  await expect(toast.getByRole('button', { name: '撤销', exact: true })).toBeVisible();
});

test('selected segmented options and the overview heatmap stay visible', async ({ page }) => {
  await page.goto('/?os=mac');
  const sidebar = page.locator('.ol-sidebar-surface');
  await sidebar.getByRole('button', { name: '用量', exact: true }).click();
  await expect(page.getByRole('heading', { name: '用量', exact: true })).toBeVisible();
  const usageSelected = page.locator('.ol-segmented-option[aria-pressed="true"]').first();
  await expect(usageSelected).toBeVisible();
  await expect(usageSelected).toHaveCSS('clip-path', 'none');
  await expect(usageSelected).toHaveCSS('border-radius', '12px');
  const usageStyle = await usageSelected.evaluate((node) => ({
    color: getComputedStyle(node).color,
    background: getComputedStyle(node).backgroundColor,
  }));
  expect(usageStyle.background).not.toBe('rgba(0, 0, 0, 0)');
  expect(usageStyle.color).not.toBe(usageStyle.background);

  await sidebar.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  const pipelineSelected = page.locator(
    '.ol-service-pipeline-track .ol-segmented-option[aria-pressed="true"]',
  );
  await expect(pipelineSelected).toBeVisible();
  await expect(pipelineSelected).toHaveCSS('clip-path', 'none');
  await expect(pipelineSelected).toHaveCSS('border-radius', '12px');
  const pipelineStyle = await pipelineSelected.evaluate((node) => ({
    color: getComputedStyle(node).color,
    background: getComputedStyle(node).backgroundColor,
  }));
  expect(pipelineStyle.background).not.toBe('rgba(0, 0, 0, 0)');
  expect(pipelineStyle.color).not.toBe(pipelineStyle.background);

  await page.getByRole('button', { name: '返回应用', exact: true }).click();
  await page.waitForTimeout(400);
  await sidebar.getByRole('button', { name: '概览', exact: true }).click();
  await expect(page.getByRole('heading', { name: '今日概览', exact: true })).toBeVisible();
  const overviewSelected = page
    .getByRole('group', { name: '统计周期' })
    .locator('.ol-segmented-option[aria-pressed="true"]');
  await expect(overviewSelected).toBeVisible();
  await expect(overviewSelected).toHaveCSS('clip-path', 'none');
  await expect(overviewSelected).toHaveCSS('border-radius', '12px');
  const overviewStyle = await overviewSelected.evaluate((node) => ({
    color: getComputedStyle(node).color,
    background: getComputedStyle(node).backgroundColor,
  }));
  expect(overviewStyle.background).not.toBe('rgba(0, 0, 0, 0)');
  expect(overviewStyle.color).not.toBe(overviewStyle.background);
  // The yearly card is the ported GitHub Activity component: its heading is the yearly
  // count line and its grid is made of real day cells.
  const activityCard = page.locator('.ol-gha');
  await expect(activityCard).toBeVisible();
  await expect(activityCard.locator('.ol-gha-heading')).toContainText('次听写');
  expect(await activityCard.locator('.ol-gha-cell').count()).toBeGreaterThan(300);
  await expect(activityCard.locator('.ol-gha-app-chips')).toBeVisible();
});
