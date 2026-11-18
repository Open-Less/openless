import { expect, test, type Page } from '@playwright/test';
import { expectOverviewContained } from './support/overview';

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
  await tools.locator('..').locator('.ol-sidebar-nav-group').evaluate(async (group) => {
    await Promise.all(group.getAnimations().map((animation) => animation.finished));
  });
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
  test(`overview fits the minimum window at ${Math.round(scale * 100)}%`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width: 980, height: 640 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.addInitScript((value) => localStorage.setItem('ol-font-scale', value), font);
    await page.goto('/?os=mac');
    const surface = page.locator('[data-ol-page="overview"]');
    const dashboard = surface.locator('.ol-overview-dashboard');
    const calendar = dashboard.locator('.ol-gha-grid');
    const cells = calendar.locator('.ol-gha-cell');
    const summary = dashboard.locator('.ol-overview-year');
    const toggle = dashboard.getByRole('button', { name: '展开常用软件' });
    await expect(calendar.locator('.ol-gha-week')).toHaveCount(53);
    expect(await calendar.evaluate((node) => node.scrollLeft)).toBe(0);
    await expectOverviewContained(dashboard.locator('.ol-gha-week').first());
    await expectOverviewContained(dashboard.locator('.ol-gha-week').last());
    await expectOverviewContained(summary);
    await expectOverviewContained(toggle);
    await expectOverviewContained(dashboard.locator('.ol-overview-period-dates'));

    for (const owner of [page.locator('#root'), surface, dashboard, calendar]) {
      const excess = await owner.evaluate((node) => ({
        x: node.scrollWidth - node.clientWidth,
        y: node.scrollHeight - node.clientHeight,
      }));
      expect(excess.x).toBeLessThanOrEqual(1);
      expect(excess.y).toBeLessThanOrEqual(1);
    }
    const first = (await cells.first().boundingBox())!;
    const next = (await cells.nth(1).boundingBox())!;
    expect(first.width / scale).toBeLessThanOrEqual(11.1);
    expect(first.width / scale).toBeGreaterThan(8);
    expect((next.y - first.y - first.height) / first.width).toBeCloseTo(3 / 11, 1);
    await cells.last().hover();
    await expect(page.getByRole('tooltip')).toContainText('次听写');

    const recent = dashboard.locator('.ol-overview-recent');
    await recent.hover();
    await page.mouse.wheel(0, 190);
    await expect.poll(() => recent.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
    expect(await surface.evaluate((node) => node.scrollTop)).toBe(0);
    await toggle.click();
    await expect(dashboard.locator('.ol-gha-app-list')).toBeInViewport();
    await expectOverviewContained(summary);
    if (scale === 1.1) {
      await page.screenshot({
        path: testInfo.outputPath('acceptance-overview-minimum-shell-110.png'),
      });
    }
  });
}

test('a 375px desktop preview keeps one vertical scroll owner', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto('/?os=mac');
  const surface = page.locator('[data-ol-page="overview"]');
  const dashboard = surface.locator('.ol-overview-dashboard');
  await expect(dashboard).toHaveAttribute('data-mobile', 'true');
  const recent = dashboard.locator('.ol-overview-recent');
  const annual = dashboard.locator('.ol-overview-year');
  await expect(annual).toHaveCSS('position', 'static');
  await expect(dashboard.locator('.ol-gha-week')).toHaveCount(53);
  const toggle = dashboard.getByRole('button', { name: '展开常用软件' });
  await toggle.click();
  await expect(dashboard.locator('.ol-gha-app-list')).toBeVisible();
  await annual.scrollIntoViewIfNeeded();
  await expectOverviewContained(annual);
  const heatmap = dashboard.locator('.ol-gha-grid');
  await heatmap.scrollIntoViewIfNeeded();
  await expectOverviewContained(heatmap.locator('.ol-gha-week').first());
  await expectOverviewContained(heatmap.locator('.ol-gha-week').last());
  await expect.poll(() => surface.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  for (const owner of [
    page.locator('#root'),
    dashboard,
    recent,
    dashboard.locator('.ol-gha-app-list'),
  ]) {
    expect(
      await owner.evaluate((node) => node.scrollHeight - node.clientHeight),
    ).toBeLessThanOrEqual(1);
  }
});
