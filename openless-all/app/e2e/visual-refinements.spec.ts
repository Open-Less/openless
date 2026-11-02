import { settleNavigation } from './support/navigation';
import { test, expect, type Locator } from '@playwright/test';

async function expectReadableLayout(dialog: Locator) {
  await settleNavigation(dialog);
  const scroll = dialog.locator('.ol-settings-scroll');
  expect(
    await scroll.evaluate((element) => element.scrollWidth - element.clientWidth),
  ).toBeLessThanOrEqual(1);
  const heading = dialog.getByRole('heading', { level: 2 }).first();
  await expect(heading).toBeVisible();
  expect(
    await heading.evaluate((element) => element.scrollWidth - element.clientWidth),
  ).toBeLessThanOrEqual(1);
}

for (const theme of ['dark', 'light'] as const) {
  test(`settings color and layout contracts · ${theme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
    await page.goto('/');
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '设置', exact: true });
    const navigation = dialog.getByRole('navigation', { name: '设置分类' });
    await expect(page.locator('html')).toHaveCSS('zoom', '1.1');
    await expect(page.locator('html')).toHaveCSS('color-scheme', theme);

    for (const section of ['快捷键', '外观与语言']) {
      const button = navigation.getByRole('button', { name: section, exact: true });
      await button.click();
      await expect(button).toHaveAttribute('aria-current', 'page');
      await expect(button).toHaveCSS('cursor', 'pointer');
      await expect(button).toHaveCSS(
        'color',
        theme === 'dark' ? 'rgb(255, 255, 255)' : 'rgb(0, 0, 0)',
      );
      const heading = dialog.getByRole('heading', { name: section, exact: true });
      await expect(heading).toBeVisible();
      await expectReadableLayout(dialog);
      const navigationBounds = (await navigation.boundingBox())!;
      const headingBounds = (await heading.boundingBox())!;
      expect(headingBounds.x).toBeGreaterThan(navigationBounds.x + navigationBounds.width);

      if (section === '快捷键') {
        const rows = dialog.locator('.ol-shortcut-row');
        await expect(rows.first()).toBeVisible();
        expect(await rows.count()).toBeGreaterThanOrEqual(9);
        await expect(dialog.getByText('问任何问题', { exact: true })).toBeVisible();
        const bounds = await rows.evaluateAll((elements) =>
          elements.map((element) => {
            const rect = element.getBoundingClientRect();
            return { top: rect.top, bottom: rect.bottom, height: rect.height };
          }),
        );
        for (let index = 0; index < bounds.length; index++) {
          expect(bounds[index].height).toBeGreaterThanOrEqual(40);
          if (index) expect(bounds[index].top).toBeGreaterThanOrEqual(bounds[index - 1].bottom - 1);
        }
      } else {
        const controls = ['字体系列', '界面大小', '阅读宽度'].map((name) =>
          dialog.getByRole('combobox', { name, exact: true }),
        );
        await expect(controls[1]).toContainText('110%');
        const bounds = [];
        for (const control of controls) {
          await expect(control).toBeVisible();
          bounds.push((await control.boundingBox())!);
        }
        for (let index = 0; index < bounds.length; index++) {
          expect(Math.abs(bounds[index].x - bounds[0].x)).toBeLessThanOrEqual(1);
          expect(bounds[index].height).toBeGreaterThanOrEqual(28);
          expect(bounds[index].width).toBeGreaterThanOrEqual(200);
          if (index)
            expect(bounds[index].y).toBeGreaterThan(bounds[index - 1].y + bounds[index - 1].height);
        }
      }
    }
  });
}
