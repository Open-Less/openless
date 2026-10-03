import { expect, test } from '@playwright/test';

test('testing features require opt-in and retain their settings across revocation', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await dialog.getByRole('button', { name: '实验与扩展', exact: true }).click();
  const beta = dialog.locator('[data-ol-advanced-entry="testingFeatures"]');
  const lessEntry = dialog.locator('[data-ol-advanced-entry="lessComputer"]');
  const consoleEntry = dialog.locator('[data-ol-advanced-entry="claudeConsole"]');
  const lessDetail = dialog.locator('[data-ol-advanced-page="lessComputer"]');
  await expect(beta).toBeVisible();
  await expect(lessEntry).toHaveCount(0);
  await expect(consoleEntry).toHaveCount(0);
  await expect(lessDetail).toHaveCount(0);
  await beta.click();
  const toggle = dialog.getByRole('switch', { name: '启用正在测试中的功能', exact: true });
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await toggle.click();
  await expect(lessEntry).toBeVisible();
  await expect(consoleEntry).toBeVisible();
  await lessEntry.click();
  await expect(lessDetail).toBeVisible();
  await lessDetail.getByRole('switch').first().click();
  await expect(lessDetail.getByRole('switch').first()).toHaveAttribute('aria-checked', 'true');
  await dialog.getByRole('button', { name: '返回正在测试中的功能', exact: true }).click();
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await expect(lessDetail).toHaveCount(0);
  await expect(lessEntry).toHaveCount(0);
  await expect(consoleEntry).toHaveCount(0);
  await dialog.getByRole('button', { name: '快捷键与选区', exact: true }).click();
  await expect(dialog.getByText('Less Computer', { exact: true })).toHaveCount(0);
  await dialog.getByRole('button', { name: '实验与扩展', exact: true }).click();
  await beta.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await toggle.click();
  await lessEntry.click();
  await expect(lessDetail.getByRole('switch').first()).toHaveAttribute('aria-checked', 'true');
  expect(errors).toEqual([]);
});
