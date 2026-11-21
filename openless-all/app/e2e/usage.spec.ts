import { expect, test } from '@playwright/test';

test('usage separates overview, services and six purpose summaries', async ({ page }) => {
  await page.goto('/');
  const usage = page.getByRole('button', { name: '用量', exact: true });
  const settings = page.getByRole('button', { name: '设置', exact: true });
  await expect(usage).toHaveCSS(
    'font-size',
    await settings.evaluate((node) => getComputedStyle(node).fontSize),
  );
  await usage.click();
  await expect(page.getByRole('heading', { name: '用量', exact: true })).toBeVisible();
  await expect(page.getByText('语音识别记录')).toBeVisible();
  await expect(page.getByText('TTS 用量', { exact: true })).toBeVisible();
  await expect(page.getByText('已报告输入 Token', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: '每日记录' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '服务与用途' })).toHaveCount(0);
  await expect(page.locator('[data-ol-page="usage"]')).toHaveAttribute(
    'data-transition-phase',
    'settled',
  );
  await page.screenshot({ path: test.info().outputPath('usage-dashboard.png') });
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '服务', exact: true })
    .click();
  await expect(page.getByRole('heading', { name: '服务与用途' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '每日记录' })).toHaveCount(0);
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '用途', exact: true })
    .click();
  const clear = page.getByRole('button', { name: '清除已记录 Token', exact: true });
  await expect(clear).toBeVisible();
  const period = page.getByRole('group', { name: '统计时段' });
  await expect(period).toBeVisible();
  const positions = await clear.evaluate((node) => {
    const button = node.getBoundingClientRect();
    const toolbar = node.closest('.ol-usage-toolbar')!.getBoundingClientRect();
    const segment = node
      .closest('.ol-usage-toolbar')!
      .querySelector('.ol-segmented')!
      .getBoundingClientRect();
    return {
      right: toolbar.right - button.right,
      center: Math.abs(button.top + button.bottom - (segment.top + segment.bottom)) / 2,
    };
  });
  expect(positions.right).toBeLessThan(1);
  expect(positions.center).toBeLessThan(1);
  await page.setViewportSize({ width: 960, height: 600 });
  await page.locator('.ol-usage-page').evaluate((node) => {
    node.style.zoom = '1.1';
  });
  const summaries = page.getByRole('region', { name: '已报告的模型用量' });
  await expect(summaries.getByRole('heading')).toHaveCount(6);
  await expect(summaries.getByRole('heading', { name: '听写后处理' })).toBeVisible();
  await expect(summaries.getByText('缓存读取', { exact: true })).toHaveCount(6);
  await expect(summaries.getByText('缓存写入', { exact: true })).toHaveCount(6);
  await expect(summaries.getByText('缓存命中率', { exact: true })).toHaveCount(6);
  await expect(page.getByRole('region', { name: '最近记录列表' })).toHaveCount(0);
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1300, height: 835 });
  await page.locator('.ol-usage-page').evaluate((node) => {
    node.style.zoom = '1';
  });

  const dimensions = await clear.evaluate((node) => ({
    scroll: node.scrollHeight,
    client: node.clientHeight,
  }));
  expect(dimensions.scroll).toBeLessThanOrEqual(dimensions.client + 1);
  await page.screenshot({ path: test.info().outputPath('usage-history.png') });
  await clear.click();
  const confirmation = page.getByRole('dialog', { name: '清除已记录 Token' });
  await confirmation.getByRole('button', { name: '清除已记录 Token' }).click();
  await expect(summaries.locator('dd[data-empty]')).toHaveCount(30);
  await page.getByRole('button', { name: '近 7 天' }).click();
  await expect(page.getByRole('button', { name: '近 7 天' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '概览', exact: true })
    .click();
  await expect(page.getByRole('heading', { name: '每日记录' })).toBeVisible();
  await expect
    .poll(() =>
      page.locator('.ol-usage-chart-dates').evaluate((node) => node.getBoundingClientRect().bottom),
    )
    .toBeLessThan(835);
  await page.screenshot({ path: test.info().outputPath('usage-dashboard-light.png') });
});
