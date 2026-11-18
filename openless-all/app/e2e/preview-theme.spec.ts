import { expect, test } from '@playwright/test';

test('a fixed preview theme survives reload against a different system theme', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/?os=mac');
  const root = page.locator('html');
  await expect(root).toHaveCSS('color-scheme', 'dark');

  const openAppearance = async () => {
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const settings = page.getByRole('dialog', { name: '设置', exact: true });
    await settings.getByRole('button', { name: '外观与语言', exact: true }).click();
    return settings.getByRole('button', { name: '浅色', exact: true });
  };
  await (await openAppearance()).click();
  await expect(root).toHaveCSS('color-scheme', 'light');
  await expect.poll(() => page.evaluate(() => localStorage.getItem('ol.theme'))).toBe('light');

  await page.reload();
  // Reopen settings after the preferences snapshot has had time to reconcile.
  await expect(await openAppearance()).toHaveAttribute('aria-pressed', 'true');
  await expect(root).toHaveCSS('color-scheme', 'light');
  expect(await page.evaluate(() => localStorage.getItem('ol.theme'))).toBe('light');
});
