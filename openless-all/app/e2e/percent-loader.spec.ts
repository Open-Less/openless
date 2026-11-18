import { expect, test } from '@playwright/test';

import { fixtureOrigin } from './support/server';

function percent(text: string) {
  return Number(text.replace('%', ''));
}

test('the counting percent loader covers startup and stays off settled pages', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/percent-loader.html`);
  const startup = page.locator('.ol-startup [role="progressbar"]');
  await expect(startup).toBeVisible();
  await expect(startup).toHaveAttribute('aria-label', 'Loading');
  await expect(startup).not.toHaveAttribute('aria-valuenow');
  const before = percent(await startup.locator('.ol-percent-loader-label').innerText());
  await expect
    .poll(async () => percent(await startup.locator('.ol-percent-loader-label').innerText()))
    .not.toBe(before);
  const sample = await startup.evaluate((node) => {
    const label = node.querySelector('.ol-percent-loader-label')?.textContent?.trim() ?? '';
    const fill = node.querySelector('.ol-percent-loader-fill');
    return { label, width: fill instanceof HTMLElement ? fill.style.width : '' };
  });
  expect(sample.width).toBe(sample.label);
  expect(sample.label).toMatch(/^\d+%$/);

  const cover = page.locator('.ol-startup');
  await expect(cover).toHaveCSS('position', 'fixed');
  await expect(cover).toHaveCSS('backdrop-filter', /blur/);
  await page.getByRole('button', { name: 'Switch page' }).click();
  await expect(page.locator('.ol-transition-viewport .ol-percent-loader')).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Arrived' })).toBeVisible();
  await expect(page.locator('.ol-transition-viewport .ol-percent-loader')).toHaveCount(0);
});
