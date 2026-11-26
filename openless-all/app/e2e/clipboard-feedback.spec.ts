import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

test('remote input reports a copy only after native success and About opens the community link', async ({
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
  await settings.getByRole('button', { name: '社区 QQ 群 ↗' }).click();
  await expect.poll(() => page.evaluate(() => (window as any).startupFixture.openedUrls))
    .toEqual(['https://openless.top/zh/#qq-support']);
});
