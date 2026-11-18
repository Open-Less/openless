import { expect, test, type Locator, type Page } from '@playwright/test';
import { fixtureOrigin } from './support/server';

const url = `${fixtureOrigin}/e2e/fixtures/sliders.html`;
async function control(slider: Locator) {
  return slider.locator('xpath=ancestor::div[contains(@class,"ol-slider-control")]');
}
async function drag(page: Page, slider: Locator, fraction: number) {
  const target = await control(slider);
  await target.scrollIntoViewIfNeeded();
  const bounds = await target.boundingBox();
  if (!bounds) throw new Error('Slider control has no bounds');
  await page.mouse.move(bounds.x + bounds.width * 0.25, bounds.y + bounds.height / 2);
  await page.mouse.down();
  await page.mouse.move(bounds.x + bounds.width * fraction, bounds.y + bounds.height / 2, {
    steps: 8,
  });
}

test('all reference compositions render and produce light and dark acceptance images', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(url);
  await expect(page.getByTestId('gallery').getByRole('slider')).toHaveCount(15);
  for (const theme of ['light', 'dark']) {
    await expect(page.getByRole('slider', { name: 'Solid', exact: true })).toHaveAccessibleName(
      'Solid',
    );
    await page
      .locator('[data-testid="gallery"]')
      .screenshot({ path: test.info().outputPath(`acceptance-slider-${theme}.png`) });
    await page.getByRole('button', { name: 'Toggle theme' }).click();
  }
  const solid = page.getByRole('slider', { name: 'Solid', exact: true });
  const root = solid.locator('xpath=ancestor::div[contains(@class,"ol-slider--solid")]');
  await expect(root.locator('.ol-slider-indicator')).toHaveCSS('opacity', '0.7');
  expect(
    await root.locator('.ol-slider-thumb').evaluate((node) => {
      const style = getComputedStyle(node);
      return style.backgroundColor === style.borderTopColor;
    }),
  ).toBe(true);
  expect(errors).toEqual([]);
});

test('pointer movement previews continuously and commits once at release', async ({ page }) => {
  await page.goto(url);
  const solid = page.getByRole('slider', { name: 'Solid', exact: true });
  await drag(page, solid, 0.8);
  const events = () => page.getByTestId('events').evaluate((node) => JSON.parse(node.textContent!));
  expect((await events()).changes.length).toBeGreaterThan(1);
  expect((await events()).commits).toEqual([]);
  await page.mouse.up();
  await expect.poll(async () => (await events()).commits.length).toBe(1);
  const final = Number(await solid.inputValue());
  expect(final).toBeGreaterThanOrEqual(79);
  expect(final).toBeLessThanOrEqual(81);
  expect((await events()).commits).toEqual([final]);
  await page.getByRole('button', { name: 'External values' }).click();
  await expect(solid).toHaveValue('60');
  expect((await events()).commits).toEqual([final]);
});

test('native keyboard, decimals, RTL, vertical and range bounds stay accessible', async ({
  page,
}) => {
  await page.goto(url);
  const simple = page.getByRole('slider', { name: 'Simple', exact: true });
  await simple.focus();
  await simple.press('Home');
  await expect(simple).toHaveValue('0');
  await simple.press('End');
  await expect(simple).toHaveValue('100');
  await simple.press('PageDown');
  await expect(simple).toHaveValue('90');
  const rtl = page.getByRole('slider', { name: 'RTL decimal' });
  await rtl.focus();
  await rtl.press('ArrowRight');
  await expect(rtl).toHaveValue('0.1');
  await rtl.press('ArrowLeft');
  await expect(rtl).toHaveValue('0.2');
  const vertical = page.getByRole('slider', { name: 'Vertical', exact: true });
  await expect(vertical).toHaveAttribute('aria-orientation', 'vertical');
  await vertical.press('ArrowUp');
  await expect(vertical).toHaveValue('6');
  const verticalControl = await control(vertical);
  await verticalControl.scrollIntoViewIfNeeded();
  const verticalBounds = await verticalControl.boundingBox();
  if (!verticalBounds) throw new Error('Missing vertical control');
  expect(verticalBounds.height).toBeGreaterThan(verticalBounds.width);
  await page.mouse.click(
    verticalBounds.x + verticalBounds.width / 2,
    verticalBounds.y + verticalBounds.height * 0.2,
  );
  await expect(vertical).toHaveValue('8');
  const minimum = page.getByRole('slider', { name: 'Range minimum', exact: true });
  const maximum = page.getByRole('slider', { name: 'Range maximum', exact: true });
  await minimum.press('End');
  expect(Number(await minimum.inputValue())).toBeLessThanOrEqual(
    Number(await maximum.inputValue()),
  );
  await expect(page.getByRole('slider', { name: 'Disabled', exact: true })).toBeDisabled();
});

