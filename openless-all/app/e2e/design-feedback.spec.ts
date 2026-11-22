import { expect, test, type Page } from '@playwright/test';
import { expectOverviewContained } from './support/overview';
import { settleNavigation } from './support/navigation';
import { fixtureOrigin } from './support/server';

test.use({ baseURL: fixtureOrigin });

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'medium'));
});

async function openAiSettings(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await settleNavigation(page);
}

async function openVoiceEnhancement(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '高级功能', exact: true }).click();
  await page.getByRole('button').filter({ hasText: '语音增强与播放抑制' }).click();
  await settleNavigation(page);
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
  await expect(history).toHaveCSS('transition-duration', '0s');
  const background = await history.evaluate((node) => getComputedStyle(node).backgroundColor);
  expect(background).not.toBe('rgba(0, 0, 0, 0)');

  await history.click();
  await expect(history).toHaveClass(/ol-nav-btn-active/);
  const activeBackground = await history.evaluate((node) => getComputedStyle(node).backgroundColor);
  expect(activeBackground).not.toBe('rgba(0, 0, 0, 0)');
  const badge = page.locator('.ol-version-badge');
  await expect(badge).toBeVisible();
  const badgeBackground = await badge.evaluate((node) => getComputedStyle(node).backgroundColor);
  expect(badgeBackground).not.toBe('rgba(0, 0, 0, 0)');
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
  await expect(confirm.getByRole('button', { name: '取消', exact: true })).toBeFocused();
  await expect(confirm).toHaveCSS('opacity', '1');
  await expect(page.locator('.ol-channel-delete-confirm')).toHaveCount(0);
  await expect(confirm.locator('.ol-channel-delete-button')).toHaveCount(0);
  await expect(confirm.locator('.ol-confirm-dialog-title')).toHaveCSS('font-size', '18px');
  const description = confirm.locator('.ol-confirm-dialog-description');
  await expect(description).toHaveCSS('font-size', '13.5px');
  const descriptionColor = await description.evaluate((node) => getComputedStyle(node).color);
  const [descRed, descGreen] = descriptionColor.match(/\d+/g)!.map(Number);
  expect(Math.abs(descRed - descGreen)).toBeLessThan(40);

  const viewport = (await page.locator('.ol-app-viewport').boundingBox())!;
  await expect
    .poll(async () => {
      const box = (await confirm.boundingBox())!;
      return Math.max(
        Math.abs(box.x + box.width / 2 - viewport.x - viewport.width / 2),
        Math.abs(box.y + box.height / 2 - viewport.y - viewport.height / 2),
      );
    })
    .toBeLessThan(10);

  const confirmDelete = confirm.getByRole('button', { name: '确认删除', exact: true });
  const cancel = confirm.getByRole('button', { name: '取消', exact: true });
  const confirmFill = await confirmDelete.evaluate(
    (node) => getComputedStyle(node).backgroundColor,
  );
  const [fillRed, fillGreen] = confirmFill.match(/\d+/g)!.map(Number);
  expect(fillRed).toBeGreaterThan(180);
  expect(fillGreen).toBeLessThan(120);
  expect((await cancel.boundingBox())!.x).toBeLessThan((await confirmDelete.boundingBox())!.x);

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

test('noise reduction uses one slider track', async ({ page }) => {
  await openVoiceEnhancement(page);
  await page.getByRole('button').filter({ hasText: '语音增强设置' }).click();
  const denoise = page.getByRole('slider', { name: '降噪强度', exact: true });
  await expect(denoise).toHaveCount(1);
  const control = page.locator('.ol-slider-control').filter({ has: denoise });
  const track = control.locator('.ol-slider-track');
  await expect(track).toHaveCount(1);
  await expect(track).toBeVisible();
  await expect(track).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
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
  await settleNavigation(page);
  const usageNav = page.locator('nav[aria-label="用量页面"]');
  await expect(usageNav.locator('.ol-segmented')).toBeVisible();
  await expect(usageNav).toHaveCSS('border-bottom-style', 'none');
  expect((await usage.boundingBox())!.width).toBeLessThan(120);
  await expect(usage).toHaveCSS('justify-self', 'end');
});

