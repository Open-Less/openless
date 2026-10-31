import { test, expect } from '@playwright/test';

test('offscreen WebGL preview releases its animation clock and resumes only when visible', async ({
  page,
}) => {
  await page.goto('/');
  const supported = await page.evaluate(() => {
    const gl = document.createElement('canvas').getContext('webgl');
    gl?.getExtension('WEBGL_lose_context')?.loseContext();
    return !!gl;
  });
  test.skip(!supported, 'WebGL unavailable');
  await page.evaluate(() => {
    const probe = window as Window & { draws: number };
    probe.draws = 0;
    const draw = WebGLRenderingContext.prototype.drawArrays;
    WebGLRenderingContext.prototype.drawArrays = function (...args) {
      probe.draws++;
      return draw.apply(this, args);
    };
  });
  await page.getByRole('button', { name: '设置', exact: true }).click();
  // The default OpenLess preview is CSS-only; opt into the WebGL style under test.
  await page
    .getByRole('group', { name: '胶囊样式' })
    .getByRole('button', { name: /Siri/ })
    .click();
  const draws = () => page.evaluate(() => (window as Window & { draws: number }).draws);
  await expect.poll(draws).toBeGreaterThan(5);
  await page.locator('.ol-settings-scroll').evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await page.waitForTimeout(300);
  const offscreen = await draws();
  await page.waitForTimeout(300);
  expect(await draws()).toBe(offscreen);
  await page.locator('.ol-settings-scroll').evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect.poll(draws).toBeGreaterThan(offscreen + 5);
});

test('pointer cancellation restores the list and never persists a partial drag', async ({
  page,
}) => {
  await page.goto('http://127.0.0.1:5326/e2e/fixtures/refinements.html');
  const handle = page.getByRole('button', { name: 'Alpha', exact: true });
  const rect = (await handle.boundingBox())!;
  await page.mouse.move(rect.x + 10, rect.y + 10);
  await page.mouse.down();
  await page.mouse.move(rect.x + 10, rect.y + 190, { steps: 6 });
  await handle.evaluate((element) =>
    element.dispatchEvent(new PointerEvent('pointercancel', { bubbles: true, pointerId: 1 })),
  );
  await page.mouse.up();
  await expect(page.locator('[data-reordering]')).toHaveCount(0);
  await expect(page.getByTestId('commits')).toHaveText('0');
  expect(
    await page
      .locator('[data-row]')
      .evaluateAll((elements) => elements.map((el) => el.getAttribute('data-row'))),
  ).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta']);
});
