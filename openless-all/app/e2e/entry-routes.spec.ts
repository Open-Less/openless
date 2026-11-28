import { expect, test } from '@playwright/test';

const profiles = [
  { os: 'mac', width: 1400, height: 900 },
  { os: 'win', width: 1240, height: 800 },
  { os: 'android', width: 412, height: 892 },
  { os: 'harmony', width: 420, height: 900 },
] as const;

test('framed browser profiles use their content size and preserve unscaled root coordinates', async ({
  page,
}) => {
  await page.setViewportSize({ width: 800, height: 700 });
  await page.addInitScript(() => {
    localStorage.setItem('ol-font-scale', 'large');
    localStorage.setItem('ol.theme', 'light');
  });

  for (const { os, width, height } of profiles) {
    await page.goto(`/?os=${os}&frame=1`);
    const frame = page.locator('.ol-preview-frame');
    const boundary = page.locator('.ol-app-viewport');
    await expect(page.locator('html')).toHaveAttribute('data-ol-platform', os);
    await expect(boundary.locator('.ol-winchrome')).toBeVisible();
    expect(await boundary.boundingBox()).toMatchObject({ width, height });
    expect(await frame.boundingBox()).toMatchObject({ width, height });
    expect(
      await boundary.evaluate((element) =>
        getComputedStyle(element).getPropertyValue('--ol-interface-scale').trim(),
      ),
    ).toBe('1.1');
    expect(
      await page.locator('html').evaluate((element) => element.style.getPropertyValue('zoom')),
    ).toBe('');
    if (width > 800) expect((await boundary.boundingBox())?.x).toBe(0);
    const mobileNav = boundary.locator('.ol-mobile-bottom-nav');
    await expect(mobileNav).toHaveCount(os === 'android' || os === 'harmony' ? 1 : 0);
  }
});

test('auxiliary routes ignore the main window zoom preference and preview frame', async ({
  page,
}) => {
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'large'));
  await page.goto('/?window=voice-edit&frame=1');
  await expect(page.locator('.ol-voice-edit-window')).toBeVisible();
  await expect(page.locator('.ol-preview-frame')).toHaveCount(0);
  await expect(page.locator('.ol-app-viewport')).toHaveAttribute('data-ol-scale', 'fixed');
});
