import { expect, test } from '@playwright/test';

test('every configurable shortcut can be cleared and stays clear after remount', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '快捷键', exact: true }).click();
  const rows = page.locator('.ol-shortcut-row[role="group"]');
  await expect(rows.first()).toBeVisible();
  const count = await rows.count();
  expect(count).toBeGreaterThanOrEqual(7);
  await expect(page.getByRole('button', { name: 'Mac 听写键', exact: true })).toHaveCount(0);
  for (let index = 0; index < count; index++) {
    const row = rows.nth(index);
    const clear = row.getByRole('button', { name: '清空', exact: true });
    await expect(clear).toBeEnabled();
    await clear.click();
    await expect(row.locator('.ol-shortcut-binding')).toHaveText('＋');
  }
  await page.getByRole('button', { name: '录音与输入', exact: true }).click();
  await expect(page.locator('.ol-shortcut-binding').first()).toHaveText('＋');
  await page.getByRole('button', { name: '快捷键', exact: true }).click();
  for (let index = 0; index < count; index++) {
    await expect(rows.nth(index).locator('.ol-shortcut-binding')).toHaveText('＋');
  }
  // A cleared key can be recorded again, rather than becoming permanently disabled.
  const trigger = rows.first().locator('.ol-shortcut-binding');
  await trigger.click();
  const popup = page.locator('.ol-shortcut-popup[data-open]');
  await expect(popup).toBeVisible();
  await expect(async () => {
    const anchor = (await trigger.boundingBox())!;
    const bounds = (await popup.boundingBox())!;
    expect(bounds.y).toBeGreaterThanOrEqual(anchor.y + anchor.height - 1);
    expect(bounds.x).toBeLessThan(anchor.x + anchor.width);
    expect(bounds.x + bounds.width).toBeGreaterThan(anchor.x);
  }).toPass();
  await page.keyboard.press('F8');
  await expect(rows.first().locator('.ol-shortcut-binding')).toContainText('F8');
});

for (const scale of [0.9, 1, 1.1]) {
  test(`standard and searchable menus open below at ${scale * 100}%`, async ({ page }) => {
    await page.setViewportSize({ width: 1100, height: 1200 });
    await page.goto('http://127.0.0.1:5326/e2e/fixtures/refinements.html');
    await page.evaluate((scale) => {
      document.documentElement.style.zoom = String(scale);
      document.documentElement.style.setProperty('--ol-interface-scale', String(scale));
      document.querySelector('main')!.style.minHeight = '100vh';
    }, scale);
    for (const label of ['Long menu', 'Searchable menu']) {
      const trigger = page.getByRole('combobox', { name: label, exact: true });
      await trigger.click();
      const popup = page.locator('.ol-select-popup[data-open]');
      await expect(popup).toBeVisible();
      await expect(async () => {
        const anchor = (await trigger.boundingBox())!;
        const bounds = (await popup.boundingBox())!;
        expect(bounds.y).toBeGreaterThanOrEqual(anchor.y + anchor.height - 1);
        expect(Math.abs(bounds.x - anchor.x)).toBeLessThan(2);
      }).toPass({ timeout: 3000 });
      await page.keyboard.press('Escape');
      await expect(popup).toBeHidden();
    }
  });
}
