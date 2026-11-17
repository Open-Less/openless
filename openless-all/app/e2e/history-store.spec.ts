import { expect, test, type Page } from '@playwright/test';

async function navigate(page: Page, key: string, label: string) {
  await page.getByRole('button', { name: label, exact: true }).click();
  const surface = page.locator(`[data-ol-page="${key}"]`);
  await expect(surface).toHaveAttribute('data-transition-phase', 'settled');
  return surface;
}

test('history writes update every page while recordings, activity and the usage ledger stay independent', async ({
  page,
}) => {
  await page.goto('/?os=win');
  await expect(page.locator('html')).toHaveAttribute('data-ol-platform', 'win');
  const initialHeading = await page.locator('.ol-gha-heading').textContent();
  const initialApps = await page.locator('.ol-gha-app-chips').textContent();
  expect(initialHeading).toBeTruthy();
  expect(initialApps).toBeTruthy();
  await navigate(page, 'history', '历史');
  const records = page.locator('[data-history-record]');
  await expect(records).toHaveCount(9);
  await page.keyboard.press('Control+k');
  const search = page.getByRole('searchbox');
  await expect(search).toBeFocused();
  await expect(search).toHaveAttribute('placeholder', /Ctrl\+K/);
  const target = records.first();
  const deletedId = await target.getAttribute('data-history-record');
  await target.click();

  const actions = page.getByRole('button', { name: '录音操作', exact: true });
  const showDelete = async () => {
    await actions.click();
    await page.getByRole('menuitem', { name: '删除', exact: true }).click();
    const confirmation = page.getByRole('dialog', { name: '删除', exact: true });
    await expect(confirmation.getByRole('button', { name: '取消' })).toBeFocused();
    return confirmation;
  };
  await showDelete();
  await page.keyboard.press('Escape');
  await expect(actions).toBeFocused();
  await expect(records).toHaveCount(9);
  const deletion = await showDelete();
  await deletion.getByRole('button', { name: '删除', exact: true }).click();
  await expect(records).toHaveCount(8);
  await expect(page.getByText('已删除记录', { exact: true })).toBeVisible();

  await navigate(page, 'overview', '概览');
  await expect(page.locator(`[data-overview-record="${deletedId}"]`)).toHaveCount(0);
  await expect(page.locator('[data-overview-record]')).toHaveCount(8);
  await expect(page.locator('.ol-gha-heading')).toHaveText(initialHeading!);
  await expect(page.locator('.ol-gha-app-chips')).toHaveText(initialApps!);

  await navigate(page, 'history', '历史');
  await page.getByRole('button', { name: '清空', exact: true }).click();
  const clear = page.getByRole('dialog', { name: '清空', exact: true });
  await clear.getByRole('button', { name: '清空', exact: true }).click();
  await expect(records).toHaveCount(1);
  await expect(records).toHaveAttribute('data-history-record', 'mock-quick-note');
  const tools = page.getByRole('button', { name: '工具', exact: true });
  await tools.click();
  await expect(tools).toHaveAttribute('aria-expanded', 'true');
  await navigate(page, 'quickNote', '速记');
  await expect(records).toHaveAttribute('data-history-record', 'mock-quick-note');

  await navigate(page, 'overview', '概览');
  await expect(page.locator('[data-overview-record]')).toHaveCount(1);
  await expect(page.locator('.ol-gha-heading')).toHaveText(initialHeading!);
  await expect(page.locator('.ol-gha-app-chips')).toHaveText(initialApps!);

  await navigate(page, 'usage', '用量');
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '记录' })
    .click();
  await expect(page.getByRole('region', { name: '模型用量列表' })).toContainText('deepseek-v3-2');
  await expect(page.getByRole('region', { name: '最近记录列表' }).locator('time')).toHaveCount(0);
});

for (const [font, scale] of [
  ['small', 0.9],
  ['medium', 1],
  ['large', 1.1],
] as const) {
  test(`overview stays scrollable and keeps 11px heatmap cells at ${Math.round(scale * 100)}%`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 980, height: 640 });
    await page.addInitScript((value) => localStorage.setItem('ol-font-scale', value), font);
    await page.goto('/?os=mac');
    const dashboard = page.locator('.ol-overview-dashboard');
    await expect(dashboard).toBeVisible();
    const calendar = page.locator('.ol-gha');
    await expect(calendar).toBeVisible();
    const cell = page.locator('.ol-gha-cell').first();
    expect(
      await cell.evaluate((node) => Number.parseFloat(getComputedStyle(node).width)),
    ).toBeCloseTo(11, 1);
    expect(
      await cell.evaluate((node) => Number.parseFloat(getComputedStyle(node.parentElement!).gap)),
    ).toBeCloseTo(3, 1);
    await expect
      .poll(async () => Math.abs((await cell.boundingBox())!.width - 11 * scale))
      .toBeLessThan(0.2);
    const summary = page.locator('.ol-overview-year');
    await summary.scrollIntoViewIfNeeded();
    await expect(summary).toBeInViewport();
    const geometry = await dashboard.evaluate((element) => ({
      scrollWidth: element.scrollWidth,
      width: element.clientWidth,
      scrollHeight: element.scrollHeight,
      height: element.clientHeight,
    }));
    expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.width + 1);
    expect(geometry.scrollHeight).toBeGreaterThanOrEqual(geometry.height);
  });
}
