import { test, expect } from '@playwright/test';

// Playwright selects reviewed references per browser and runner OS: system
// font rasterization is intentionally not shared between macOS and Ubuntu.

for (const theme of ['dark', 'light'] as const) {
  test(`settings visual baseline · ${theme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
    await page.goto('/');
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '设置', exact: true });
    await page.getByRole('button', { name: '快捷键', exact: true }).click();
    await expect(page.locator('.ol-shortcut-row').first()).toBeVisible();
    // Collect both independent screens on failure; soft assertions still fail CI.
    await expect.soft(dialog).toHaveScreenshot(`shortcuts-${theme}.png`, {
      animations: 'disabled',
      maxDiffPixels: 100,
      threshold: 0.1,
    });
    await page.getByRole('button', { name: '外观与语言', exact: true }).click();
    await expect(page.locator('.ol-typography')).toBeVisible();
    await expect.soft(dialog).toHaveScreenshot(`typography-${theme}.png`, {
      animations: 'disabled',
      maxDiffPixels: 100,
      threshold: 0.1,
    });
  });
}
