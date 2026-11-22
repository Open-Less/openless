import { expect, test, type Page } from '@playwright/test';
import { expectOverviewContained } from './support/overview';
import { fixtureOrigin } from './support/server';

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
  const initialApps = await page.locator('.ol-gha-app-preview').textContent();
  expect(initialHeading).toBeTruthy();
  expect(initialApps).toBeTruthy();
  const history = await navigate(page, 'history', '历史');
  await expect(history.getByRole('button', { name: '刷新', exact: true })).toHaveCount(0);
  await page.keyboard.press('Control+r');
  await expect(history).toHaveAttribute('data-transition-phase', 'settled');
  await expect(page.getByText('已刷新', { exact: true })).toHaveCount(0);
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
  await expect(page.locator('.ol-gha-app-preview')).toHaveText(initialApps!);

  await navigate(page, 'history', '历史');
  await page.getByRole('button', { name: '清空', exact: true }).click();
  const clear = page.getByRole('dialog', { name: '清空', exact: true });
  await clear.getByRole('button', { name: '清空', exact: true }).click();
  await expect(records).toHaveCount(1);
  await expect(records).toHaveAttribute('data-history-record', 'mock-quick-note');
  const tools = page.getByRole('button', { name: '工具', exact: true });
  await tools.click();
  await expect(tools).toHaveAttribute('aria-expanded', 'true');
  await tools
    .locator('..')
    .locator('.ol-sidebar-nav-group')
    .evaluate(async (group) => {
      await Promise.all(group.getAnimations().map((animation) => animation.finished));
    });
  await navigate(page, 'quickNote', '速记');
  await expect(records).toHaveAttribute('data-history-record', 'mock-quick-note');

  await navigate(page, 'overview', '概览');
  await expect(page.locator('[data-overview-record]')).toHaveCount(1);
  await expect(page.locator('.ol-gha-heading')).toHaveText(initialHeading!);
  await expect(page.locator('.ol-gha-app-preview')).toHaveText(initialApps!);

  await navigate(page, 'usage', '用量');
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '服务', exact: true })
    .click();
  await expect(page.getByRole('region', { name: '模型缓存报告列表' })).toContainText('deepseek-v3-2');
  await expect(page.getByRole('region', { name: '服务与用途列表' })).toContainText('没有可归属服务的记录');
});

test('history switches transcripts inside a fixed frame with copy below the text', async ({
  page,
}) => {
  await page.setViewportSize({ width: 980, height: 640 });
  await page.goto('/?os=mac');
  const surface = await navigate(page, 'history', '历史');
  await surface.locator('[data-history-record="mock-quick-note"]').click();

  const steps = surface.locator('.ol-history-pipeline-chip');
  await expect(steps).toHaveCount(2);
  expect((await steps.first().boundingBox())!.y).toBeCloseTo(
    (await steps.last().boundingBox())!.y,
    0,
  );
  const frame = surface.locator('.ol-history-content-frame');
  const content = frame.locator('.ol-history-content-scroll');
  const copy = frame.getByRole('button', { name: '复制', exact: true });
  await expect(frame.locator('.ol-history-content-footer').getByRole('button')).toHaveCount(1);
  const raw = frame.getByRole('button', { name: '原文', exact: true });
  const polished = frame.getByRole('button', { name: /^润色 · / });
  await expect(content).toHaveText('Bring the notes to the planning meeting.');
  await expect(content).toHaveCSS('overflow-y', 'auto');
  const anchor = (await copy.boundingBox())!;
  expect(anchor.y).toBeGreaterThan((await content.boundingBox())!.y);

  for (let attempt = 0; attempt < 3; attempt++) {
    await raw.click();
    await expect(raw).toHaveAttribute('aria-pressed', 'true');
    await expect(content).toHaveText('Remember to bring the notes to the planning meeting.');
    expect((await copy.boundingBox())!.y).toBeCloseTo(anchor.y, 0);
    await polished.click();
    await expect(polished).toHaveAttribute('aria-pressed', 'true');
    await expect(content).toHaveText('Bring the notes to the planning meeting.');
  }
});

test('a long retranscription scrolls without hiding the original transcript controls', async ({
  page,
}) => {
  await page.setViewportSize({ width: 980, height: 640 });
  await page.route('**/src/preview/history.ts*', async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      body: `${await response.text()}
previewHistory.retranscribe = () => ({
  updatedEntry: null,
  text: 'Long retranscription paragraph.\\n\\n'.repeat(35) + 'End of retranscription.'
});`,
    });
  });
  await page.goto(`${fixtureOrigin}/?os=mac`);
  const surface = await navigate(page, 'history', '历史');
  await surface.locator('[data-history-record="mock-quick-note"]').click();
  await surface.getByRole('button', { name: '录音操作', exact: true }).click();
  await page.getByRole('menuitem', { name: '重新转录', exact: true }).click();

  const result = surface.getByRole('region', { name: '重新转录', exact: true });
  await expect(result).toBeVisible();
  expect(await result.evaluate((node) => node.scrollHeight - node.clientHeight)).toBeGreaterThan(
    200,
  );
  await result.evaluate((node) => (node.scrollTop = node.scrollHeight));
  await expect(result.getByText('End of retranscription.')).toBeInViewport({ ratio: 1 });

  const frame = surface.locator('.ol-history-content-frame');
  const original = frame.getByRole('button', { name: '原文', exact: true });
  await expect(original).toBeInViewport({ ratio: 1 });
  await original.click();
  await expect(frame.locator('.ol-history-content-scroll')).toHaveText(
    'Remember to bring the notes to the planning meeting.',
  );
  await expect(frame.getByRole('button', { name: '复制', exact: true })).toBeInViewport({
    ratio: 1,
  });
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
    await expect
      .poll(() => owner.evaluate((node) => node.scrollHeight - node.clientHeight))
      .toBeLessThanOrEqual(1);
  }
});
