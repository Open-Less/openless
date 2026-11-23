import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';
import { settleNavigation } from './support/navigation';

test('general translation page saves counts and explicitly saves the default', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/microphone-entry.html`);
  await page.evaluate(() => window.microphoneEntryFixture.releaseCapabilities());
  const entry = page.locator('[data-ol-general-entry="translation"]');
  await expect(entry.locator('.ol-settings-category-icon svg')).toBeVisible();
  expect(
    await page.evaluate(() =>
      Object.hasOwn(window.microphoneEntryFixture.settings(), 'translationShiftTapCount'),
    ),
  ).toBe(false);
  await entry.click();
  await settleNavigation(page);
  const count = page.getByRole('combobox', { name: 'Shift tap count', exact: true });
  await expect(count).toBeVisible();
  await expect(count).toContainText('1 tap');
  await expect(page.getByText(/Applies to the default Shift translation shortcut/)).toBeVisible();
  for (const value of [2, 3, 1]) {
    await count.click();
    await page
      .getByRole('option', { name: `${value} ${value === 1 ? 'tap' : 'taps'}`, exact: true })
      .click();
    await expect(count).toContainText(`${value} ${value === 1 ? 'tap' : 'taps'}`);
    await expect
      .poll(() =>
        page.evaluate(() => window.microphoneEntryFixture.settings().translationShiftTapCount),
      )
      .toBe(value);
  }
  await expect(count).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('.ol-select-popup')).toBeHidden();
  await page.screenshot({ path: test.info().outputPath('translation-settings.png') });
  await page.setViewportSize({ width: 760, height: 835 });
  await expect(count).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true,
  );
  await page.screenshot({ path: test.info().outputPath('translation-settings-narrow.png') });
});
