import { expect, test } from '@playwright/test';

test('self-hosted sync configuration only appears under experimental and extensions', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '权限与数据', exact: true }).click();
  await expect(page.getByText('启用加密同步', { exact: true }).first()).toBeVisible();
  await expect(page.getByText('自建同步服务器（高级）', { exact: true })).toBeHidden();
  await expect(page.getByLabel('登录 Token', { exact: true })).toBeHidden();
  await page.getByRole('button', { name: '实验与扩展', exact: true }).click();
  await page.locator('[data-ol-advanced-entry="customSyncServer"]').click();
  const detail = page.locator('[data-ol-advanced-page="customSyncServer"]');
  await expect(
    detail.getByRole('textbox', { name: '服务器地址（https://...）', exact: true }),
  ).toBeVisible();
  await expect(detail.getByLabel('登录 Token', { exact: true })).toHaveAttribute(
    'type',
    'password',
  );
  await expect(detail.getByRole('button', { name: '保存', exact: true })).toBeVisible();
  await expect(detail.getByText('启用加密同步', { exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '权限与数据', exact: true }).click();
  await expect(page.getByLabel('登录 Token', { exact: true })).toBeHidden();
  await expect(page.getByText('启用加密同步', { exact: true }).first()).toBeVisible();
});