test('style pack deletion asks once, keeps the pack on cancel, and reports a committed deletion', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  const sidebar = page.locator('.ol-sidebar-surface');
  await sidebar.getByRole('button', { name: '风格', exact: true }).click();
  await sidebar.getByRole('button', { name: '润色模式', exact: true }).click();
  await expect(page.getByRole('heading', { name: '风格包', exact: true })).toBeVisible();
  const pack = page.locator('[data-ol-style-pack="imported.creator-note"]');
  const deleteButton = pack.getByRole('button', { name: '删除', exact: true });
  const dialog = page.getByRole('dialog', { name: '删除', exact: true });
  const toast = page.locator('.ol-notification-card[data-state="deleted"]');

  await deleteButton.click();
  await expect(dialog).toContainText('创作者口播');
  await expect(dialog).toContainText('市场上已发布的版本会保留');
  await expect(pack).toHaveCount(1);
  await expect(toast).toHaveCount(0);
  await dialog.getByRole('button', { name: '取消', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(pack).toBeVisible();

  await deleteButton.click();
  await dialog.getByRole('button', { name: '删除', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(pack).toHaveCount(0);
  await expect(toast).toContainText('创作者口播');
  const icon = toast.locator('.ol-notification-icon--deleted');
  await expect(icon).toBeVisible();
  const iconColor = await icon.evaluate((node) => getComputedStyle(node).backgroundColor);
  const [red, green] = iconColor.match(/\d+/g)!.map(Number);
  expect(red).toBeGreaterThan(200);
  expect(green).toBeLessThan(160);
  await expect(toast.getByRole('button', { name: '撤销', exact: true })).toHaveCount(0);
});

test('selected segmented options and the overview heatmap stay visible', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'medium'));
  await page.goto('/?os=mac');
  const sidebar = page.locator('.ol-sidebar-surface');
  await sidebar.getByRole('button', { name: '用量', exact: true }).click();
  await expect(page.getByRole('heading', { name: '用量', exact: true })).toBeVisible();
  const usageTab = page.locator('.ol-usage-tabs .ol-segmented-option[aria-pressed="true"]');
  await expect(usageTab).toBeVisible();
  const tabBackground = await usageTab.evaluate((node) => getComputedStyle(node).backgroundColor);
  expect(tabBackground).not.toBe('rgba(0, 0, 0, 0)');
  const usageSelected = page
    .getByRole('group', { name: '统计时段' })
    .locator('.ol-segmented-option[aria-pressed="true"]');
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
  const pipelineOptions = page.locator('.ol-service-pipeline-track .ol-segmented-option');
  await expect(pipelineOptions).toHaveCount(3);
  for (const option of await pipelineOptions.all()) {
    await expect(option).toBeVisible();
    expect((await option.innerText()).trim().length).toBeGreaterThan(0);
  }
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
  await expect(page.locator('.ol-settings-surface')).toBeHidden();
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
  const dashboard = page.locator('.ol-overview-dashboard');
  await expect
    .poll(() => dashboard.evaluate((element) => element.scrollHeight - element.clientHeight))
    .toBeLessThan(2);
  const activityCard = page.locator('.ol-gha');
  await expect(activityCard).toBeVisible();
  await expect(activityCard.locator('.ol-gha-heading')).toContainText('次听写');
  expect(await activityCard.locator('.ol-gha-cell').count()).toBeGreaterThan(300);
  await expect(activityCard.locator('.ol-gha-app-chips')).toBeVisible();
  const cellSize = await activityCard
    .locator('.ol-gha-cell')
    .first()
    .evaluate((node) => {
      const style = getComputedStyle(node);
      return { width: parseFloat(style.width), height: parseFloat(style.height) };
    });
  expect(Math.abs(cellSize.width - 11)).toBeLessThan(0.2);
  expect(Math.abs(cellSize.height - 11)).toBeLessThan(0.2);
  const heatmapBox = (await activityCard.boundingBox())!;
  expect(heatmapBox.width).toBeGreaterThan(950);
  await expect(activityCard).toHaveCSS('border-radius', '28px');
  await expect(activityCard).toHaveCSS('padding-left', '16px');
  const yearBoard = page.locator('.ol-overview-year');
  await expect(yearBoard).toBeVisible();
  const yearBox = (await yearBoard.boundingBox())!;
  const gridBox = (await activityCard.locator('.ol-gha-grid').boundingBox())!;
  const footerBox = (await activityCard.locator('.ol-gha-panel-header').boundingBox())!;
  expect(gridBox.width).toBeCloseTo(739, 0);
  expect(yearBox.x).toBeGreaterThan(gridBox.x + gridBox.width);
  expect(yearBox.x).toBeGreaterThan(footerBox.x + footerBox.width);
  expect(yearBox.x + yearBox.width).toBeLessThan(heatmapBox.x + heatmapBox.width);
  await expectOverviewContained(yearBoard);
  await expectOverviewContained(activityCard.locator('.ol-gha-panel-header'));
  await page.screenshot({ path: testInfo.outputPath('acceptance-overview-default-shell.png') });
});

test('marketplace search expands inside the same plate', async ({ page }) => {
  await page.goto('/?os=mac');
  const sidebar = page.locator('.ol-sidebar-surface');
  await sidebar.getByRole('button', { name: '风格', exact: true }).click();
  await sidebar.getByRole('button', { name: '风格市场', exact: true }).click();
  await expect(page.getByRole('heading', { name: '风格包市场', exact: true })).toBeVisible();
  const opener = page.getByRole('button', { name: '搜索名称 / 描述 / 标签…' });
  await opener.click();
  const plate = page.locator('.ol-search-open');
  const field = plate.locator('.ol-search-field');
  await expect(field).toBeVisible();
  await expect(plate).toHaveCSS('width', '220px');
  await expect(field).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(field).toHaveCSS('border-top-width', '0px');
  const plateBackground = await plate.evaluate((node) => getComputedStyle(node).backgroundColor);
  const iconBackground = await opener.evaluate((node) => getComputedStyle(node).backgroundColor);
  expect(iconBackground).toBe('rgba(0, 0, 0, 0)');
  expect(plateBackground).not.toBe('rgba(0, 0, 0, 0)');
  await expect(plate.locator('input')).toHaveCount(1);
});
