import { expect, test } from '@playwright/test';

test('usage separates the dashboard, services and history', async ({ page }) => {
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
    .getByRole('button', { name: '服务与用途' })
    .click();
  await expect(page.getByRole('heading', { name: '服务与用途' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '每日记录' })).toHaveCount(0);
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '记录' })
    .click();
  const clear = page.getByRole('button', { name: '清除已记录 Token', exact: true });
  await expect(clear).toBeVisible();
  const dimensions = await clear.evaluate((node) => ({
    scroll: node.scrollHeight,
    client: node.clientHeight,
  }));
  expect(dimensions.scroll).toBeLessThanOrEqual(dimensions.client + 1);
  await page.screenshot({ path: test.info().outputPath('usage-history.png') });
  await clear.click();
  await expect(page.getByText('该时段没有已保存的服务商 token 报告。')).toBeVisible();
  await page.getByRole('button', { name: '近 7 天' }).click();
  await expect(page.getByRole('button', { name: '近 7 天' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
});
