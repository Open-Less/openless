import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

for (const colorScheme of ['light', 'dark'] as const) {
  test.describe(`Linux surface backgrounds (${colorScheme})`, () => {
    test.use({ colorScheme });
    for (const kind of ['capsule', 'capsule-rail', 'voice-edit']) {
      test(`${kind} keeps html/body/root transparent`, async ({ page }) => {
        // Exercise the actual bootstrap and stylesheet, not a duplicated CSS fixture.
        await page.goto(`${fixtureOrigin}/?bridge=http&window=${kind}`);
        await expect(page.locator('html')).toHaveAttribute('data-ol-platform', 'linux');
        await expect(page.locator('html')).toHaveAttribute('data-ol-aux', '');
        for (const selector of ['html', 'body', '#root']) {
          await expect(page.locator(selector)).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
        }
      });
    }
    test('main window retains an opaque background', async ({ page }) => {
      await page.goto(`${fixtureOrigin}/?bridge=http`);
      await expect(page.locator('html')).toHaveAttribute('data-ol-platform', 'linux');
      await expect(page.locator('html')).not.toHaveAttribute('data-ol-aux');
      for (const selector of ['html', 'body', '#root']) {
        await expect(page.locator(selector)).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
      }
    });
  });
}
