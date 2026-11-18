import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

test('remote input and About only report a copy once the native clipboard accepts it', async ({
  page,
}) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/startup-gate.html?os=win&clipboard`);
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const settings = page.getByRole('dialog', { name: '设置', exact: true });
  await settings.getByRole('button', { name: /^手机与远程输入/ }).click();
  const fingerprintCopy = settings.getByRole('button', { name: '复制完整指纹' });
  await fingerprintCopy.click();
  await expect(page.locator('.ol-notification-card').filter({ hasText: '操作失败' })).toBeVisible();
  await expect(fingerprintCopy).toBeVisible();

  await page.evaluate(() => (window as any).startupFixture.allowClipboard(true));
  await fingerprintCopy.click();
  await expect(settings.getByRole('button', { name: '指纹已复制' })).toBeVisible();

  await settings
    .getByRole('navigation', { name: '设置分类' })
    .getByRole('button', { name: '关于与更新', exact: true })
    .click();
  const code = settings.locator('kbd').getByText('1078960553');
  const qq = code.locator('..').getByRole('button');
  await page.evaluate(() => (window as any).startupFixture.allowClipboard(false));
  await qq.click();
  await expect(settings.getByText('已复制', { exact: true })).toHaveCount(0);
  await page.evaluate(() => (window as any).startupFixture.allowClipboard(true));
  await qq.click();
  await expect(settings.getByText('已复制', { exact: true })).toBeVisible();
});
