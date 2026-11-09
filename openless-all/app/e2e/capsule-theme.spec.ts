import { expect, test, type Page } from '@playwright/test';

const fixture = 'http://127.0.0.1:5326/e2e/fixtures/capsule-motion.html?themeSync=1';
async function preference(page: Page, theme: 'light' | 'dark' | 'system') {
  await page.evaluate((theme) => (window as any).fixture.setTheme(theme), theme);
}

test('an already-mounted capsule and badge follow preference events without restarting', async ({
  page,
}) => {
  await page.goto(fixture);
  await page.waitForFunction(() => (window as any).fixture.themeReads > 0);
  const pill = page.locator('.ol-default-capsule');
  await page.evaluate(() => {
    (window as any).themePill = document.querySelector('.ol-default-capsule');
    (window as any).fixture.setCapsule({ translation: true });
  });
  for (const theme of ['dark', 'light', 'dark'] as const) {
    await preference(page, theme);
    await expect(pill).toHaveCSS('color-scheme', theme);
    await expect(page.locator('.ol-classic-translation')).toHaveCSS('color-scheme', theme);
    await expect(pill.locator('.ol-default-waveform > span').first()).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(167, 199, 255)' : 'rgb(29, 78, 216)',
    );
  }
  expect(
    await page.evaluate(
      () => document.querySelector('.ol-default-capsule') === (window as any).themePill,
    ),
  ).toBe(true);
  expect(await page.evaluate(() => (window as any).fixture.nativeThemeWrites)).toEqual([]);
});

test('a hidden capsule receives theme changes before the next recording', async ({ page }) => {
  await page.goto(fixture + '&full=1');
  await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
  await page.evaluate(() => (window as any).fixture.emit('capsule:state', { state: 'idle' }));
  await expect(page.locator('.ol-default-capsule')).toHaveCount(0);
  await preference(page, 'dark');
  await expect(page.locator('html')).toHaveAttribute('data-ol-theme', 'dark');
  await page.evaluate(() =>
    (window as any).fixture.emit('capsule:state', {
      state: 'recording',
      level: 0.2,
      capsuleStyle: 'classic',
    }),
  );
  await expect(page.locator('.ol-default-capsule')).toHaveCSS('color-scheme', 'dark');
});

test('late startup reads cannot overwrite a newer capsule theme', async ({ page }) => {
  await page.goto(fixture + '&holdThemeRead=1');
  await page.waitForFunction(() => (window as any).fixture.themeReads >= 1);
  await preference(page, 'dark');
  await page.evaluate(() => (window as any).fixture.releaseThemeRead());
  await page.waitForTimeout(100);
  await expect(page.locator('.ol-default-capsule')).toHaveCSS('color-scheme', 'dark');
  expect(await page.evaluate(() => localStorage.getItem('ol.theme'))).toBe('dark');
});

test('live system theme tracking can be enabled and disabled without reloading a capsule', async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto(fixture);
  await page.waitForFunction(() => (window as any).fixture.themeReads >= 1);
  await preference(page, 'system');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('.ol-default-capsule')).toHaveCSS('color-scheme', 'dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('.ol-default-capsule')).toHaveCSS('color-scheme', 'light');
  await preference(page, 'dark');
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('.ol-default-capsule')).toHaveCSS('color-scheme', 'dark');
  await page.evaluate(() =>
    (window as any).fixture.emit('prefs:changed', { capsuleStyle: 'classic' }),
  );
  await expect(page.locator('.ol-default-capsule')).toHaveCSS('color-scheme', 'dark');
});
