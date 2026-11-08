import { expect, test } from '@playwright/test';

test('usage describes retained history coverage and switches period', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '用量', exact: true }).click();
  await expect(page.getByRole('heading', { name: '用量', exact: true })).toBeVisible();
  await expect(page.getByText(/最多 200 条/)).toBeVisible();
  await expect(page.getByText('语音识别记录')).toBeVisible();
  await page.getByRole('button', { name: '近 7 天' }).click();
  await expect(page.getByRole('button', { name: '近 7 天' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
});
