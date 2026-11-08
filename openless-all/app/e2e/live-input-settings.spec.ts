import { expect, test } from '@playwright/test';

for (const [os, ua, platform] of [
  ['macOS', 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)', 'MacIntel'],
  ['Windows', 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)', 'Win32'],
] as const) {
  test(`live transcript input has a dedicated advanced settings page on ${os}`, async ({
    page,
  }) => {
    await page.addInitScript(
      ({ ua, platform }) => {
        Object.defineProperty(navigator, 'userAgent', { value: ua });
        Object.defineProperty(navigator, 'platform', { value: platform });
        Object.defineProperty(navigator, 'userAgentData', { value: undefined });
      },
      { ua, platform },
    );
    await page.goto('/');
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '设置', exact: true });
    await dialog.getByRole('button', { name: '高级功能', exact: true }).click();
    await dialog.locator('[data-ol-advanced-entry="liveInput"]').click();
    const panel = dialog.locator('[data-ol-advanced-page="liveInput"]');
    await expect(panel).toBeVisible();
    await expect(panel.getByText('实时原文显示在输入框', { exact: true })).toBeVisible();
  });
}
