import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

test('history detail and single/bulk selection recover complete originals', async ({ page }) => {
  await page.goto(
    `${fixtureOrigin}/e2e/fixtures/startup-gate.html?os=mac&clipboard&historyOriginals`,
  );
  await page.evaluate(() => (window as any).startupFixture.allowClipboard(true));
  await page.getByRole('button', { name: '历史', exact: true }).click();
  const history = page.locator('[data-ol-page="history"]');
  await expect(history).toHaveAttribute('data-transition-phase', 'settled');
  await expect(history.locator('[data-history-record]')).toHaveCount(2);
  await history.getByRole('button', { name: '原文', exact: true }).click();
  expect(await history.locator('.ol-history-raw-text').textContent()).toEqual(
    await page.evaluate(() => (window as any).startupFixture.originals[0]),
  );
  await history.locator('.ol-history-copy-btn').click();
  await expect
    .poll(() => page.evaluate(() => (window as any).startupFixture.copiedTexts[0]))
    .toEqual(await page.evaluate(() => (window as any).startupFixture.originals[0]));

  const copySelected = history.getByRole('button', { name: '复制所选原文', exact: true });
  await expect(copySelected).toBeDisabled();
  const checkboxes = history.getByRole('checkbox');
  await checkboxes.nth(0).check();
  await copySelected.click();
  await expect
    .poll(() => page.evaluate(() => (window as any).startupFixture.copiedTexts[1]))
    .toEqual(await page.evaluate(() => (window as any).startupFixture.originals[0]));
  await checkboxes.nth(1).check();
  await copySelected.click();
  await expect
    .poll(() => page.evaluate(() => (window as any).startupFixture.copiedTexts[2]))
    .toEqual(await page.evaluate(() => (window as any).startupFixture.originals.join('\n\n')));
  await history.locator('[data-history-record="original-1"]').click();
  await expect(history.getByRole('button', { name: '原文', exact: true }))
    .toHaveAttribute('aria-pressed', 'true');
  expect(await history.locator('.ol-history-raw-text').textContent()).toEqual(
    await page.evaluate(() => (window as any).startupFixture.originals[1]),
  );
  await page.setViewportSize({ width: 520, height: 800 });
  await expect(history.locator('.ol-history-content-scroll')).toBeVisible();
  const overflow = await history.evaluate((element) => element.scrollWidth > element.clientWidth);
  expect(overflow).toBe(false);
});