test('stepper and distribution inputs clamp, round, cancel and follow external values', async ({
  page,
}) => {
  await page.goto(url);
  const credits = page.getByRole('slider', { name: 'Credits' });
  await page.getByRole('button', { name: 'Increase value' }).click();
  await expect(credits).toHaveValue('5.25');
  await credits.press('End');
  await expect(page.getByRole('button', { name: 'Increase value' })).toBeDisabled();
  const low = page.getByRole('textbox', { name: 'Minimum price' });
  const high = page.getByRole('textbox', { name: 'Maximum price' });
  await low.fill('22.3');
  await low.press('Enter');
  await expect(low).toHaveValue('22.5');
  await expect(page.getByTestId('distribution-commits')).toHaveText('1');
  await high.fill('invalid');
  await high.press('Tab');
  await expect(high).toHaveValue('80');
  await low.fill('65');
  await low.press('Escape');
  await expect(low).toHaveValue('22.5');
  await expect(page.getByTestId('distribution-commits')).toHaveText('1');
  await high.fill('-100');
  await high.press('Enter');
  await expect(high).toHaveValue('22.5');
  await expect(page.getByTestId('distribution-commits')).toHaveText('2');
  await low.fill('99');
  await low.press('Enter');
  await expect(low).toHaveValue('22.5');
  await expect(page.getByTestId('distribution-commits')).toHaveText('2');
  await page.getByRole('button', { name: 'External values' }).click();
  await expect(low).toHaveValue('30');
  await expect(high).toHaveValue('70');
});

test('production DSP commits the latest draft once and preserves equalizer bands', async ({
  page,
}) => {
  await page.goto(url);
  const gain = page.getByRole('slider', { name: 'Input gain', exact: true });
  await drag(page, gain, 0.7);
  await expect(page.getByTestId('dsp-commits')).toHaveText('0');
  await page.mouse.up();
  await expect(page.getByTestId('dsp-commits')).toHaveText('1');
  const gainValue = Number(await gain.inputValue());
  await page.getByText('Tone and room adjustments', { exact: true }).click();
  const band = page.getByRole('slider', { name: 'Equalizer 31 Hz', exact: true });
  await expect(band).toBeVisible();
  await band.press('ArrowRight');
  await expect(page.getByTestId('dsp-commits')).toHaveText('2');
  const saved = await page
    .getByTestId('dsp-value')
    .evaluate((node) => JSON.parse(node.textContent!));
  expect(saved.gainDb).toBe(gainValue);
  expect(saved.equalizerBandsDb).toEqual([1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
});

test.describe('touch gestures', () => {
  test.use({ hasTouch: true });
  test('real touch moves a slider and commits once', async ({ page, browserName }) => {
    test.skip(browserName !== 'chromium', 'Chromium CDP supplies Android WebView touch gestures');
    await page.goto(url);
    const slider = page.getByRole('slider', { name: 'Solid', exact: true });
    const target = await control(slider);
    await target.scrollIntoViewIfNeeded();
    await expect(target).toBeInViewport({ ratio: 1 });
    const bounds = await target.boundingBox();
    if (!bounds) throw new Error('Missing touch target');
    const session = await page.context().newCDPSession(page);
    try {
      await session.send('Input.dispatchTouchEvent', {
        type: 'touchStart',
        touchPoints: [{ x: bounds.x + bounds.width * 0.25, y: bounds.y + bounds.height / 2 }],
      });
      for (const fraction of [0.35, 0.5, 0.65, 0.8]) {
        await session.send('Input.dispatchTouchEvent', {
          type: 'touchMove',
          touchPoints: [{ x: bounds.x + bounds.width * fraction, y: bounds.y + bounds.height / 2 }],
        });
        await page.waitForTimeout(20);
      }
      await expect.poll(async () => Number(await slider.inputValue())).toBeGreaterThan(70);
      const value = Number(await slider.inputValue());
      const commits = async (): Promise<number[]> =>
        JSON.parse(await page.getByTestId('events').innerText()).commits;
      expect(await commits()).toEqual([]);
      await session.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await expect.poll(commits).toEqual([value]);
    } finally {
      await session.detach();
    }
  });
});
