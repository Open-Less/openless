import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

test('history rows and detail views recover complete originals and results', async ({ page }) => {
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

  await expect(history.getByRole('checkbox')).toHaveCount(0);
  await expect(history.getByRole('button', { name: '复制所选原文' })).toHaveCount(0);
  await history.getByRole('button', { name: /^选区润色/ }).click();
  await history.locator('.ol-history-copy-btn').click();
  await expect.poll(() => page.evaluate(() => (window as any).startupFixture.copiedTexts[1]))
    .toEqual(await page.evaluate(() => (window as any).startupFixture.results[0]));
  const search = history.getByRole('searchbox');
  const searchBox = await history.locator('.ol-history-search').boundingBox();
  const rowBox = await history.locator('[data-history-record]').first().boundingBox();
  expect(Math.abs(searchBox!.x - rowBox!.x)).toBeLessThan(1);
  expect(Math.abs(searchBox!.width - rowBox!.width)).toBeLessThan(1);
  await search.fill('second original');
  await expect(history.locator('[data-history-record]')).toHaveCount(1);
  await search.fill('');
  await expect(history.locator('[data-history-record]')).toHaveCount(2);
  await history.locator('[data-history-record="original-1"]').click();
  await history.getByRole('button', { name: '原文', exact: true }).click();
  expect(await history.locator('.ol-history-raw-text').textContent()).toEqual(
    await page.evaluate(() => (window as any).startupFixture.originals[1]),
  );
  await history.locator('.ol-history-copy-btn').click();
  await expect.poll(() => page.evaluate(() => (window as any).startupFixture.copiedTexts[2]))
    .toEqual(await page.evaluate(() => (window as any).startupFixture.originals[1]));
  await page.screenshot({ path: '.cache/selection-history-corrected.png' });
  await page.setViewportSize({ width: 520, height: 800 });
  await expect(history.locator('.ol-history-content-scroll')).toBeVisible();
  const overflow = await history.evaluate((element) => element.scrollWidth > element.clientWidth);
  expect(overflow).toBe(false);
});
