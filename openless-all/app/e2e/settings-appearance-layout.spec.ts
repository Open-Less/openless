import { expect, test, type Locator } from '@playwright/test';
import { settleNavigation } from './support/navigation';

async function expectReadableText(locator: Locator) {
  await expect(locator).toBeVisible();
  const fits = await locator.evaluate((element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    const bounds = element.getBoundingClientRect();
    const runs = Array.from(range.getClientRects());
    return (
      runs.length > 0 &&
      runs.every((run) => run.left >= bounds.left - 1 && run.right <= bounds.right + 1)
    );
  });
  expect(fits).toBe(true);
}

for (const profile of [
  {
    locale: 'en',
    width: 980,
    scale: 'large',
    theme: 'dark',
    settings: 'Settings',
    appearance: 'Appearance & language',
    stacked: false,
  },
  {
    locale: 'de',
    width: 810,
    scale: 'large',
    theme: 'light',
    settings: 'Einstellungen',
    appearance: 'Darstellung und Sprache',
    stacked: true,
  },
  {
    locale: 'zh-CN',
    width: 930,
    scale: 'small',
    theme: 'light',
    settings: '设置',
    appearance: '外观与语言',
    stacked: false,
  },
]) {
  test(`appearance keeps its translated headings and select sizes at ${profile.locale} / ${profile.scale}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: profile.width, height: 860 });
    await page.addInitScript(({ locale, scale, theme }) => {
      localStorage.setItem('ol.locale', locale);
      localStorage.setItem('ol-font-scale', scale);
      localStorage.setItem('ol.theme', theme);
    }, profile);
    await page.goto('/?os=mac');
    await page.getByRole('button', { name: profile.settings, exact: true }).click();
    const dialog = page.getByRole('dialog', { name: profile.settings, exact: true });
    const rail = dialog.getByRole('button', { name: profile.appearance, exact: true });
    await rail.click();
    await settleNavigation(dialog);
    await expect(rail).toHaveAttribute('aria-current', 'page');
    await expectReadableText(rail.locator('.ol-settings-rail-label'));
    const title = dialog.getByRole('heading', { name: profile.appearance, exact: true });
    const saveNote = dialog.locator('.ol-settings-auto-save');
    await expectReadableText(title);
    await expectReadableText(saveNote);
    await expect(saveNote).toBeInViewport({ ratio: 1 });

    const form = dialog.locator('[data-section="appearance"]');
    const selects = form.locator('.ol-setting-row-control > .ol-select-trigger');
    await expect(selects).toHaveCount(4);
    const geometry = await selects.evaluateAll((elements) =>
      elements.map((element) => {
        const control = element.getBoundingClientRect();
        const label = element.closest('.ol-setting-row')!
          .querySelector('.ol-setting-row-label')!.getBoundingClientRect();
        return {
          x: control.x,
          width: control.width,
          stacked: control.top >= label.bottom - 1,
        };
      }),
    );
    const expectedWidth = 280 * (profile.scale === 'large' ? 1.1 : 0.9);
    for (const { x, width, stacked } of geometry) {
      expect(Math.abs(x - geometry[0].x)).toBeLessThan(1);
      expect(Math.abs(width - expectedWidth)).toBeLessThan(1);
      expect(stacked).toBe(profile.stacked);
    }
  });
}
